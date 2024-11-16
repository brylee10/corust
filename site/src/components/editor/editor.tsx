import React, { useCallback, useMemo, useState } from "react";
import "../../App.css"; // Ensure to import the CSS file
import CodeMirror, {
  ViewUpdate,
  Decoration,
  ViewPlugin,
  WidgetType,
  EditorView,
  Range,
  DecorationSet,
  BasicSetupOptions,
} from "@uiw/react-codemirror";
import { rust } from "@codemirror/lang-rust";
import { UserInner } from "corust-components/corust_components.js";
import {
  SelectionFocused,
  SelectionRange,
  UserSelectionRange,
} from "../../App.tsx";
import {
  Alert,
  Button,
  Grow,
  Snackbar,
  Stack,
  styled,
  Tooltip,
  useTheme,
} from "@mui/material";
import { useDispatch, useSelector } from "react-redux";
import { RootState } from "../../store/store.tsx";
import {
  SelectedCodeType,
  setLive,
  setLastExecution,
} from "../../store/slices/codeSelectorSlice.tsx";

interface CodeSelectorProps {
  selected: boolean;
}

const CodeSelector = styled(Button)<CodeSelectorProps>(
  ({ theme, selected }) => {
    const backgroundColor = selected ? "#C96556" : "#DCDCDC";
    // Custom darker hover color
    const hoverBackgroundColor = selected ? "#A15145" : "#A7A7A7";
    const fontWeight = selected ? "bold" : "normal";
    const color = selected ? "white" : "black";
    return {
      backgroundColor,
      color,
      border: "none",
      cursor: "pointer",
      fontWeight,
      height: 20,
      alignSelf: "center",
      "&:hover": {
        backgroundColor: hoverBackgroundColor,
      },
    };
  }
);

interface UserSelectionRangeColor {
  userSelectionRange: UserSelectionRange;
  // `rgb(r, g, b)` string
  rgb: string;
  name: string;
}

interface EditorProps {
  setView: (view: EditorView) => void;
  handleEditorChange: (viewUpdate: ViewUpdate) => void;
  userArr: UserInner[];
  collabSelections: UserSelectionRange[];
}

