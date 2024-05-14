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
  ReactCodeMirrorRef,
  ChangeSpec,
  TransactionSpec,
  Annotation,
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
import { Alert, Button, Fade, styled } from "@mui/material";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";

// Define constants once
const zeroPosition = wasm.CursorPos.new(0, 0, 0, 0);

const CustomButton = styled(Button)({
  padding: "10px 20px",
  backgroundColor: "green",
  color: "white",
  border: "none",
  borderRadius: "5px",
  cursor: "pointer",
  alignSelf: "flex-start",
  marginTop: "10px",
  marginLeft: "10px",
  marginRight: "10px",
  fontWeight: "bold",
  lineHeight: "1.25",
  "&:hover": {
    backgroundColor: "green",
  },
});

// Interfaces

// Document update, with additional metadata
interface DocUpdateWrapper {
  inner: BroadcastLocalDocUpdate;
  // Whether update is unsent, pending, or acked
  opState: OpState;
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

// Output of the compilation + runner
interface RunnerOutput {
  stdout: string;
  stderr: string;
  status: number;
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
}

interface AppProps {
  userId: bigint;
}

function App({ userId }: AppProps) {
  // Route params
  const params = useParams();
  // CargoOutput has schema: Object {stdout: string, stderr: string, status: number}
  const [cargoOutput, setCargoOutput] = useState<RunnerOutput | null>(null);
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
  // Event listeners capture static state, so we need to use refs for indirection to the latest state
  const cargoOutputRef = useRef(cargoOutput);
  const codeContainerTextRef = useRef(codeContainerText);
  const clientRef = useRef(client);
  const ws = useRef<WebSocket | null>(null);
  const codeMirrorRef = useRef<ReactCodeMirrorRef | null>(null);

  // Rust `ServerMessage` is serialized as a string
  type ServerMessage = string;

  const dispatchTransaction = useCallback(
    (textUpdates: TextUpdate[]) => {
      // Convert text updates into a transaction spec to update the editor
      const view = codeMirrorRef.current?.view;
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
    [codeMirrorRef, remoteAnnotationType]
  );

  useEffect(() => {
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

    const controller = new AbortController();
    const signal = controller.signal;

    // Connection opened
    newSocket.addEventListener(
      "open",
      function (event) {
        console.log("Connected to WS Server");
        setWsOpen(true);
      },
      { signal }
    );

    // Listen for messages
    newSocket.addEventListener(
      "message",
      function (event) {
        // Decode the collaborative code update in the ws message and set it to `code`
        try {
          const serverMessage: ServerMessage = event.data;
          const clientResponse: ClientResponse | undefined =
            clientRef.current.handle_server_message(serverMessage);
          console.log("Received client response: ", clientResponse);
          if (clientResponse) {
            const updateType: ClientResponseType =
              clientResponse.message_type();
            // Send the next buffered client operation to the server if applicable
            // (occurs when `serverMessage` is an ack to an outstanding client operation and client has another operation buffered)
            if (updateType === ClientResponseType.BroadcastLocalDocUpdate) {
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
              wsSendRef.current(docUpdateWrapper);
            } else if (updateType === ClientResponseType.UserList) {
              console.debug("User list update");
              const userList = clientResponse.get_user_list() as UserList;

              setUserArr(userList.users());
              console.debug("UserList: " + userList.to_string());
            } else if (updateType === ClientResponseType.RemoteDocUpdate) {
              console.debug("Local doc update");
              const localDocUpdate: RemoteDocUpdate =
                clientResponse.get_remote_doc_update() as RemoteDocUpdate;
              dispatchTransaction(localDocUpdate.text_updates());
            } else {
              console.error("Unknown client response type when : ", updateType);
            }
          }

          console.debug(
            "Post server ws message - Bridge length (should converge to 0): ",
            clientRef.current.buffer_len()
          );
          const cursorPositions: UserCursorPos[] =
            clientRef.current.cursor_pos_vec();
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
          // Always update code container. This should not change the code container if the update is an ack to a local operation.
          setCollabSelections(newCollabSelections);
          setCodeContainerText({ code: clientRef.current.document() });
        } catch (error) {
          if (error instanceof SyntaxError) {
            console.error("JSON Syntax Error:", error);
          } else {
            console.error("Error parsing JSON:", error);
          }
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
  }, [client, params.sessionId, dispatchTransaction]);

  const wsSend = useCallback((docUpdateWrapper: DocUpdateWrapper) => {
    console.debug("Try sending ws message", ws.current, ws.current?.readyState);
    if (ws.current && ws.current.readyState === WebSocket.OPEN) {
      console.debug("Sending doc update: ", docUpdateWrapper.inner.docUpdate);
      console.assert(
        docUpdateWrapper.opState === OpState.Unsent,
        "The doc update sent should not have been previously sent"
      );
      ws.current.send(docUpdateWrapper.inner.docUpdate);
    } else {
      console.error("WS not open to send message: ", docUpdateWrapper);
    }
  }, []);
  const wsSendRef = useRef(wsSend);

  // Update Refs
  useEffect(() => {
    cargoOutputRef.current = cargoOutput;
  }, [cargoOutput]);

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
    const headers = new Headers();
    headers.append("Content-Type", "application/json");

    // Will only throw an error if network error encountered
    try {
      const response = await fetch("http://127.0.0.1:8000/compile", {
        method: "POST",
        headers: headers,
        body: JSON.stringify({ code: codeContainerText.code }),
      });
      const cargoOutput = await response.json();
      setCargoOutput(cargoOutput);
    } catch (error) {
      console.error("Fetch error: ", error);
    }
  }, [codeContainerText.code]);

  const formatCargoOutput = (cargoOutput: RunnerOutput | null) => {
    if (!cargoOutput) {
      return "";
    }
    return `stdout: \n${cargoOutput.stdout}\nstderr: \n${cargoOutput.stderr}\nstatus: \n${cargoOutput.status}`;
  };

  const handleEditorChange = useCallback(
    (viewUpdate: ViewUpdate) => {
      // console.debug("Selection set: ", viewUpdate.selectionSet);
      // console.debug("Doc changed: ", viewUpdate.docChanged);
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
        // // 1. If remote, the server update would have already been applied to the client,
        // //    so document and cursor pos would match
        // // Note: Do not use strict equality (===) for WASM types, otherwise WASM addresses will be compared.
        // // Instead, use `to_string()` or implement `equals()` in Rust
        // const cond1 =
        //   client.document() === editorText &&
        //   client.cursor_pos()?.equals(cursorPos);
        // // 2. If the editor does not have focus, then the update cannot be local. There is an
        // //    odd situation where CodeMirror assumes programatic setting of the editor text
        // //    is an insert and delete of the entire document. This is fine by itself. But CodeMirror
        // //    also assumes when the user does not focus on the editor then the selection is
        // //    (from: 0, to: 0, anchor: 0, head: 0). The Client object does not reset the cursor position
        // //    when focus changes. Without the hasFocus check, the cursor position could easily be different
        // //    between the client and editor when the editor is not focused. This would incorrectly "look like"
        // //    a local update.
        // // Note: Known bug
        // //    When the user first clicks into the editor sometimes the editor view is marked as not focused so the
        // //    cursor is not broadcast. After the user clicks elsewhere or types the focus is obtained. This issue
        // //    is "worked around" by also including a check that the cursor position is 0 to still address the above
        // //    condition 2, so this bug only occurs when the user selects position 0 as the first click.
        // const cond2 =
        //   !viewUpdate.view.hasFocus &&
        //   cursorPos.to_string() === zeroPosition.to_string();
        // const serverSyncTriggered = cond1 || cond2;
        // console.debug(
        //   "Document and cursor pos have not changed (cond1): ",
        //   cond1,
        //   " OR View update does not have focus (cond2): ",
        //   !viewUpdate.view.hasFocus,
        //   " and is at zero position: ",
        //   cursorPos.to_string() + " " + zeroPosition.to_string(),
        //   " Focus changed: ",
        //   viewUpdate.focusChanged
        // );

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
          wsSend(docUpdate);
        }
        setCodeContainerText({ code: editorText });
      }
    },
    [wsSend, client]
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
    (rgb: string) =>
      Decoration.widget({
        widget: new (class extends WidgetType {
          toDOM() {
            const cursor = document.createElement("div");
            cursor.style.display = "inline";
            cursor.style.borderLeft = `2px solid ${rgb}`; // Simulate the cursor line
            cursor.style.pointerEvents = "none"; // Make sure the cursor doesn't interfere with text selection
            cursor.style.marginLeft = "-1px"; // Optional: Adjust alignment if necessary
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
            return {
              userSelectionRange: userSelectionRange,
              rgb: rgb ? rgb : "rgb(0, 0, 0)",
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
          const sel = userSel.userSelectionRange.selection as SelectionFocused;
          return cursorDecoration(rgb).range(sel.anchor);
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
              compileCode();
            }}
            endIcon={<PlayArrowIcon />}
          >
            RUN
          </CustomButton>
        </div>
        <UserIconList userArr={userArr} />
      </div>
      <CodeMirror
        ref={codeMirrorRef}
        className="editor"
        height="100%"
        // value={codeContainerText.code}
        extensions={[rust(), extraCursorsPlugin]}
        onUpdate={handleEditorChange}
      />
      {!wsOpen ? (
        <Alert severity="error">
          Disconnected from server. Please refresh the page to rejoin.
        </Alert>
      ) : null}
      {/* <div className="editor-container">
        <textarea
          className="compile-output editor-right"
          value={formatCargoOutput(cargoOutput)}
          readOnly
          placeholder="Cargo Output"
        />
      </div> */}
    </div>
  );
}

export default App;
