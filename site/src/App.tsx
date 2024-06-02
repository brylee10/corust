import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import "./App.css"; // Ensure to import the CSS file
import CodeMirror, {
  ViewUpdate,
  Decoration,
  ViewPlugin,
  WidgetType,
  EditorView,
  Range,
  DecorationSet,
  ChangeSpec,
  TransactionSpec,
  AnnotationType,
} from "@uiw/react-codemirror";
import { rust } from "@codemirror/lang-rust";
import * as wasm from "corust-components/corust_components.js";
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
import UserIconList from "./components/userIconList";
import { Alert, Button, Snackbar, styled, Grow } from "@mui/material";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import { RunOutputDisplay, RunOutput } from "./components/runOutputDisplay";

// Define constants once
const CustomButton = styled(Button)({
  padding: "10px 20px",
  backgroundColor: "green",
  color: "white",
  border: "none",
  borderRadius: "5px",
  cursor: "pointer",
  alignSelf: "flex-start",
  fontWeight: "bold",
  lineHeight: "1.25",
  "&:hover": {
    backgroundColor: "green",
  },
});

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
  Clippy = "Clippy",
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

interface UserSelectionRangeColor {
  userSelectionRange: UserSelectionRange;
  // `rgb(r, g, b)` string
  rgb: string;
  name: string;
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

type ServerMessageType = "RemoteUpdate" | "Run" | "Snapshot" | "UserList";

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

function App({ userId }: AppProps) {
  // Route params
  const params = useParams();
  // CargoOutput has schema: Object {stdout: string, stderr: string, status: number}
  const [runOutput, setRunOutput] = useState<RunOutput | null>(null);
  const [showCargoOutput, setShowCargoOutput] = useState<boolean>(false);
  const [codeContainerText, setCodeContainerText] = useState<CodeContainerText>(
    {
      code: "",
    }
  );
  const [collabSelections, setCollabSelections] = useState<
    UserSelectionRange[]
  >([]);
  const [userArr, setUserArr] = useState<UserInner[]>([]);
  const [wsOpen, setWsOpen] = useState<boolean>(false);
  const [remoteAnnotationType] = useState<AnnotationType<boolean>>(
    new AnnotationType()
  );
  // The client object should be created once per component render. It cannot be passed in as
  // a prop and modified in place, otherwise the component would be impure.
  const [client] = useState<Client>(Client.new(userId));
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
        const annotation = remoteAnnotationType.of(true);
        const transactionSpec: TransactionSpec = {
          changes: changeSpec,
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
      console.log(
        "Trying to connect to WS with session ID: ",
        params.sessionId,
        " and user ID: ",
        client.user_id()
      );
      const newSocket = new WebSocket(
        `ws://127.0.0.1:8000/websocket/${params.sessionId}/${client.user_id()}`
      );

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
                    console.debug("Local doc update");
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
              console.debug("Received run output");
              const runOutput = serverMessageObj[type] as RunOutput;
              setRunOutput(runOutput);
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
          console.log("Disconnected from WS Server");
          setWsOpen(false);
        },
        { signal }
      );

