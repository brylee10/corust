/**
 * Container for the editor and output display.
 * Determines the layout based on screen size and supports window resizing by users.
 */

import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";
import Editor from "./editor";
import { useCallback } from "react";
import { RunOutputDisplay } from "./runOutputDisplay";
import { EditorView, ViewUpdate } from "@uiw/react-codemirror";
import { useSelector } from "react-redux";
import { RootState } from "../../store/store";
import WindowSizeListener from "../windowSize";
import { styled } from "@mui/material";
import { Client } from "corust-components";
import { UserSelectionRange } from "../mainPage";

interface EditorContainerProps {
  setView: (view: EditorView) => void;
  handleEditorChange: (viewUpdate: ViewUpdate) => void;
  userArr: any[];
  client: Client;
  getCollabSelections: (client: Client) => UserSelectionRange[];
  showCargoOutput: boolean;
  cargoOutputOpen: boolean;
  setCargoOutputOpen: (open: boolean) => void;
  runOutput: any;
  runStatus: any;
}

interface NarrowScreenProps {
  isNarrowScreen: boolean;
}

const ResizeHandle = styled(PanelResizeHandle)<NarrowScreenProps>(
  ({ isNarrowScreen }) => ({
    margin: isNarrowScreen ? "5px 0" : "0 5px",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    cursor: isNarrowScreen ? "row-resize" : "col-resize",
    width: isNarrowScreen ? "100%" : "10px",
    height: isNarrowScreen ? "10px" : "100%",
    "&::before": {
      content: isNarrowScreen ? '"⋯"' : '"⋮"',
      fontSize: 14,
      color: "#888",
    },
  })
);

const StyledPanel = styled(Panel)<NarrowScreenProps>(({ isNarrowScreen }) => ({
  height: isNarrowScreen ? "100%" : "auto",
  display: "flex",
}));

function EditorContainer({
  setView,
  handleEditorChange,
  userArr,
  client,
  getCollabSelections,
  showCargoOutput,
  cargoOutputOpen,
  setCargoOutputOpen,
  runOutput,
  runStatus,
}: EditorContainerProps) {
  const isNarrowScreen = useSelector(
    (state: RootState) => state.displaySelector.isNarrowScreen
  );

  const renderCargoOutput = useCallback(() => {
    if (showCargoOutput) {
      if (cargoOutputOpen) {
        const cargoOutput = (
          <>
            <ResizeHandle isNarrowScreen={isNarrowScreen} />
            <StyledPanel
              isNarrowScreen={isNarrowScreen}
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
            </StyledPanel>
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
    isNarrowScreen,
  ]);

  const renderEditorAndOutput = useCallback(() => {
    const direction = isNarrowScreen ? "vertical" : "horizontal";
    return (
      <PanelGroup direction={direction} className="max-height">
        <Panel className="max-height" minSize={10} id={"1"}>
          <Editor
            setView={setView}
            handleEditorChange={handleEditorChange}
            userArr={userArr}
            client={client}
            getCollabSelections={getCollabSelections}
          />
        </Panel>
        {renderCargoOutput()}
      </PanelGroup>
    );
  }, [
    client,
    getCollabSelections,
    handleEditorChange,
    renderCargoOutput,
    userArr,
    isNarrowScreen,
    setView,
  ]);

  return (
    <>
      <WindowSizeListener />
      {renderEditorAndOutput()}
    </>
  );
}

export type { NarrowScreenProps };
export default EditorContainer;
