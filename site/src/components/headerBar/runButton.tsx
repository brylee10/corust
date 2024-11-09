import React, { useCallback, useMemo, useState } from "react";

import { RunStatus, RunState } from "../runOutputDisplay.tsx";
import { CargoCommand } from "../../App.tsx";
import {
  commonButtonStyle,
  commonTypographyStyle,
  StyledPopover,
} from "./runConfigButtons.tsx";

import {
  Button,
  ButtonGroup,
  Stack,
  Tooltip,
  Typography,
  styled,
} from "@mui/material";
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

interface CargoCommandButtonProps {
  cargoCommand: CargoCommand;
  description: React.JSX.Element;
}

interface RunButtonProps {
  runStatus: RunStatus | null;
  setShowCargoOutput: (show: boolean) => void;
  executeCode: () => void;
  cargoCommand: CargoCommand;
  setCargoCommand: (cargoCommand: CargoCommand) => void;
}

// Represents the `Run` or `Build` button in the header bar
function RunButton({
  runStatus,
  setShowCargoOutput,
  executeCode,
  cargoCommand,
  setCargoCommand,
}: RunButtonProps) {
  // Cargo Command
  // The `autoCargoCommand` is inferred by the parent component.
  // The user set `cargoCommand` can override the `autoCargoCommand`.
  const [cargoCommandPopoverOpen, setCargoCommandPopoverOpen] = useState(false);
  const [cargoCommandAnchor, setCargoCommandAnchor] =
    useState<null | HTMLElement>(null);

  const buttonTextMap = useMemo(
    () => ({
      [CargoCommand.Build]: "BUILD",
      [CargoCommand.Run]: "RUN",
      [CargoCommand.Test]: "TEST",
    }),
    []
  );

  const handleCargoCommandPopoverClose = useCallback(() => {
    setCargoCommandPopoverOpen(false);
  }, [setCargoCommandPopoverOpen]);

  const handleCargoCommandPopoverOpen = useCallback(
    (event: React.MouseEvent<HTMLButtonElement>) => {
      setCargoCommandAnchor(event.currentTarget);
      setCargoCommandPopoverOpen(true);
    },
    [setCargoCommandAnchor, setCargoCommandPopoverOpen]
  );

  const CargoCommandButton = useCallback(
    ({ cargoCommand, description }: CargoCommandButtonProps) => {
      return (
        <Button
          fullWidth
          sx={commonButtonStyle}
          onClick={() => {
            setCargoCommand(cargoCommand);
            handleCargoCommandPopoverClose();
          }}
        >
          <Typography
            variant="subtitle2"
            fontWeight="bold"
            color="text.primary"
          >
            {cargoCommand}
          </Typography>
          <Typography
            variant="subtitle2"
            color="text.secondary"
            sx={commonTypographyStyle}
          >
            {description}
          </Typography>
        </Button>
      );
    },
    [handleCargoCommandPopoverClose, setCargoCommand]
  );

  const renderRunButton = useCallback(() => {
    const buttonText = buttonTextMap[cargoCommand];
    const enabledButton = (
      <>
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
          <Tooltip title="Select Cargo Command">
            <CustomSelectButton
              variant="contained"
              size="small"
              color="secondary"
              onClick={handleCargoCommandPopoverOpen}
            >
              <MoreHorizIcon />
            </CustomSelectButton>
          </Tooltip>
        </ButtonGroup>
        <StyledPopover
          id={"cargo-command-popover"}
          open={cargoCommandPopoverOpen}
          anchorEl={cargoCommandAnchor}
          onClose={handleCargoCommandPopoverClose}
        >
          <Stack direction={"column"}>
            <CargoCommandButton
              cargoCommand={CargoCommand.Run}
              description={
                <>
                  Build and run code (
                  <code className="code-highlight">cargo run</code>).
                </>
              }
            />
            <CargoCommandButton
              cargoCommand={CargoCommand.Build}
              description={
                <>
                  Build code (
                  <code className="code-highlight">cargo build</code>
                  ).
                </>
              }
            />
            <CargoCommandButton
              cargoCommand={CargoCommand.Test}
              description={
                <>
                  Build code and run tests (
                  <code className="code-highlight">cargo test</code>).
                </>
              }
            />
          </Stack>
        </StyledPopover>
      </>
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
  }, [
    cargoCommand,
    cargoCommandPopoverOpen,
    cargoCommandAnchor,
    handleCargoCommandPopoverClose,
    CargoCommandButton,
    runStatus?.runState,
    setShowCargoOutput,
    executeCode,
    handleCargoCommandPopoverOpen,
    buttonTextMap,
  ]);

  return renderRunButton();
}

export default RunButton;
