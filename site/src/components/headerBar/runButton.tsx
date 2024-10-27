import React, { useCallback } from "react";
import { Button, Tooltip, styled } from "@mui/material";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import { RunStatus, RunState } from "../runOutputDisplay.tsx";
import { CargoCommand } from "../../App.tsx";

const CustomButton = styled(Button)({
  padding: "10px 20px",
  // Rust!
  backgroundColor: "#CE412B",
  color: "white",
  border: "none",
  borderRadius: "5px",
  cursor: "pointer",
  fontWeight: "bold",
  lineHeight: "1.25",
  height: 38,
  alignSelf: "center",
  "&:hover": {
    backgroundColor: "#CE412B",
  },
});

const DisabledButton = styled(Button)({
  padding: "10px 20px",
  backgroundColor: "gray",
  color: "white",
  border: "none",
  // Do not show a cursor helper
  cursor: "default",
  borderRadius: "5px",
  fontWeight: "bold",
  lineHeight: "1.25",
  alignSelf: "center",
  "&:hover": {
    backgroundColor: "gray",
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
    const buttonText = cargoCommand === CargoCommand.Build ? "BUILD" : "RUN";
    const enabledButton = (
      <>
        <CustomButton
          variant="contained"
          size="small"
          onClick={() => {
            setShowCargoOutput(true);
            executeCode();
          }}
          endIcon={<PlayArrowIcon />}
        >
          {buttonText}
        </CustomButton>
      </>
    );

    const disabledButton = (
      <Tooltip title="Code executing, cannot start simultaneous run.">
        <DisabledButton
          variant="contained"
          size="small"
          endIcon={<PlayArrowIcon />}
        >
          {buttonText}
        </DisabledButton>
      </Tooltip>
    );
    return runStatus?.runState !== RunState.Running
      ? enabledButton
      : disabledButton;
  }, [executeCode, runStatus, setShowCargoOutput, cargoCommand]);

  return renderRunButton();
}

export default RunButton;
