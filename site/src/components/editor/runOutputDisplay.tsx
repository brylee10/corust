import { useCallback, useEffect, useState } from "react";
import CloseIcon from "@mui/icons-material/Close";
import {
  Alert,
  Box,
  Grow,
  IconButton,
  Snackbar,
  styled,
  Tooltip,
  useTheme,
} from "@mui/material";
import React from "react";
import { BouncingDotsLoader } from "./bouncingDotsLoader";
import TerminalIcon from "@mui/icons-material/Terminal";
import { useDispatch, useSelector } from "react-redux";
import { RootState } from "../../store/store";
import { setOutputTooLargeClose } from "../../store/slices/runStatusSlice";

const AUTO_HIDE_DURATION_MS: number = 6000;
// Maximum bytes that stdout or stderr can be before child process is killed
// This should be enough for reasonable usecases
const MAX_OUTPUT_SIZE_BYTES: number = 64000;

// Output of the compilation + runner
type RunType = "Execute";

type RunStateUpdate =
  | "RunStarted"
  | "RunEnded"
  | "ConcurrentCompilation"
  | "ClientStateOutOfSync"
  | "StdoutErrTooLarge";

interface RunOutput {
  runType: RunType;
  stderr: string;
  stdout: string;
  exitCode: number;
}

interface ServerRunStatus {
  runType: RunType;
  runStateUpdate: RunStateUpdate;
}

// An object which contains run state and error information
interface RunStatus {
  runType: RunType;
  runState: RunState;
  concurrentCompilation: boolean;
  clientStateOutOfSync: boolean;
  stdoutErrTooLarge: boolean;
}

interface RunOutputProps {
  runOutput: RunOutput | null;
  runStatus: RunStatus | null;
  open: boolean;
  setOpen: (open: boolean) => void;
}

enum RunState {
  // Before any run has started
  Init,
  // The most recent run is in progress
  Running,
  // The most recent run has ended
  Ended,
}

const initRunStatus = (runType: RunType): RunStatus => {
  return {
    runType: runType,
    runState: RunState.Init,
    concurrentCompilation: false,
    clientStateOutOfSync: false,
    stdoutErrTooLarge: false,
  };
};

const RunnerOutput = styled("div")<{ open: boolean; isNarrowScreen: boolean }>(
  ({ theme, open, isNarrowScreen }) => {
    const isDarkMode = theme.palette.mode === "dark";
    return {
      fontSize: 16,
      borderRadius: theme.spacing(0.75),
      background: isDarkMode ? "transparent" : "#fefaf9",
      paddingLeft: 10,
      paddingRight: 10,
      borderWidth: "thin",
      borderColor: isDarkMode ? "#CEA6A044" : "#CEA6A0",
      borderStyle: "solid",
      display: "flex",
      flexDirection: open ? "column" : undefined,
      paddingBottom: open ? 10 : undefined,
      overflow: open ? "auto" : undefined,
      flex: open ? 1 : 0,
      cursor: !open ? "pointer" : undefined,
      marginTop: !(isNarrowScreen && open) ? 12 : undefined,
      marginLeft: !isNarrowScreen && !open ? 12 : undefined,
    };
  }
);

const Container = styled("div")<{ open: boolean }>(({ open }) => ({
  fontFamily: '"Roboto", "Helvetica", "Arial", sans-serif',
  display: "flex",
  flexDirection: "column",
  flex: 1,
  alignItems: open ? undefined : "center",
  justifyContent: open ? undefined : "center",
}));

const TitleContainer = styled("div")({
  display: "flex",
  flexDirection: "row-reverse",
  position: "relative",
  paddingTop: 4,
});

const TitleOpen = styled("div")<{ isNarrowScreen: boolean }>(
  ({ isNarrowScreen }) => ({
    fontFamily: '"Roboto", "Helvetica", "Arial", sans-serif',
    position: "absolute",
    left: "50%",
    transform: "translateX(-50%)",
    paddingTop: 8,
    color: "#C96556",
    fontWeight: 600,
    textAlign: isNarrowScreen ? undefined : "center",
    flexGrow: isNarrowScreen ? undefined : 1,
  })
);

