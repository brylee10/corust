import React, { useCallback } from "react";
import {
  Button,
  ButtonGroup,
  Tooltip,
  styled,
  Stack,
  Typography,
} from "@mui/material";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import {
  RustChannel,
  setChannel,
  setChannelVersion,
} from "@/store/slices/channelSlice";
import { OptLevel, setOptLevel } from "@/store/slices/optSlice";
import { useDispatch, useSelector } from "react-redux";
import { RootState } from "@/store/store";
import {
  WsClientTextMsg,
  WsClientTextMsgType,
  WsConfigUpdate,
} from "@/components/mainPage";
import { selectUserState } from "@/store/slices/userSlice";
import StyledPopover from "@/components/ui/StyledPopover";

const ConfigButton = styled(Button)(({ theme }) => ({
  padding: "10px 15px",
  backgroundColor: theme.palette.secondary.main,
  color: "black",
  border: "none",
  cursor: "pointer",
  fontWeight: "bold",
  lineHeight: "1.25",
  height: 38,
  alignSelf: "center",
}));

export const commonButtonStyle = {
  justifyContent: "flex-start",
  textAlign: "left",
  pt: 1,
  pb: 1,
  pl: 2,
  pr: 2,
  display: "block",
  // Placeholder empty border to prevent button
  // from shrinking when not hovered
  border: "1px solid transparent",
};

export const commonTypographyStyle = {
  maxWidth: "300px",
  // No auto capitalization
  textTransform: "none",
  fontSize: "12px",
};

interface OptButtonProps {
  level: OptLevel;
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
  wsSendRef: React.MutableRefObject<
    (wsMessage: WsClientTextMsg) => void | null
  >;
}

function RunConfigButtons({
  stableVersion,
  betaVersion,
  nightlyVersion,
  wsSendRef,
}: RunConfigButtonsProps) {
  const dispatch = useDispatch();
  const cargoCommand = useSelector(
    (state: RootState) => state.cargoCommandSelector.command
  );
  // Current User
  const currUser = useSelector(selectUserState);
  // Optimization level
  const optLevel = useSelector((state: RootState) => state.optSelector.level);
  const [optAnchor, setOptAnchor] = React.useState<HTMLButtonElement | null>(
    null
  );
  const optPopoverOpen = Boolean(optAnchor);
  // Rust channel
  const channel = useSelector(
    (state: RootState) => state.channelSelector.channel
  );
  const channelVersion = useSelector(
    (state: RootState) => state.channelVersionSelector[channel]
  );
  const [channelAnchor, setChannelAnchor] =
    React.useState<HTMLButtonElement | null>(null);
  const channelPopoverOpen = Boolean(channelAnchor);

  const sendRunnerConfig = useCallback(
    (optLevel: OptLevel, channel: RustChannel) => {
      if (currUser.username) {
        const runnerConfig: WsConfigUpdate = {
          type: WsClientTextMsgType.WsConfigUpdate,
          cargoCommand,
          optLevel,
          channel,
          username: currUser.username,
        };
        wsSendRef.current(runnerConfig);
      }
    },
    [cargoCommand, wsSendRef, currUser]
  );

  // Optimization level buttons
  const OptButton = ({ level, description }: OptButtonProps) => (
    <Button
      fullWidth
      sx={commonButtonStyle}
      onClick={() => {
        dispatch(setOptLevel(level));
        sendRunnerConfig(level, channel);
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
        dispatch(setChannel(channel));
        const channelVersionPayload = { channel, version };
        dispatch(setChannelVersion(channelVersionPayload));
        sendRunnerConfig(optLevel, channel);
        handleChannelPopoverClose();
      }}
    >
      <Typography variant="subtitle2" fontWeight="bold" color="text.primary">
        {channel}
      </Typography>
      <Typography color="text.secondary" sx={commonTypographyStyle}>
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
      <ButtonGroup
        sx={{
          boxShadow:
            "rgba(0, 0, 0, 0.2) 0px 3px 1px -2px, rgba(0, 0, 0, 0.14) 0px 2px 2px 0px, rgba(0, 0, 0, 0.12) 0px 1px 5px 0px;",
        }}
      >
        <Tooltip title="Optimization Level" arrow>
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
        <Tooltip
          title={`Rust ${channel} Channel Version ${channelVersion}`}
          arrow
        >
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
            level={OptLevel.Release}
            description="Build with optimizations."
          />
          <OptButton
            level={OptLevel.Debug}
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
