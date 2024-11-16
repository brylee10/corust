import React, { useCallback, useMemo, useState } from "react";

import { RunStatus, RunState } from "../editor/runOutputDisplay.tsx";
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
import {
  CargoCommand,
  setCargoCommand,
} from "../../store/slices/cargoCommandSlice.tsx";
import { useDispatch, useSelector } from "react-redux";
import { RootState } from "../../store/store.tsx";
import {
  WsClientTextMsg,
  WsClientTextMsgType,
  WsConfigUpdate,
} from "../../App.tsx";
import { selectUserState } from "../../store/slices/userSlice.tsx";

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
  setCargoOutputOpen: (show: boolean) => void;
  executeCode: () => void;
  wsSendRef: React.MutableRefObject<(wsMessage: WsClientTextMsg) => void>;
}

// Represents the `Run` or `Build` button in the header bar
function RunButton({
  runStatus,
  setShowCargoOutput,
  setCargoOutputOpen,
  executeCode,
  wsSendRef,
}: RunButtonProps) {
  const dispatch = useDispatch();
  const currUser = useSelector(selectUserState);
  const channel = useSelector(
    (state: RootState) => state.channelSelector.channel
  );
  const optLevel = useSelector((state: RootState) => state.optSelector.level);
  // Cargo Command
  // The `cargoCommand` is inferred by the parent component.
  // The `cargoCommand` manually set by the user can override the `autoCargoCommand`.
  const cargoCommand = useSelector(
    (state: RootState) => state.cargoCommandSelector.command
  );
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

  const sendRunnerConfig = useCallback(
    (cargoCommand: CargoCommand) => {
      if (currUser.username !== undefined) {
        const runnerConfig: WsConfigUpdate = {
          type: WsClientTextMsgType.WsConfigUpdate,
          cargoCommand: cargoCommand,
          optLevel: optLevel,
          channel: channel,
          username: currUser.username,
        };
        wsSendRef.current(runnerConfig);
      }
    },
    [optLevel, channel, wsSendRef, currUser]
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
            dispatch(setCargoCommand(cargoCommand));
            sendRunnerConfig(cargoCommand);
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
    [handleCargoCommandPopoverClose, dispatch]
  );

  const renderRunButton = useCallback(() => {
    const buttonText = buttonTextMap[cargoCommand];
    const cargoCommandCapitalized =
      cargoCommand.charAt(0).toUpperCase() + cargoCommand.slice(1);
    const enabledButton = (
      <>
        <ButtonGroup>
          <Tooltip title={`${cargoCommandCapitalized} the code`}>
            <CustomRunButton
              variant="contained"
              size="small"
              onClick={() => {
                setShowCargoOutput(true);
                setCargoOutputOpen(true);
                executeCode();
              }}
              endIcon={<PlayArrowIcon />}
            >
              {buttonText}
            </CustomRunButton>
          </Tooltip>
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
    setCargoOutputOpen,
  ]);

  return renderRunButton();
}

export default RunButton;