function RunOutputDisplay({
  runOutput,
  runStatus,
  open,
  setOpen,
}: RunOutputProps) {
  const dispatch = useDispatch();
  const theme = useTheme();
  const isNarrowScreen = useSelector(
    (state: RootState) => state.displaySelector.isNarrowScreen
  );
  const isDarkMode = useSelector(
    (state: RootState) => state.displaySelector.dark
  );

  const [stderr, setStderr] = useState<string>("");
  const [stdout, setStdout] = useState<string>("");
  // Alerts user a compilation was rejected because another compilation was in progress
  const [showConcurrentCompError, setShowConcurrentCompError] =
    useState<boolean>(false);
  // Alerts user the output of the compilation was too large
  const showOutputSizeError = useSelector(
    (state: RootState) => state.runStatusSelector.outputTooLargeOpen
  );
  // Indicates is running icon
  const [showRunningIcon, setShowRunningIcon] = useState<boolean>(false);

  useEffect(
    function runStatusUpdateState() {
      if (runStatus !== null) {
        setShowConcurrentCompError(runStatus.concurrentCompilation);
        setShowRunningIcon(runStatus.runState === RunState.Running);
      }
    },
    [runStatus]
  );

  useEffect(() => {
    if (runOutput) {
      setStderr(runOutput.stderr);
      setStdout(runOutput.stdout);
    }
  }, [runOutput]);

  const closeOutput = useCallback(() => {
    setOpen(false);
  }, [setOpen]);

  const openOutput = useCallback(() => {
    setOpen(true);
  }, [setOpen]);

  // <pre> preserves the error whitespacing
  const renderOpenedOutput = useCallback(() => {
    return (
      <RunnerOutput
        open={open}
        isNarrowScreen={isNarrowScreen}
        id={"runner-output-opened"}
      >
        <Container open={open} id={"container-opened"}>
          <TitleContainer>
            <TitleOpen isNarrowScreen={isNarrowScreen}>OUTPUT</TitleOpen>
            <Box className="close">
              <Tooltip title="Close Output" arrow>
                <IconButton onClick={closeOutput} size="small">
                  <CloseIcon />
                </IconButton>
              </Tooltip>
            </Box>
          </TitleContainer>
          {showRunningIcon && (
            <Box className="runner-output-body">
              <Box className="subtitle">Progress</Box>
              <BouncingDotsLoader />
            </Box>
          )}
          <Box className="runner-output-body">
            <Box className="subtitle">Standard Error</Box>
            <Box
              className="content"
              sx={{ color: isDarkMode ? "white" : "black" }}
            >
              <pre>{stderr}</pre>
            </Box>
          </Box>
          <Box className="runner-output-body">
            <Box className="subtitle">Standard Out</Box>
            <Box
              className="content"
              sx={{ color: isDarkMode ? "white" : "black" }}
            >
              <pre>{stdout}</pre>
            </Box>
          </Box>
        </Container>
      </RunnerOutput>
    );
  }, [
    stderr,
    stdout,
    closeOutput,
    showRunningIcon,
    isNarrowScreen,
    open,
    isDarkMode,
  ]);

  const renderClosedOutput = useCallback(() => {
    const placement = isNarrowScreen ? "top" : "left";
    return (
      <Tooltip
        title="Open output from last execution"
        arrow
        placement={placement}
      >
        <RunnerOutput
          open={open}
          isNarrowScreen={isNarrowScreen}
          onClick={openOutput}
          id={"runner-output-closed"}
        >
          <Container open={open} id={"container-closed"}>
            <IconButton
              size="large"
              sx={{ color: theme.palette.primary.light }}
            >
              <TerminalIcon fontSize="inherit" />
            </IconButton>
          </Container>
        </RunnerOutput>
      </Tooltip>
    );
  }, [openOutput, theme, open, isNarrowScreen]);

  // When additional RunTypes are supported, multiple header names will be supported
  return (
    <>
      {open ? renderOpenedOutput() : renderClosedOutput()}
      <Snackbar
        open={showConcurrentCompError}
        TransitionComponent={Grow}
        autoHideDuration={AUTO_HIDE_DURATION_MS}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
        onClose={() => setShowConcurrentCompError(false)}
      >
        <Alert
          severity="warning"
          onClose={() => setShowConcurrentCompError(false)}
        >
          Compilation already occurring. Run request not processed. Please wait
          for the current compilation to finish.
        </Alert>
      </Snackbar>
      <Snackbar
        open={showOutputSizeError}
        TransitionComponent={Grow}
        autoHideDuration={AUTO_HIDE_DURATION_MS}
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
        onClose={() => dispatch(setOutputTooLargeClose())}
      >
        <Alert
          severity="warning"
          onClose={() => dispatch(setOutputTooLargeClose())}
        >
          Stderr and stdout output size exceeded max output{" "}
          {MAX_OUTPUT_SIZE_BYTES} bytes. Process killed.
        </Alert>
      </Snackbar>
    </>
  );
}

export type { RunOutput, RunType, RunStatus, RunStateUpdate, ServerRunStatus };
export { RunOutputDisplay, RunState, initRunStatus };
