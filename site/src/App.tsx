import React, {
  ReactElement,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import "./App.css"; // Ensure to import the CSS file
import {
  ViewUpdate,
  EditorView,
  ChangeSpec,
  TransactionSpec,
  AnnotationType,
  ChangeSet,
} from "@uiw/react-codemirror";
import {
  OpState,
  TextUpdateRange,
  TextUpdate,
  CursorPos,
  ClientResponse,
  ClientResponseType,
  UserList,
  UserInner,
  UserCursorPos,
  RemoteDocUpdate,
  Client,
} from "corust-components/corust_components.js";
import { useParams } from "react-router-dom";
import {
  Alert,
  Snackbar,
  Grow,
  Box,
  IconButton,
  useTheme,
} from "@mui/material";
import { Close as CloseIcon } from "@mui/icons-material";
import { SnackbarProvider, enqueueSnackbar, closeSnackbar } from "notistack";
import {
  RunOutput,
  RunStatus,
  updateRunStatus,
  ServerRunStatus,
} from "./components/editor/runOutputDisplay.tsx";
import HeaderBar from "./components/headerBar/headerBar.tsx";
import RunButton from "./components/headerBar/runButton.tsx";
import RunConfigButtons from "./components/headerBar/runConfigButtons.tsx";
import {
  CargoCommand,
  setCargoCommand,
  setLastExecuteCargoCommand,
} from "./store/slices/cargoCommandSlice.tsx";
import { useDispatch, useSelector } from "react-redux";
import { RootState } from "./store/store.tsx";
import EditorContainer from "./components/editor/container.tsx";
import {
  OptLevel,
  setLastExecuteOptLevel,
  setOptLevel,
} from "./store/slices/optSlice.tsx";
import {
  RustChannel,
  setChannel,
  setLastExecuteChannel,
} from "./store/slices/channelSlice.tsx";
import {
  setExecutingUser,
  setLastExecutionCode,
} from "./store/slices/codeSelectorSlice.tsx";
import { UserStateDefined } from "./store/slices/userSlice.tsx";

// Interfaces/Type definitions

// Websocket message variants
// Commands with a `type`, which can be deserialized by
// the server into a corresponding Rust struct.
type WsClientTextMsg = WsRustDocUpdate | WsRustExecuteCommand | WsConfigUpdate;

enum WsClientTextMsgType {
  WsDocUpdate = "wsDocUpdate",
  WsExecuteCommand = "wsExecuteCommand",
  WsConfigUpdate = "wsConfigUpdate",
}

interface WsRustDocUpdate {
  type: WsClientTextMsgType.WsDocUpdate;
  docUpdate: string;
}

interface WsRustExecuteCommand {
  type: WsClientTextMsgType.WsExecuteCommand;
  code: string;
  targetType: TargetType;
  cargoCommand: CargoCommand;
  optLevel: OptLevel;
  channel: RustChannel;
}

// Represents an update to the execution configuration
interface WsConfigUpdate {
  type: WsClientTextMsgType.WsConfigUpdate;
  cargoCommand: CargoCommand;
  optLevel: OptLevel;
  channel: RustChannel;
  username: string;
}

// Describes the most recently executed code.
type RunConfigAction = RecentExecutionConfig | ConfigUpdate;

interface RecentExecutionConfig {
  type: RunConfigType.RecentExecution;
  cargoCommand: CargoCommand;
  optLevel: OptLevel;
  channel: RustChannel;
  code: string;
  // User who ran the code
  username: string;
}

interface ConfigUpdate {
  type: RunConfigType.ConfigUpdate;
  cargoCommand: CargoCommand;
  optLevel: OptLevel;
  channel: RustChannel;
  username: string;
  // No `recentRunCode` field
}

// Represents a configuration to run code
interface RunConfig {
  cargoCommand: CargoCommand;
  optLevel: OptLevel;
  channel: RustChannel;
}

enum RunConfigType {
  RecentExecution = "RecentExecution",
  ConfigUpdate = "ConfigUpdate",
}

// Document update, with additional metadata
interface DocUpdateWrapper {
  inner: BroadcastLocalDocUpdate;
  // Whether update is unsent, pending, or acked
  opState: OpState;
}

interface ExecuteCommand {
  code: string;
  targetType: TargetType;
  cargoCommand: CargoCommand;
  optLevel: OptLevel;
  channel: RustChannel;
}

enum TargetType {
  Library = "Library",
  Binary = "Binary",
}

// Document update, without metadata. These fields are sent to the server.
interface BroadcastLocalDocUpdate {
  // Serialized Rust doc update, directly passed to server
  // Avoids sending complex types like cursor maps to JS.
  docUpdate: string;
}

// Code section
interface CodeContainerText {
  code: string;
}

enum ContainerMessageType {
  // `cargo run/build/test` command
  Execute = "Execute",
  // Future
  Clippy = "Clippy",
}

// Represents a message to execute code in the container
type ContainerMessage = {
  [key in ContainerMessageType]: ExecuteCommand;
};

interface CodeOutputState {
  containerMsg: ContainerMessage;
  runnerOutput?: RunOutput;
}

// Snapshot sent by server on user join
interface Snapshot {
  source: number;
  dest: number;
  document: String;
  // Opaque Rust struct that does not need to be accessed
  cursorMap: any;
  stateId: number;
  runConfig: RunConfig;
  /// Optional - Present if the code has been executed before
  codeOutputState?: CodeOutputState;
}

// Text selection range (start <= end)
// Either `from === anchor && to === head` or `from === head && to === anchor`
interface SelectionFocused {
  from: number;
  to: number;
  // Side of selection which does not change
  anchor: number;
  // Moved when selection is extended
  head: number;
}

// Fields `undefined` when user cursor is not focused on the text box
interface SelectionUnfocused {
  from: undefined;
  to: undefined;
  anchor: undefined;
  head: undefined;
}

type SelectionRange = SelectionFocused | SelectionUnfocused;

interface UserSelectionRange {
  userId: bigint;
  selection: SelectionRange;
}

interface AppProps {
  currUser: UserStateDefined;
}

enum ServerMessageType {
  RemoteUpdate = "RemoteUpdate",
  Run = "Run",
  RunStatus = "RunStatus",
  RunConfigAction = "RunConfigAction",
  Snapshot = "Snapshot",
  UserList = "UserList",
}

// Utilities
const docUpdateToRust = (msg: DocUpdateWrapper): WsRustDocUpdate => {
  return {
    type: WsClientTextMsgType.WsDocUpdate,
    docUpdate: msg.inner.docUpdate,
  };
};

const executeCommandToObj = (msg: ExecuteCommand): WsRustExecuteCommand => {
  return {
    type: WsClientTextMsgType.WsExecuteCommand,
    code: msg.code,
    targetType: msg.targetType,
    cargoCommand: msg.cargoCommand,
    optLevel: msg.optLevel,
    channel: msg.channel,
  };
};

// https://github.com/rust-lang/rust-playground/blob/main/ui/frontend/selectors/index.ts
const HAS_MAIN_FUNCTION_RE = new RegExp(
  [
    /^([^\n\r\/]*;)?/,
    /\s*(pub\s+)?\s*(const\s+)?\s*(async\s+)?\s*/,
    /fn\s+main\s*\(\s*(\/\*.*\*\/)?\s*\)/,
  ]
    .map((r) => r.source)
    .join(""),
  "m"
);

function App({ currUser }: AppProps) {
  const dispatch = useDispatch();
  const theme = useTheme();
  // Maximum 1000 document updates a minute.
  // For reference, 1000 character updates per minute is a typing speed of ~200 words per minute.
  const maxUpdatesPerMinute = 1000;
  // Maximum cumulative size of documents (in characters) sent per minute.
  // For reference, 1k lines should have at most ~50k characters. It is unlikely a user
  // will repeatedly copy and delete such large code blocks.
  const maxDocSizePerMinute = 200000;
  // Route params
  const params = useParams();
  // CargoOutput has schema: Object {stdout: string, stderr: string, status: number}
  const [runOutput, setRunOutput] = useState<RunOutput | null>(null);
  const [runStatus, setRunStatus] = useState<RunStatus | null>(null);
  const [showCargoOutput, setShowCargoOutput] = useState<boolean>(false);
  const [cargoOutputOpen, setCargoOutputOpen] = useState<boolean>(true);
  const [codeContainerText, setCodeContainerText] = useState<CodeContainerText>(
    {
      code: "",
    }
  );
  const [collabSelections, setCollabSelections] = useState<
    UserSelectionRange[]
  >([]);

  // `cargo` configuraiton
  const [targetType, setTargetType] = useState<TargetType>(TargetType.Library);
  const cargoCommand = useSelector(
    (state: RootState) => state.cargoCommandSelector.command
  );
  const optLevel = useSelector((state: RootState) => state.optSelector.level);
  const channel = useSelector(
    (state: RootState) => state.channelSelector.channel
  );
  const cargoCommandRef = useRef(cargoCommand);
  const optLevelRef = useRef(optLevel);
  const channelRef = useRef(channel);
  const [stableVersion, setStableVersion] = useState<string>("1.82.0");
  const [betaVersion, setBetaVersion] = useState<string>("1.82.0");
  const [nightlyVersion, setNightlyVersion] = useState<string>("1.82.0");

  const [userArr, setUserArr] = useState<UserInner[]>([]);
  const [wsOpen, setWsOpen] = useState<boolean>(true);
  const [wsDisconnectMsg, setWsDisconnectMsg] = useState<String>(
    "Disconnected from server. Please refresh the page to rejoin."
  );
  const [remoteAnnotationType] = useState<AnnotationType<boolean>>(
    new AnnotationType()
  );
  // The client object should be created once per component render. It cannot be passed in as
  // a prop and modified in place, otherwise the component would be impure.
  const [client] = useState<Client>(
    Client.new(currUser.userId, maxUpdatesPerMinute, maxDocSizePerMinute)
  );
  // CodeMirror view
  const [view, setView] = useState<EditorView | undefined>(undefined);
  // Event listeners capture static state, so we need to use refs for indirection to the latest state
  const cargoOutputRef = useRef(runOutput);
  const codeContainerTextRef = useRef(codeContainerText);
  const clientRef = useRef(client);
  const ws = useRef<WebSocket | null>(null);

  // Rust `ServerMessage` is serialized as a string
  type ServerMessage = string;

  const dispatchTransaction = useCallback(
    (textUpdates: TextUpdate[]) => {
      // Convert text updates into a transaction spec to update the editor
      console.debug("view at dispatch transaction: ", view);
      if (view) {
        const changeSpec: ChangeSpec = textUpdates.map((textUpdate) => {
          return {
            from: textUpdate.prev().from(),
            to: textUpdate.prev().to(),
            insert: textUpdate.text(),
          };
        });
        // Explicitly map the current cursor with forward bias (shift cursor forward with text,
        // including when text is insert at the current cursor)
        const length = view.state.doc.length;
        const changeSet = ChangeSet.of(changeSpec, length);
        const changeDesc = changeSet.desc;
        const currentSelection = view.state.selection.main;
        // 1 bias associates the cursor with the next character, giving forward bias
        // https://codemirror.net/docs/ref/#state.SelectionRange.assoc
        const newSelection = currentSelection.map(changeDesc, 1);
        const annotation = remoteAnnotationType.of(true);
        const transactionSpec: TransactionSpec = {
          changes: changeSpec,
          selection: newSelection,
          annotations: [annotation],
        };
        console.debug("Dispatching transaction spec: ", transactionSpec);
        view.dispatch(transactionSpec);
      }
    },
    [remoteAnnotationType, view]
  );

  const updateCollabSelections = useCallback((client: Client) => {
    const cursorPositions: UserCursorPos[] = client.cursor_pos_vec();
    const newCollabSelections: UserSelectionRange[] = cursorPositions.map(
      (userCursorPos) => {
        const cursorPos = userCursorPos.cursor_pos();
        const selectionRange = {
          from: cursorPos.from(),
          to: cursorPos.to(),
          anchor: cursorPos.anchor(),
          head: cursorPos.head(),
        };

        return {
          userId: userCursorPos.user_id(),
          selection: selectionRange,
        };
      }
    );
    setCollabSelections(newCollabSelections);
  }, []);

  // Updates editor, code output, and cargo command configuration state after
  // a snapshot message
  const handleSnapshot = useCallback(
    (snapshot: Snapshot) => {
      console.debug("Handling snapshot: ", snapshot);

      dispatch(setOptLevel(snapshot.runConfig.optLevel));
      dispatch(setChannel(snapshot.runConfig.channel));
      dispatch(setCargoCommand(snapshot.runConfig.cargoCommand));

      // Set previous execution state
      const runnerOutput = snapshot.codeOutputState?.runnerOutput ?? null;
      if (runnerOutput !== null) {
        // Prior cargo execution exists, show the cargo output
        setShowCargoOutput(true);
        // But do not open the `Output` panel to reduce visual clutter on join
        setCargoOutputOpen(false);
        setRunOutput(runnerOutput);
      }

      const containerMsg = snapshot.codeOutputState?.containerMsg ?? null;
      if (containerMsg !== null) {
        // Update the cargo command configuration
        const containerMessageType = Object.keys(
          containerMsg
        )[0] as ContainerMessageType;
        // If the container message is present, it should have one key
        if (containerMessageType === undefined) {
          console.error(
            "Container message type is undefined for snapshot: ",
            snapshot
          );
          return;
        }
        // The container message
        const executeCommand = containerMsg[containerMessageType];
        dispatch(setLastExecuteCargoCommand(executeCommand.cargoCommand));
        dispatch(setLastExecuteOptLevel(executeCommand.optLevel));
        dispatch(setLastExecuteChannel(executeCommand.channel));
        dispatch(setLastExecutionCode(executeCommand.code));
      }
    },
    [dispatch]
  );

  // Enqueues a snackbar wiht a custom color palette and close button
  const enqueueCustomSnackbar = useCallback(
    (html: ReactElement) => {
      enqueueSnackbar(html, {
        // Style mirrors the `channel` and `optLevel` colors,
        // secondary button colors
        style: {
          backgroundColor: theme.palette.secondary.main,
          color: "black",
        },
        action: (key) => (
          <IconButton
            aria-label="close"
            color="inherit"
            onClick={() => closeSnackbar(key)}
          >
            <CloseIcon />
          </IconButton>
        ),
      });
    },
    [theme]
  );

  // Conditionally enqueues a snackbar if the provided `username` is not the current user
  const filteredEnqueueSnackbar = useCallback(
    (html: ReactElement, username: string) => {
      if (currUser.username !== username) {
        enqueueCustomSnackbar(html);
      }
    },
    [enqueueCustomSnackbar, currUser]
  );

  useEffect(() => {
    // Requires a CodeMirror view for the transaction dispatch to target
    if (view) {
      // Create WebSocket connection.
      console.debug(
        "Trying to connect to WS with session ID: ",
        params.sessionId,
        " and user ID: ",
        client.user_id()
      );
      const wsUri = `${process.env.REACT_APP_WEBSOCKET_URI}/websocket/${
        params.sessionId
      }/${client.user_id()}`;
      console.debug("Setting ws as ", wsUri);
      const newSocket = new WebSocket(wsUri);

      // Signal used to remove event listeners on component unmount
      const controller = new AbortController();
      const signal = controller.signal;

      // Connection opened
      newSocket.addEventListener(
        "open",
        function (event) {
          console.debug("Connected to WS Server");
          setWsOpen(true);
        },
        { signal }
      );

      // Listen for messages
      newSocket.addEventListener(
        "message",
        function (event) {
          const serverMessage: ServerMessage = event.data;
          const serverMessageObj = JSON.parse(serverMessage);
          // Corresponds to rust `ServerMessage` enum variant
          const serverMsgType: ServerMessageType = Object.keys(
            serverMessageObj
          )[0] as ServerMessageType;
          console.debug("Server message: ", serverMessageObj);
          console.debug("Server message type: ", serverMsgType);
          switch (serverMsgType) {
            case "RemoteUpdate":
            case "Snapshot":
            case "UserList":
              // Decode the collaborative code update in the ws message and set it to `code`
              try {
                const clientResponse: ClientResponse | undefined =
                  clientRef.current.handle_server_message(serverMessage);
                console.debug("Received client response: ", clientResponse);

                // Always update code container. This should not change the code container if the update is an ack to a local operation.
                updateCollabSelections(clientRef.current);
                setCodeContainerText({ code: clientRef.current.document() });

                if (serverMsgType === ServerMessageType.Snapshot) {
                  handleSnapshot(serverMessageObj[serverMsgType] as Snapshot);
                }

                if (clientResponse) {
                  const updateType: ClientResponseType =
                    clientResponse.message_type();
                  // Send the next buffered client operation to the server if applicable
                  // (occurs when `serverMessage` is an ack to an outstanding client operation and client has another operation buffered)
                  if (
                    updateType === ClientResponseType.BroadcastLocalDocUpdate
                  ) {
                    console.debug("Sending buffered client operation");
                    // The doc update will be a string because the tag `updateType` is `BroadcastLocalDocUpdate`
                    const docUpdate =
                      clientResponse.get_broadcast_doc_update() as string;
                    const docUpdateWrapper: DocUpdateWrapper = {
                      inner: {
                        docUpdate: docUpdate,
                      },
                      opState: OpState.Unsent,
                    };
                    console.assert(
                      clientRef.current.document() ===
                        codeContainerTextRef.current.code,
                      "Client document should match code container if the update is acking a local operation\n",
                      "client doc - ",
                      clientRef.current.document(),
                      "container code - ",
                      codeContainerTextRef.current.code
                    );
                    const docUpdateRust = docUpdateToRust(docUpdateWrapper);
                    wsSendRef.current(docUpdateRust);
                  } else if (updateType === ClientResponseType.UserList) {
                    console.debug("User list update");
                    const userList = clientResponse.get_user_list() as UserList;

                    setUserArr(userList.users());
                    console.debug("UserList: " + userList.to_string());
                  } else if (
                    updateType === ClientResponseType.RemoteDocUpdate
                  ) {
                    console.debug("Applying an update to the local document");
                    const localDocUpdate: RemoteDocUpdate =
                      clientResponse.get_remote_doc_update() as RemoteDocUpdate;
                    dispatchTransaction(localDocUpdate.text_updates());
                  } else {
                    console.error(
                      "Unknown client response type when : ",
                      updateType
                    );
                  }
                }

                console.debug(
                  "Post server ws message - Bridge length (should converge to 0): ",
                  clientRef.current.buffer_len()
                );
              } catch (error) {
                if (error instanceof SyntaxError) {
                  console.error("JSON Syntax Error:", error);
                } else {
                  console.error("Error parsing JSON:", error);
                }
              }
              break;
            case "Run":
              const runOutput = serverMessageObj[serverMsgType] as RunOutput;
              console.debug("Received run output: ", runOutput);
              setRunOutput(runOutput);
              break;
            case "RunStatus":
              const serverRunStatus = serverMessageObj[
                serverMsgType
              ] as ServerRunStatus;
              console.debug(
                "Received server run status message: ",
                serverRunStatus
              );
              setRunStatus((prev) =>
                updateRunStatus(
                  serverRunStatus.runType,
                  serverRunStatus.runStateUpdate,
                  prev
                )
              );
              // A `RunStatus` message (particularly `RunStateUpdate` = `RunStarted`) indicates the start of a run,
              // so cargo output should be shown. A `RunStatus` message will precede any `Run` message, so toggling here
              // is sufficient and preferrable over toggling in the `Run` message handler.
              setShowCargoOutput(true);
              break;
            case "RunConfigAction":
              const runConfigAction: RunConfigAction =
                serverMessageObj[serverMsgType];
              const runConfigType = runConfigAction.type;
              const newChannel = runConfigAction.channel;
              const newOptLevel = runConfigAction.optLevel;
              const newCargoCommand = runConfigAction.cargoCommand;

              console.debug("Received run config message: ", runConfigAction);
              if (runConfigType === RunConfigType.RecentExecution) {
                const lastExecutionCode = runConfigAction.code;
                const executingUser = runConfigAction.username;
                dispatch(setLastExecutionCode(lastExecutionCode));
                dispatch(setExecutingUser(executingUser));
                // Enqueue an info snackbar
                console.debug("Enqueueing snackbar");
                filteredEnqueueSnackbar(
                  <span>
                    User <strong>{executingUser}</strong> ran:{" "}
                    <code>cargo {newCargoCommand.toLowerCase()}</code>
                  </span>,
                  executingUser
                );
              } else if (runConfigType === RunConfigType.ConfigUpdate) {
                const updatingUser = runConfigAction.username;
                if (channelRef.current !== newChannel) {
                  filteredEnqueueSnackbar(
                    <span>
                      User <strong>{updatingUser}</strong> updated the channel
                      from <strong>{channelRef.current}</strong> to{" "}
                      <strong>{newChannel}</strong>
                    </span>,
                    updatingUser
                  );
                }
                if (optLevelRef.current !== newOptLevel) {
                  filteredEnqueueSnackbar(
                    <span>
                      User <strong>{updatingUser}</strong> updated the
                      optimization level from{" "}
                      <strong>{optLevelRef.current}</strong> to{" "}
                      <strong>{newOptLevel}</strong>
                    </span>,
                    updatingUser
                  );
                }
                if (cargoCommandRef.current !== newCargoCommand) {
                  filteredEnqueueSnackbar(
                    <span>
                      User <strong>{updatingUser}</strong> updated the cargo
                      command from <strong>{cargoCommandRef.current}</strong> to{" "}
                      <strong>{newCargoCommand}</strong>
                    </span>,
                    updatingUser
                  );
                }
              }

              dispatch(setChannel(newChannel));
              dispatch(setOptLevel(newOptLevel));
              dispatch(setCargoCommand(newCargoCommand));

              console.debug(
                "Received run config message: ",
                serverMessageObj[serverMsgType]
              );

              break;
            default:
              console.error("Unknown server message type: ", serverMsgType);
          }
        },
        { signal }
      );

      // Connection closed
      newSocket.addEventListener(
        "close",
        function (event) {
          console.error("Disconnected from WS Server");
          setWsOpen(false);
        },
        { signal }
      );

      // WS error
      newSocket.addEventListener(
        "error",
        (error) => {
          // A ws error implies disconnection
          console.error("WebSocket error: ", error);
          setWsOpen(false);
        },
        { signal }
      );

      ws.current = newSocket;

      // Clean up on unmount
      return () => {
        console.debug("Closing + cleaning up WS connection and event handlers");
        newSocket?.close();
        controller.abort();
      };
    }
  }, [
    client,
    params.sessionId,
    dispatchTransaction,
    view,
    updateCollabSelections,
    dispatch,
    handleSnapshot,
    enqueueCustomSnackbar,
    filteredEnqueueSnackbar,
  ]);

  // Sends a stringified object to the server
  const wsSend = useCallback((wsMessage: WsClientTextMsg) => {
    const stringifiedMsg = JSON.stringify(wsMessage);
    console.debug("Try sending ws message", stringifiedMsg);
    if (ws.current && ws.current.readyState === WebSocket.OPEN) {
      ws.current.send(stringifiedMsg);
    } else {
      console.error("WS not open to send message: ", stringifiedMsg);
    }
  }, []);
  const wsSendRef = useRef(wsSend);

  // Update Refs, does not trigger rerenders
  // Refs are used to avoid mounting and unmounting the websocket connection
  useEffect(() => {
    cargoOutputRef.current = runOutput;
  }, [runOutput]);

  useEffect(() => {
    codeContainerTextRef.current = codeContainerText;
  }, [codeContainerText]);

  useEffect(() => {
    wsSendRef.current = wsSend;
  }, [wsSend]);

  useEffect(() => {
    console.debug("Setting client ref");
    clientRef.current = client;
  }, [client]);

  useEffect(() => {
    const isBinary = HAS_MAIN_FUNCTION_RE.test(codeContainerText.code);
    setTargetType(isBinary ? TargetType.Binary : TargetType.Library);
  }, [codeContainerText.code]);

  useEffect(() => {
    cargoCommandRef.current = cargoCommand;
  }, [cargoCommand]);

  useEffect(() => {
    optLevelRef.current = optLevel;
  }, [optLevel]);

  useEffect(() => {
    channelRef.current = channel;
  }, [channel]);

  // Currently makes simple assumption that binary crates are always run
  // and library crates are built
  const executeCode = useCallback(async () => {
    const executeCommand = {
      code: codeContainerText.code,
      targetType: targetType,
      cargoCommand: cargoCommand,
      optLevel: optLevel,
      channel: channel,
    };

    const executeCommandObj = executeCommandToObj(executeCommand);
    console.debug("Sending execute command over ws: ", executeCommand);
    wsSend(executeCommandObj);
  }, [
    codeContainerText.code,
    wsSend,
    targetType,
    cargoCommand,
    optLevel,
    channel,
  ]);

  const handleEditorChange = useCallback(
    (viewUpdate: ViewUpdate) => {
      console.log("View update: ", viewUpdate);
      // Handle cursor updates and doc updates
      if (viewUpdate.selectionSet || viewUpdate.docChanged) {
        // Log transactions for view update
        const textUpdates: TextUpdate[] = [];
        viewUpdate.changes.iterChanges((fromA, toA, fromB, toB, text) => {
          const prev: TextUpdateRange = TextUpdateRange.new(fromA, toA);
          const next: TextUpdateRange = TextUpdateRange.new(fromB, toB);
          const textUpdate = TextUpdate.new(prev, next, text.toString());
          textUpdates.push(textUpdate);
        });

        const selection = viewUpdate.state.selection.main;
        const cursorPos: CursorPos = CursorPos.new(
          selection.from,
          selection.to,
          selection.head,
          selection.anchor
        );

        const editorText = viewUpdate.state.doc.toString();
        console.debug(
          "Cursor position: ",
          cursorPos.to_string(),
          ", Client cursor position: ",
          client.cursor_pos()?.to_string()
        );
        console.debug(
          "Cursors equal: ",
          client.cursor_pos()?.equals(cursorPos)
        );
        console.debug("Editor text: " + editorText);
        console.debug("Client doc: " + client.document());
        console.debug("Documents equal: ", client.document() === editorText);

        // Important: Check if the change is local or remote.
        const isRemoteUpdate = (viewUpdate: ViewUpdate): boolean => {
          const isRemoteUpdate = viewUpdate.transactions.some((t) =>
            t.annotation(remoteAnnotationType)
          );
          if (isRemoteUpdate) {
            console.assert(
              viewUpdate.transactions.every((t) =>
                t.annotation(remoteAnnotationType)
              ),
              "If one transaction is remote, all transactions should be remote updates"
            );
          }
          return isRemoteUpdate;
        };

        // `handleEditorChange` only operates on local updates. The ws message handler
        // handles remote updates.
        if (isRemoteUpdate(viewUpdate)) {
          console.debug(
            "Server sync triggered editor change. Not a local update."
          );
          return;
        }
        const prevDocLen = viewUpdate.changes.desc.length;
        let docUpdateStringified = "";
        try {
          docUpdateStringified = client.update_document_wasm(
            editorText,
            prevDocLen,
            textUpdates,
            cursorPos
          );
        } catch (error: unknown) {
          console.error(
            "Disconnecting from websocket. Received client update document error: " +
              error
          );
          setWsDisconnectMsg(
            (msg) =>
              String(error) +
              " Disconnected from server. Please refresh the page to rejoin."
          );
          ws.current?.close();
          return;
        }
        const clientDoc = client.document();
        console.assert(
          clientDoc === editorText,
          "Client doc should match user input"
        );
        const shouldSendUpdate = client.prepare_send_local_update();
        console.debug("Should send update: ", shouldSendUpdate);
        console.debug(
          "Bridge length (should converge to 0): ",
          client.buffer_len()
        );
        // `shouldSendUpdate` is true if this is the only update in the Client buffer
        if (shouldSendUpdate) {
          console.assert(
            client.buffer_len() === 1,
            "If `shouldSendUpdate then buffer length should be 1"
          );

          const docUpdate: DocUpdateWrapper = {
            inner: {
              docUpdate: docUpdateStringified,
            },
            opState: OpState.Unsent,
          };
          console.debug("Sending doc update: ", docUpdate.inner.docUpdate);
          console.assert(
            docUpdate.opState === OpState.Unsent,
            "The doc update sent should not have been previously sent"
          );
          const docUpdateRust = docUpdateToRust(docUpdate);
          wsSend(docUpdateRust);
        }

        // Local update updates cursor positions and code container
        updateCollabSelections(clientRef.current);
        setCodeContainerText({ code: editorText });
      }
    },
    [wsSend, updateCollabSelections, client, remoteAnnotationType]
  );

  return (
    <Box className="App">
      <HeaderBar
        RunButton={RunButton({
          runStatus,
          setShowCargoOutput,
          setCargoOutputOpen,
          executeCode,
          wsSendRef,
        })}
        RunConfigButtons={RunConfigButtons({
          stableVersion,
          betaVersion,
          nightlyVersion,
          wsSendRef,
        })}
        userArr={userArr}
        currUser={currUser}
      />
      <EditorContainer
        setView={setView}
        handleEditorChange={handleEditorChange}
        userArr={userArr}
        collabSelections={collabSelections}
        showCargoOutput={showCargoOutput}
        cargoOutputOpen={cargoOutputOpen}
        setCargoOutputOpen={setCargoOutputOpen}
        runOutput={runOutput}
        runStatus={runStatus}
      />
      <Snackbar
        open={!wsOpen}
        TransitionComponent={Grow}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert severity="error">{wsDisconnectMsg}</Alert>
      </Snackbar>
      <SnackbarProvider
        maxSnack={3}
        autoHideDuration={5000}
        anchorOrigin={{
          vertical: "bottom",
          horizontal: "right",
        }}
      />
    </Box>
  );
}

export default App;
export { WsClientTextMsgType };
export type {
  UserSelectionRange,
  SelectionRange,
  SelectionFocused,
  SelectionUnfocused,
  WsConfigUpdate,
  WsClientTextMsg,
};
