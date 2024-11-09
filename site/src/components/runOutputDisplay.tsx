import { useCallback, useEffect, useState } from "react";
import CloseIcon from "@mui/icons-material/Close";
import {
  Alert,
  Box,
  Grow,
  IconButton,
  Snackbar,
  Tooltip,
  useTheme,
} from "@mui/material";
import React from "react";
import { BouncingDotsLoader } from "./bouncingDotsLoader";
import TerminalIcon from "@mui/icons-material/Terminal";

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

// Utility to update the RunStatus object based on the RunStateUpdate. Similar to a reducer.
const updateRunStatus = (
  runType: RunType,
  runStateUpdate: RunStateUpdate,
  runStatus: RunStatus | null
): RunStatus => {
  if (runStatus === null) {
    runStatus = initRunStatus(runType);
  }

  switch (runStateUpdate) {
    case "RunStarted":
      // Resets status errors on new run
      runStatus = { ...runStatus, runState: RunState.Running };
      break;
    case "RunEnded":
      runStatus = { ...runStatus, runState: RunState.Ended };
      break;
    case "ConcurrentCompilation":
      runStatus = { ...runStatus, concurrentCompilation: true };
      break;
    case "ClientStateOutOfSync":
      runStatus = { ...runStatus, clientStateOutOfSync: true };
      break;
    case "StdoutErrTooLarge":
      runStatus = { ...runStatus, stdoutErrTooLarge: true };
      break;
  }
  console.debug("Updated run status", runStatus, runType, runStateUpdate);
  return runStatus;
};

function RunOutputDisplay({
  runOutput,
  runStatus,
  open,
  setOpen,
}: RunOutputProps) {
  const theme = useTheme();

  const [stderr, setStderr] = useState<string>("");
  const [stdout, setStdout] = useState<string>("");
  // Alerts user a compilation was rejected because another compilation was in progress
  const [showConcurrentCompError, setShowConcurrentCompError] =
    useState<boolean>(false);
  // Alerts user a compilation was rejected because another compilation was in progress
  const [showOutputSizeError, setShowOutputSizeError] =
    useState<boolean>(false);
  // Indicates is running icon
  const [showRunningIcon, setShowRunningIcon] = useState<boolean>(false);

  useEffect(
    function runStatusUpdateState() {
      console.debug!("Received run status message", runStatus);
      if (runStatus !== null) {
        setShowConcurrentCompError(runStatus.concurrentCompilation);
        setShowOutputSizeError(runStatus.stdoutErrTooLarge);
        setShowRunningIcon(runStatus.runState === RunState.Running);
      }
    },
    [runStatus]
  );

  useEffect(() => {
    if (runOutput) {
      console.debug("Run Output: " + JSON.stringify(runOutput));
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
      <Box className="runner-output open">
        <Box className="container">
          <Box className="title-container-vert">
            <Box className="title-open-vert">OUTPUT</Box>
            <Box className="close">
              <Tooltip title="Close Output">
                <IconButton onClick={closeOutput}>
                  <CloseIcon />
                </IconButton>
              </Tooltip>
            </Box>
          </Box>
          {showRunningIcon && (
            <Box className="body">
              <Box className="subtitle">Progress</Box>
              <BouncingDotsLoader />
            </Box>
          )}
          <Box className="body">
            <Box className="subtitle">Standard Error</Box>
            <Box className="content">
              <pre>{stderr}</pre>
            </Box>
          </Box>
          <Box className="body">
            <Box className="subtitle">Standard Out</Box>
            <Box className="content">
              <pre>{stdout}</pre>
            </Box>
          </Box>
        </Box>
      </Box>
    );
  }, [stderr, stdout, closeOutput, showRunningIcon]);

  const renderClosedOutput = useCallback(() => {
    return (
      <Tooltip title="Open Output">
        <Box className="runner-output closed vertical" onClick={openOutput}>
          <Box className="container">
            <IconButton
              size="large"
              sx={{ color: theme.palette.primary.light }}
            >
              <TerminalIcon fontSize="inherit" />
            </IconButton>
          </Box>
        </Box>
      </Tooltip>
    );
  }, [openOutput, theme]);

  // When additional RunTypes are supported, multiple header names will be supported
  return (
    <React.Fragment>
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
        onClose={() => setShowOutputSizeError(false)}
      >
        <Alert severity="warning" onClose={() => setShowOutputSizeError(false)}>
          Stderr and stdout output size exceeded max output{" "}
          {MAX_OUTPUT_SIZE_BYTES} bytes. Process killed.
        </Alert>
      </Snackbar>
    </React.Fragment>
  );
}

export type { RunOutput, RunType, RunStatus, ServerRunStatus };
export { RunOutputDisplay, RunState, updateRunStatus };
