import { useCallback, useEffect, useState } from "react";
import CloseIcon from "@mui/icons-material/Close";
import { IconButton, Tooltip } from "@mui/material";
import React from "react";

// Output of the compilation + runner
type RunType = "Execute";

interface RunOutput {
  runType: RunType;
  stderr: string;
  stdout: string;
  exitCode: number;
}

interface RunOutputProps {
  runOutput: RunOutput | null;
}

function RunOutputDisplay({ runOutput }: RunOutputProps) {
  const [stderr, setStderr] = useState<string>("");
  const [stdout, setStdout] = useState<string>("");
  const [open, setOpen] = useState<boolean>(true);

  useEffect(() => {
    console.log("Run Output: ", runOutput);
  }, [runOutput]);

  useEffect(() => {
    if (runOutput) {
      setStderr(runOutput.stderr);
      setStdout(runOutput.stdout);
    }
  }, [runOutput]);

  const closeDisplay = useCallback(() => {
    setOpen(false);
  }, []);

  const openDisplay = useCallback(() => {
    setOpen(true);
  }, []);

  const renderOpenDisplay = useCallback(() => {
    return (
      <div className="runner-output open">
        <div className="container">
          <div className="title">OUTPUT</div>
          <div className="body">
            <div className="subtitle">Standard Error</div>
            <div className="content">{stderr}</div>
          </div>
          <div className="body">
            <div className="subtitle">Standard Out</div>
            <div className="content">{stdout}</div>
          </div>
        </div>
        <div className="close">
          <Tooltip title="Close Display">
            <IconButton onClick={closeDisplay}>
              <CloseIcon />
            </IconButton>
          </Tooltip>
        </div>
      </div>
    );
  }, [stderr, stdout, closeDisplay]);

  const renderCloseDisplay = useCallback(() => {
    return (
      <div className="runner-output closed">
        <div className="container">
          <Tooltip title="Open Display">
            <div className="title" onClick={openDisplay}>
              OUTPUT
            </div>
          </Tooltip>
        </div>
      </div>
    );
  }, [openDisplay]);

  // When additional RunTypes are supported, multiple header names will be supported
  return (
    <React.Fragment>
      {open ? renderOpenDisplay() : renderCloseDisplay()}
    </React.Fragment>
  );
}

export type { RunOutput, RunType };
export { RunOutputDisplay };