function Editor({
  setView,
  handleEditorChange,
  userArr,
  collabSelections,
}: EditorProps) {
  const theme = useTheme();
  const dispatch = useDispatch();
  const codeTypeSelector = useSelector(
    (state: RootState) => state.codeSelector.type
  );
  const prevExecutedSelector = useSelector(
    (state: RootState) => state.codeSelector.executionCodePrevSet
  );
  const lastExecutedCode = useSelector(
    (state: RootState) => state.codeSelector.lastExecutionCode
  );
  const [openPrevCodeWarning, setOpenPrevCodeWarning] = useState(false);

  const liveEditorOptions: BasicSetupOptions = { tabSize: 4 };
  const readOnlyEditorOptions: BasicSetupOptions = {
    tabSize: 4,
    highlightActiveLine: false,
    highlightActiveLineGutter: false,
  };

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

  // Customize CodeMirror color scheme to rust
  const rustTheme = useMemo(
    () =>
      EditorView.theme({
        // Targets editor root, `cm-editor`
        "&": {
          fontFamily: '"Source Code Pro", monospace',
          fontSize: "1rem",
          borderRadius: theme.spacing(0.75),
          borderBottomLeftRadius: "0px",
          border: `1px solid #CEA6A0`,
        },
        ".cm-activeLine": {
          // Darker rust for the active line
          backgroundColor: "#CEA6A044",
        },
        ".cm-activeLineGutter": {
          backgroundColor: "#CEA6A044",
        },
        ".cm-gutters": {
          // Light rust for gutters
          backgroundColor: "#FEFAF9",
          borderTopLeftRadius: theme.spacing(0.75),
        },
      }),
    [theme]
  );

  const readOnlyTheme = useMemo(
    () =>
      EditorView.theme({
        "&": {
          fontFamily: '"Source Code Pro", monospace',
          fontSize: "1rem",
          borderRadius: theme.spacing(0.75),
          borderBottomLeftRadius: "0px",
          border: `1px solid #CEA6A0`,
        },
        ".cm-content": {
          // Light grey representing read-only
          backgroundColor: "#EEEEEE80",
        },
        ".cm-gutters": {
          // Light rust for gutters
          backgroundColor: "#FEFAF9",
          borderTopLeftRadius: theme.spacing(0.75),
        },
      }),
    [theme]
  );

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

  const renderCodeSelectorButtons = useCallback(() => {
    return (
      <>
        <Tooltip title="Show live code editor">
          <CodeSelector
            selected={codeTypeSelector === SelectedCodeType.Live}
            onClick={() => dispatch(setLive())}
            sx={{
              borderRadius: "0px",
              borderBottomLeftRadius: "5px",
            }}
          >
            Live
          </CodeSelector>
        </Tooltip>
        <Tooltip title="Show code that was most recently executed (read only)">
          <CodeSelector
            selected={codeTypeSelector === SelectedCodeType.LastExecution}
            onClick={() => {
              if (!prevExecutedSelector) {
                setOpenPrevCodeWarning(true);
              }
              dispatch(setLastExecution());
            }}
            sx={{
              borderRadius: "0px",
              borderBottomRightRadius: "5px",
            }}
          >
            Last Execution
          </CodeSelector>
        </Tooltip>
      </>
    );
  }, [codeTypeSelector, dispatch, prevExecutedSelector]);

  const renderEditor = useCallback(() => {
    // Special value of "nocursor" instead of `true` makes the editor unfocuasable
    const readOnly = codeTypeSelector === SelectedCodeType.LastExecution;
    // Keeps both components mounted but only one visible at a time.
    // This is necessary to keep the editor state when switching between live and read only.
    // Otherwise, the liver editor mount causes side effects, such as creating a new ws connection.
    // The alternative is to unmount the editor when switching between live and read only by setting the
    // `key` prop. Although the editors would lose state, the new ws connection would trigger a snapshot
    // which would restore the live editor state.
    const liveEditor = (
      <CodeMirror
        id="live-editor"
        className="editor"
        height="100%"
        extensions={[rust(), extraCursorsPlugin, rustTheme]}
        onUpdate={handleEditorChange}
        onCreateEditor={(view, state) => {
          setView(view);
        }}
        style={{
          marginTop: theme.spacing(1.5),
          flex: 1,
          display: readOnly ? "none" : "block",
          // Add scrollbars when content overflows
          overflow: "auto",
        }}
        basicSetup={liveEditorOptions}
        editable={true}
        readOnly={false}
      />
    );
    const readOnlyEditor = (
      <CodeMirror
        id="read-only-editor"
        value={lastExecutedCode}
        className="editor"
        height="100%"
        // Remove rust syntax highlighting to make it visually apparent
        // that the editor is read only
        extensions={[readOnlyTheme]}
        style={{
          marginTop: theme.spacing(1.5),
          flex: 1,
          display: readOnly ? "block" : "none",
          // Add scrollbars when content overflows
          overflow: "auto",
        }}
        basicSetup={readOnlyEditorOptions}
        editable={false}
        readOnly={true}
      />
    );
    return (
      <>
        {liveEditor}
        {readOnlyEditor}
      </>
    );
  }, [
    extraCursorsPlugin,
    setView,
    theme,
    codeTypeSelector,
    rustTheme,
    lastExecutedCode,
    readOnlyTheme,
    handleEditorChange,
  ]);

  return (
    <Stack direction="column" sx={{ width: "100%", height: "100%", pb: 100 }}>
      {renderEditor()}
      <Stack direction="row">{renderCodeSelectorButtons()}</Stack>
      <Snackbar
        open={openPrevCodeWarning}
        autoHideDuration={10000}
        TransitionComponent={Grow}
        onClose={() => setOpenPrevCodeWarning(false)}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
      >
        <Alert severity="info">
          No code has been executed before so the "Last Execution" panel is
          empty. Run some code to see it here.
        </Alert>
      </Snackbar>
    </Stack>
  );
}

export default Editor;

export { SelectedCodeType };
