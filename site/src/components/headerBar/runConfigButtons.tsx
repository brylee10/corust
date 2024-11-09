import React, { useState, ReactNode, useEffect } from "react";
import {
  Button,
  ButtonGroup,
  Tooltip,
  styled,
  Stack,
  Typography,
  Popover,
  Grow,
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

const Arrow = styled("div")({
  width: 0,
  height: 0,
  borderLeft: "1rem solid transparent",
  borderRight: "1rem solid transparent",
  borderBottom: "1rem solid white",
  position: "absolute",
  top: "-8px",
  left: "50%",
  zIndex: 1,
});

const StyledPopover = ({
  id,
  open,
  anchorEl,
  onClose,
  children,
}: StyledPopoverProps) => {
  const [arrowPosition, setArrowPosition] = useState<{
    top: number;
    left: number;
  } | null>(null);

  useEffect(() => {
    if (anchorEl && open) {
      // Calculate position of the anchor element
      const rect = anchorEl.getBoundingClientRect();
      setArrowPosition({
        top: rect.bottom + window.scrollY, // Position arrow just below the anchor
        left: rect.left + rect.width / 2, // Center the arrow horizontally on the anchor
      });
    } else {
      setArrowPosition(null); // Hide arrow when Popover is closed or anchor is unavailable
    }
  }, [anchorEl, open]);

  return (
    <>
      {/* Conditionally render the Arrow below the anchorEl */}
      {arrowPosition && (
        <Grow
          in={open}
          timeout={300}
          style={{
            transformOrigin: "top center",
            transform: "translateX(-50%)",
          }}
        >
          <Arrow
            style={{
              top: arrowPosition.top,
              left: arrowPosition.left,
            }}
            className="arrow"
          />
        </Grow>
      )}

      <Popover
        id={id}
        open={open}
        anchorEl={anchorEl}
        onClose={onClose}
        anchorOrigin={{
          vertical: "bottom",
          horizontal: "center",
        }}
        transformOrigin={{
          vertical: "top",
          horizontal: "center",
        }}
        slotProps={{
          paper: {
            sx: {
              mt: "0.75rem",
              overflow: "visible",
            },
          },
        }}
      >
        {children}
      </Popover>
    </>
  );
};

const commonButtonStyle = {
  justifyContent: "flex-start",
  textAlign: "left",
  pt: 1,
  pb: 1,
  pl: 2,
  pr: 2,
  display: "block",
  // Placeholder empty border to prevent button
  // from shrinking when not hovered
  border: "1px solid #FFFFFF",
};

const commonTypographyStyle = {
  maxWidth: "250px",
  // No auto capitalization
  textTransform: "none",
};

enum OptimizationLevel {
  Debug = "Debug",
  Release = "Release",
}

enum RustChannel {
  Stable = "Stable",
  Beta = "Beta",
  Nightly = "Nightly",
}

interface StyledPopoverProps {
  id: string;
  open: boolean;
  anchorEl: HTMLElement | null;
  onClose: () => void;
  children: ReactNode;
}

interface OptButtonProps {
  level: OptimizationLevel;
  description: string;
}

interface ChannelButtonProps {
  channel: RustChannel;
  version: string;
  description: string;
}

interface RunConfigButtonsProps {
  stableVersion: string;
  betaVersion: string;
  nightlyVersion: string;
}

function RunConfigButtons({
  stableVersion,
  betaVersion,
  nightlyVersion,
}: RunConfigButtonsProps) {
  // Optimization level
  const [optAnchor, setOptAnchor] = React.useState<HTMLButtonElement | null>(
    null
  );
  const [optLevel, setOptLevel] = useState<OptimizationLevel>(
    OptimizationLevel.Release
  );
  const optPopoverOpen = Boolean(optAnchor);
  // Rust channel
  const [channelAnchor, setChannelAnchor] =
    React.useState<HTMLButtonElement | null>(null);
  const [channel, setChannel] = useState<RustChannel>(RustChannel.Stable);
  const [channelVersion, setChannelVersion] = useState<string>(stableVersion);
  const channelPopoverOpen = Boolean(channelAnchor);

  // Optimization level buttons
  const OptButton = ({ level, description }: OptButtonProps) => (
    <Button
      fullWidth
      sx={commonButtonStyle}
      onClick={() => {
        setOptLevel(level);
        handleOptPopoverClose();
      }}
    >
      <Typography variant="subtitle2" fontWeight="bold" color="text.primary">
        {level}
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

  const handleOptPopoverOpen = (event: React.MouseEvent<HTMLButtonElement>) => {
    setOptAnchor(event.currentTarget);
  };

  const handleOptPopoverClose = () => {
    setOptAnchor(null);
  };

  // Rust channel buttons
  const ChannelButton = ({
    channel,
    version,
    description,
  }: ChannelButtonProps) => (
    <Button
      fullWidth
      sx={commonButtonStyle}
      onClick={() => {
        setChannel(channel);
        setChannelVersion(version);
        handleChannelPopoverClose();
      }}
    >
      <Typography variant="subtitle2" fontWeight="bold" color="text.primary">
        {channel}
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

  const handleChannelPopoverOpen = (
    event: React.MouseEvent<HTMLButtonElement>
  ) => {
    setChannelAnchor(event.currentTarget);
  };

  const handleChannelPopoverClose = () => {
    setChannelAnchor(null);
  };

  return (
    <Stack direction="row" spacing={2} sx={{ alignItems: "center" }}>
      <ButtonGroup>
        <Tooltip title="Optimization Level">
          <ConfigButton
            variant="contained"
            size="small"
            endIcon={<KeyboardArrowDownIcon />}
            color="secondary"
            onClick={handleOptPopoverOpen}
          >
            {optLevel}
          </ConfigButton>
        </Tooltip>
        <Tooltip title={`Rust ${channel} Channel Version ${channelVersion}`}>
          <ConfigButton
            variant="contained"
            size="small"
            endIcon={<KeyboardArrowDownIcon />}
            color="secondary"
            onClick={handleChannelPopoverOpen}
          >
            {channel}
          </ConfigButton>
        </Tooltip>
      </ButtonGroup>
      <StyledPopover
        id={"optimization-popover"}
        open={optPopoverOpen}
        anchorEl={optAnchor}
        onClose={handleOptPopoverClose}
      >
        <Stack direction={"column"}>
          <OptButton
            level={OptimizationLevel.Release}
            description="Build with optimizations."
          />
          <OptButton
            level={OptimizationLevel.Debug}
            description="Build with debug information, without optimizations."
          />
        </Stack>
      </StyledPopover>
      <StyledPopover
        id={"channel-popover"}
        open={channelPopoverOpen}
        anchorEl={channelAnchor}
        onClose={handleChannelPopoverClose}
      >
        <Stack direction={"column"}>
          <ChannelButton
            channel={RustChannel.Stable}
            version={stableVersion}
            description={`Stable version ${stableVersion}`}
          />
          <ChannelButton
            channel={RustChannel.Beta}
            version={betaVersion}
            description={`Beta version ${betaVersion}`}
          />
          <ChannelButton
            channel={RustChannel.Nightly}
            version={nightlyVersion}
            description={`Nightly version ${nightlyVersion}`}
          />
        </Stack>
      </StyledPopover>
    </Stack>
  );
}

export default RunConfigButtons;

export { commonButtonStyle, commonTypographyStyle, StyledPopover };