      // WS error
      newSocket.addEventListener(
        "error",
        (error) => {
          console.error("WebSocket error: ", error);
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

  const compileCode = useCallback(async () => {
    const executeCommand = {
      code: codeContainerText.code,
      targetType: TargetType.Binary,
      cargoCommand: CargoCommand.Run,
    };
    const executeCommandObj = executeCommandToObj(executeCommand);
    console.debug("Sending execute command over ws: ", executeCommand);
    wsSend(executeCommandObj);
  }, [codeContainerText.code, wsSend]);

  const handleEditorChange = useCallback(
    (viewUpdate: ViewUpdate) => {
      // Handle cursor updates and doc updates
      if (viewUpdate.selectionSet || viewUpdate.docChanged) {
        // Log transactions for view update
        const textUpdates: TextUpdate[] = [];
        viewUpdate.changes.iterChanges((fromA, toA, fromB, toB, text) => {
          const prev: TextUpdateRange = TextUpdateRange.new(fromA, toA);
          const next: TextUpdateRange = TextUpdateRange.new(fromB, toB);
          console.debug(
            "Change from ",
            fromA,
            " to ",
            toA,
            " to ",
            fromB,
            " to ",
            toB,
            " with text ",
            text
          );
          const textUpdate = TextUpdate.new(prev, next, text.toString());
          textUpdates.push(textUpdate);
        });

        const selection = viewUpdate.state.selection.main;
        const cursorPos: CursorPos = wasm.CursorPos.new(
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
        const docUpdateStringified = client.update_document_wasm(
          editorText,
          prevDocLen,
          textUpdates,
          cursorPos
        );
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

  const isSelectionFocused = useCallback(
    (sel: SelectionRange): sel is SelectionFocused => {
      return (
        sel.from !== undefined &&
        sel.to !== undefined &&
        sel.anchor !== undefined &&
        sel.head !== undefined
      );
    },
    []
  );

  const cursorDecoration = useCallback(
    (rgb: string, name: string) =>
      Decoration.widget({
        widget: new (class extends WidgetType {
          toDOM() {
            const cursor = document.createElement("div");
            cursor.className = "cursor";
            cursor.style.display = "inline";
            cursor.style.borderLeft = `2px solid ${rgb}`; // Simulate the cursor line
            // cursor.style.pointerEvents = "none"; // Make sure the cursor doesn't interfere with text selection
            cursor.style.marginLeft = "-1px";
            // Does not affect cursor display, but tooltip is positioned relative to this
            cursor.style.position = "relative";

            const tooltip = document.createElement("span");
            tooltip.innerText = `${name}`;
            tooltip.className = "tooltip";
            tooltip.style.visibility = "hidden";
            tooltip.style.backgroundColor = `${rgb}`;
            tooltip.style.color = "whitesmoke";
            tooltip.style.textAlign = "center";
            tooltip.style.borderRadius = "4px";
            tooltip.style.paddingLeft = "3px";
            tooltip.style.paddingRight = "3px";
            tooltip.style.position = "absolute";
            tooltip.style.zIndex = "1";
            tooltip.style.bottom = "30%";
            tooltip.style.fontSize = "small";
            tooltip.style.opacity = "0";
            tooltip.style.transition =
              "visibility 0.2s ease-in-out, opacity 0.2s ease-in-out";

            // Append the tooltip to the cursor element
            cursor.appendChild(tooltip);

            // Add event listeners for hover actions
            cursor.addEventListener("mouseenter", () => {
              console.debug("Hovering the cursor");
              tooltip.style.visibility = "visible";
              tooltip.style.opacity = "1";
            });

            cursor.addEventListener("mouseleave", () => {
              console.debug("Mouseout the cursor");
              tooltip.style.visibility = "hidden";
              tooltip.style.opacity = "0";
            });

            return cursor;
          }
        })(),
      }),
    []
  );

  const textHighlightDecoration = useCallback((rgb: string) => {
    function convertRgbToRgba(rgb: string, alpha = 0.2) {
      return rgb.replace("rgb", "rgba").replace(")", `, ${alpha})`);
    }

    const decoration = Decoration.mark({
      attributes: { style: `background-color: ${convertRgbToRgba(rgb, 0.2)}` },
    });

    return decoration;
  }, []);

  const extraCursorsPlugin = useMemo(() => {
    const computeCursorDecorations = (
      view: EditorView,
      userArr: UserInner[],
      collabSelections: UserSelectionRange[]
    ): Range<Decoration>[] => {
      // Augment `collabSelections` with user colors
      const collabSelectionsColored: UserSelectionRangeColor[] =
        collabSelections
          .filter((userSelectionRange) => {
            // Check if the userId is found in the userArr
            return userArr.some(
              (user) => user.user_id() === userSelectionRange.userId
            );
          })
          .map((userSelectionRange) => {
            // The user will be found given the filter above
            const user = userArr.find(
              (user) => user.user_id() === userSelectionRange.userId
            ) as UserInner;
            const rgb = user.color();
            const name = user.username();
            return {
              userSelectionRange: userSelectionRange,
              rgb: rgb,
              name: name,
            };
          });

      // Only display selections in the current viewport
      // See https://codemirror.net/docs/guide/ on `Viewport`
      // Truncate any highlight ranges that are outside the viewport
      const focusedSelections: UserSelectionRangeColor[] =
        collabSelectionsColored
          .filter((r) => isSelectionFocused(r.userSelectionRange.selection))
          .filter((r) => {
            // Selection will be focused given `isSelectionFocused` filter
            const sel = r.userSelectionRange.selection as SelectionFocused;
            return (
              view.viewport.from <= sel.anchor && sel.anchor <= view.viewport.to
            );
          });

      // Use a Set to ensure uniqueness of anchor positions
      const seenAnchors = new Set();
      const userSelectionWithUniqueAnchor: UserSelectionRangeColor[] = [];
      focusedSelections.forEach((r) => {
        const selection = r.userSelectionRange.selection as SelectionFocused;
        if (!seenAnchors.has(selection.anchor)) {
          seenAnchors.add(selection.anchor);
          userSelectionWithUniqueAnchor.push(r);
        }
      });

      // Map each unique anchor to a cursor decoration range
      const uniqueAnchors = Array.from(userSelectionWithUniqueAnchor).map(
        (userSel) => {
          const rgb = userSel.rgb;
          const name = userSel.name;
          const sel = userSel.userSelectionRange.selection as SelectionFocused;
          return cursorDecoration(rgb, name).range(sel.anchor);
        }
      );

      // Map each selection to a highlight range
      const highlightRanges = collabSelectionsColored
        .filter((r) => isSelectionFocused(r.userSelectionRange.selection))
        // * truncate highlight range if it extends outside the viewport
        //    Condition 1: sel from, view from -> round up to view from
        //    Condition 2: view to, sel to -> round down to view to
        // * highlights cannot be zero size
        .filter((r) => {
          // Selection will be focused given `isSelectionFocused` filter
          const sel = r.userSelectionRange.selection as SelectionFocused;
          return (
            Math.max(sel.from, view.viewport.from) !==
            Math.min(sel.to, view.viewport.to)
          );
        })
        // Valid relative positions of `selection` and `view` from/to
        // Condition 1: sel from, view from, sel to, view to
        // Condition 2: view from, sel from, view to, sel to
        .filter((r) => {
          const sel = r.userSelectionRange.selection as SelectionFocused;
          return (
            (view.viewport.from <= sel.from && sel.from <= view.viewport.to) ||
            (view.viewport.from <= sel.to && sel.to <= view.viewport.to)
          );
        })
        .map((r) => {
          const sel = r.userSelectionRange.selection as SelectionFocused;
          const rgb = r.rgb;
          return textHighlightDecoration(rgb).range(
            Math.max(sel.from, view.viewport.from),
            Math.min(sel.to, view.viewport.to)
          );
        });

      const combinedRanges = [...uniqueAnchors, ...highlightRanges];
      combinedRanges.sort((a, b) => a.from - b.from);
      return combinedRanges;
    };

    // console.debug("Collab selections: ", collabSelections);
    console.debug("Users array: ", userArr);

    return ViewPlugin.fromClass(
      class {
        decorations: DecorationSet;

        constructor(view: EditorView) {
          const combinedRanges = computeCursorDecorations(
            view,
            userArr,
            collabSelections
          );
          this.decorations = Decoration.set(combinedRanges);
        }

        // Update the decorations before the view updates.
        // Needed for deletes, otherwise highlights from the constructor
        // will be out of range in the new viewport before the client updates
        // the `collabSelections` variable. In general, the client setting `collabSelections`
        // does not happen before the view updates. This means the view updates and a stale
        // set of `collabSelections` is present until the client quickly updates the `collabSelections`.
        // This is impercetible to the user but would otherwise cause out of bounds in the editor.
        update(view: ViewUpdate) {
          const editorView = view.view;
          const combinedRanges = computeCursorDecorations(
            editorView,
            userArr,
            collabSelections
          );
          this.decorations = Decoration.set(combinedRanges);
        }
      },
      {
        decorations: (v) => v.decorations,
      }
    );
  }, [
    collabSelections,
    userArr,
    isSelectionFocused,
    cursorDecoration,
    textHighlightDecoration,
  ]);

  return (
    <div className="App">
      <div className="header-bar">
        <div>
          <CustomButton
            variant="contained"
            size="small"
            onClick={() => {
              setShowCargoOutput(true);
              compileCode();
            }}
            endIcon={<PlayArrowIcon />}
          >
            RUN
          </CustomButton>
        </div>
        <UserIconList userArr={userArr} selfUserId={client.user_id()} />
      </div>
      <CodeMirror
        className="editor"
        height="100%"
        extensions={[rust(), extraCursorsPlugin]}
        onUpdate={handleEditorChange}
        onCreateEditor={(view, state) => {
          setView(view);
        }}
      />
      {showCargoOutput ? <RunOutputDisplay runOutput={runOutput} /> : null}
      <Snackbar
        open={!wsOpen}
        TransitionComponent={Grow}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert severity="error">
          Disconnected from server. Please refresh the page to rejoin.
        </Alert>
      </Snackbar>
    </div>
  );
}

export default App;
