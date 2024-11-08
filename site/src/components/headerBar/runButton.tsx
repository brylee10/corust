import React, { useCallback } from "react";

import { RunStatus, RunState } from "../runOutputDisplay.tsx";
import { CargoCommand } from "../../App.tsx";

import { Button, ButtonGroup, Tooltip, styled } from "@mui/material";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import MoreHorizIcon from "@mui/icons-material/MoreHoriz";

const CustomRunButton = styled(Button)({
  padding: "10px 20px",
  color: "white",
  border: "none",
  borderRadius: "5px 0px 0px 5px",
  cursor: "pointer",
  fontWeight: "bold",
  lineHeight: "1.25",
  height: 38,
  alignSelf: "center",
});

const CustomSelectButton = styled(Button)({
  minWidth: "32px",
  color: "black",
  border: "none",
  borderRadius: "0px 5px 5px 0px",
  cursor: "pointer",
  height: 38,
  alignSelf: "center",
});

const DisabledRunButton = styled(Button)({
  padding: "10px 20px",
  backgroundColor: "#A3A3A3",
  color: "white",
  border: "none",
  // Do not show a cursor helper
  cursor: "default",
  borderRadius: "5px 0px 0px 5px",
  fontWeight: "bold",
  lineHeight: "1.25",
  alignSelf: "center",
  "&:hover": {
    backgroundColor: "#A3A3A3",
  },
});

const DisabledSelectButton = styled(Button)({
  minWidth: "32px",
  backgroundColor: "#CECECE",
  color: "white",
  border: "none",
  borderRadius: "0px 5px 5px 0px",
  cursor: "pointer",
  height: 38,
  alignSelf: "center",
  "&:hover": {
    backgroundColor: "#CECECE",
  },
});

interface RunButtonProps {
  runStatus: RunStatus | null;
  setShowCargoOutput: (show: boolean) => void;
  executeCode: () => void;
  cargoCommand: CargoCommand;
}

// Represents the `Run` or `Build` button in the header bar
function RunButton({
  runStatus,
  setShowCargoOutput,
  executeCode,
  cargoCommand,
}: RunButtonProps) {
  const renderRunButton = useCallback(() => {
    const buttonTextMap = {
      [CargoCommand.Build]: "BUILD",
      [CargoCommand.Run]: "RUN",
      [CargoCommand.Test]: "TEST",
    };
    const buttonText = buttonTextMap[cargoCommand];
    const enabledButton = (
      <ButtonGroup>
        <CustomRunButton
          variant="contained"
          size="small"
          onClick={() => {
            setShowCargoOutput(true);
            executeCode();
          }}
          endIcon={<PlayArrowIcon />}
        >
          {buttonText}
        </CustomRunButton>
        <CustomSelectButton variant="contained" size="small" color="secondary">
          <MoreHorizIcon />
        </CustomSelectButton>
      </ButtonGroup>
    );

    const disabledButton = (
      <Tooltip title="Code executing, cannot start simultaneous run.">
        <ButtonGroup>
          <DisabledRunButton
            variant="contained"
            size="small"
            endIcon={<PlayArrowIcon />}
          >
            {buttonText}
          </DisabledRunButton>
          <DisabledSelectButton variant="contained" size="small">
            <MoreHorizIcon />
          </DisabledSelectButton>
        </ButtonGroup>
      </Tooltip>
    );
    return runStatus?.runState !== RunState.Running
      ? enabledButton
      : disabledButton;
  }, [executeCode, runStatus, setShowCargoOutput, cargoCommand]);

  return renderRunButton();
}

export default RunButton;
