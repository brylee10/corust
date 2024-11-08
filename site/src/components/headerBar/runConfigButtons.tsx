import React, { useState } from "react";
import {
  Button,
  ButtonGroup,
  Tooltip,
  styled,
  Switch,
  Stack,
  Typography,
} from "@mui/material";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";

const ConfigButton = styled(Button)({
  padding: "10px 15px",
  backgroundColor: "#F5EEE3",
  color: "black",
  border: "none",
  cursor: "pointer",
  fontWeight: "bold",
  lineHeight: "1.25",
  height: 38,
  alignSelf: "center",
});

const UnclickableConfigButton = styled(ConfigButton)({
  //   pointerEvents: "none", // Disable mouse events
  cursor: "default", // Use default cursor
  "&:hover": {
    backgroundColor: "#F5EEE3", // Consistent hover color
    // Keep consistent box shadow on hover
    boxShadow:
      "0px 3px 1px -2px rgba(0, 0, 0, 0.2), 0px 2px 2px 0px rgba(0, 0, 0, 0.14), 0px 1px 5px 0px rgba(0, 0, 0, 0.12)",
  },
});

const ConfigButtonGroup = styled(ButtonGroup)({
  border: "none",
  "&:hover": {
    border: "none",
  },
});

interface RunConfigButtonsProps {
  rustVersion: string;
}

function RunConfigButtons({ rustVersion }: RunConfigButtonsProps) {
  const [liveMode, setLiveMode] = useState(true);
  return (
    <Stack direction="row" spacing={2} sx={{ alignItems: "center" }}>
      <ButtonGroup>
        <ConfigButton
          variant="contained"
          size="small"
          endIcon={<KeyboardArrowDownIcon />}
          color="secondary"
        >
          RELEASE
        </ConfigButton>
        <Tooltip title={`Rust Stable Version ${rustVersion}`}>
          <UnclickableConfigButton
            variant="contained"
            size="small"
            disableRipple
            disableFocusRipple
            disableTouchRipple
            color="secondary"
          >
            STABLE
          </UnclickableConfigButton>
        </Tooltip>
      </ButtonGroup>
      <Stack direction="row" spacing={1} sx={{ alignItems: "center" }}>
        <Typography>Recent Run</Typography>
        <Switch checked={liveMode} onChange={() => setLiveMode(!liveMode)} />
        <Typography>Live</Typography>
      </Stack>
    </Stack>
  );
}

export default RunConfigButtons;
