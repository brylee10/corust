import React, { useCallback, useEffect, useRef, useState } from "react";
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
import { Alert, Snackbar, Grow } from "@mui/material";
import {
  RunOutputDisplay,
  RunOutput,
  RunStatus,
  updateRunStatus,
  ServerRunStatus,
} from "./components/runOutputDisplay.tsx";
import HeaderBar from "./components/headerBar/headerBar.tsx";
import RunButton from "./components/headerBar/runButton.tsx";
import RunConfigButtons from "./components/headerBar/runConfigButtons.tsx";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";
import Editor from "./components/editor/editor.tsx";

// Interfaces/Type definitions

// Ws message variants
type WsClientTextMsg = RustDocUpdate | RustExecuteCommand;

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
}

enum TargetType {
  Library = "Library",
  Binary = "Binary",
}

enum CargoCommand {
  Build = "Build",
  Run = "Run",
  Test = "Test",
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
  userId: bigint;
}

interface RustDocUpdate {
  type: string;
  docUpdate: string;
}

interface RustExecuteCommand {
  type: string;
  code: string;
  targetType: TargetType;
  cargoCommand: CargoCommand;
}

type ServerMessageType =
  | "RemoteUpdate"
  | "Run"
  | "RunStatus"
  | "Snapshot"
  | "UserList";

// Utilities
const docUpdateToRust = (msg: DocUpdateWrapper): RustDocUpdate => {
  return {
    type: "wsDocUpdate",
    docUpdate: msg.inner.docUpdate,
  };
};

const executeCommandToObj = (msg: ExecuteCommand): RustExecuteCommand => {
  return {
    type: "wsExecuteCommand",
    code: msg.code,
    targetType: msg.targetType,
    cargoCommand: msg.cargoCommand,
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

function App({ userId }: AppProps) {
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
  const [cargoCommand, setCargoCommand] = useState<CargoCommand>(
    CargoCommand.Build
  );
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
    Client.new(userId, maxUpdatesPerMinute, maxDocSizePerMinute)
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
          const type: ServerMessageType = Object.keys(
            serverMessageObj
          )[0] as ServerMessageType;
          console.debug("Server message: ", serverMessageObj);
          switch (type) {
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
              const runOutput = serverMessageObj[type] as RunOutput;
              console.debug("Received run output: ", runOutput);
              setRunOutput(runOutput);
              break;
            case "RunStatus":
              const serverRunStatus = serverMessageObj[type] as ServerRunStatus;
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
            default:
              console.error("Unknown server message type: ", type);
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
  ]);

  // Sends a stringified object to the server
  const wsSend = useCallback((wsMessage: WsClientTextMsg) => {
    console.debug("Try sending ws message", ws.current, ws.current?.readyState);
    const stringifiedMsg = JSON.stringify(wsMessage);
    if (ws.current && ws.current.readyState === WebSocket.OPEN) {
      ws.current.send(stringifiedMsg);
    } else {
      console.error("WS not open to send message: ", stringifiedMsg);
    }
  }, []);
  const wsSendRef = useRef(wsSend);

  // Update Refs, does not trigger rerenders
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
    clientRef.current = client;
  }, [client]);

  useEffect(() => {
    const isBinary = HAS_MAIN_FUNCTION_RE.test(codeContainerText.code);
    setTargetType(isBinary ? TargetType.Binary : TargetType.Library);
  }, [codeContainerText.code]);

  // Currently makes simple assumption that binary crates are always run
  // and library crates are built
  const executeCode = useCallback(async () => {
    const executeCommand = {
      code: codeContainerText.code,
      targetType: targetType,
      cargoCommand: cargoCommand,
    };

    const executeCommandObj = executeCommandToObj(executeCommand);
    console.debug("Sending execute command over ws: ", executeCommand);
    wsSend(executeCommandObj);
  }, [codeContainerText.code, wsSend, targetType, cargoCommand]);

  const handleEditorChange = useCallback(
    (viewUpdate: ViewUpdate) => {
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

  const renderCargoOutput = useCallback(() => {
    if (showCargoOutput) {
      if (cargoOutputOpen) {
        const cargoOutput = (
          <>
            <PanelResizeHandle className="resize-handle-vert" />
            <Panel
              className="max-height-vert"
              collapsible={true}
              minSize={10}
              onCollapse={() => setCargoOutputOpen(false)}
              id={"2"}
            >
              <RunOutputDisplay
                runOutput={runOutput}
                runStatus={runStatus}
                open={cargoOutputOpen}
                setOpen={setCargoOutputOpen}
              />
            </Panel>
          </>
        );
        return cargoOutput;
      } else {
        // If output is closed, resize handle not needed
        const cargoOutput = (
          <RunOutputDisplay
            runOutput={runOutput}
            runStatus={runStatus}
            open={cargoOutputOpen}
            setOpen={setCargoOutputOpen}
          />
        );
        return cargoOutput;
      }
    } else {
      return null;
    }
  }, [
    setCargoOutputOpen,
    showCargoOutput,
    cargoOutputOpen,
    runOutput,
    runStatus,
  ]);

  return (
    <div className="App">
      <HeaderBar
        RunButton={RunButton({
          runStatus,
          setShowCargoOutput,
          executeCode,
          cargoCommand,
          setCargoCommand,
        })}
        RunConfigButtons={RunConfigButtons({
          stableVersion,
          betaVersion,
          nightlyVersion,
        })}
        userArr={userArr}
        selfUserId={client.user_id()}
      />
      <PanelGroup direction="horizontal" className="max-height">
        <Panel className="max-height" minSize={10} id={"1"}>
          <Editor
            setView={setView}
            handleEditorChange={handleEditorChange}
            userArr={userArr}
            collabSelections={collabSelections}
          />
        </Panel>
        {renderCargoOutput()}
      </PanelGroup>
      <Snackbar
        open={!wsOpen}
        TransitionComponent={Grow}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert severity="error">{wsDisconnectMsg}</Alert>
      </Snackbar>
    </div>
  );
}

export default App;
export { CargoCommand };
export type {
  UserSelectionRange,
  SelectionRange,
  SelectionFocused,
  SelectionUnfocused,
};
