/**
 * Selects the Rust channel to use.
 * Channel can be stable, beta, or nightly.
 */

import { createSlice, PayloadAction } from "@reduxjs/toolkit";
type Version = string;

enum RustChannel {
  Stable = "Stable",
  Beta = "Beta",
  Nightly = "Nightly",
}

const channelSlice = createSlice({
  name: "channel",
  initialState: {
    channel: RustChannel.Stable,
  },
  reducers: {
    setChannel(state, action) {
      state.channel = action.payload;
    },
  },
});

// Define the payload type for `setChannelVersion`
interface ChannelVersionPayload {
  channel: RustChannel;
  version: Version;
}

// Map of each `RustChannel` to its version.
const channelVersionSlice = createSlice({
  name: "channelVersion",
  initialState: {
    [RustChannel.Stable]: "",
    [RustChannel.Beta]: "",
    [RustChannel.Nightly]: "",
  },
  reducers: {
    setChannelVersion: (
      state,
      action: PayloadAction<ChannelVersionPayload>
    ) => {
      const { channel, version } = action.payload;
      state[channel] = version;
    },
  },
});

export { RustChannel };
export const { setChannel } = channelSlice.actions;
export const { setChannelVersion } = channelVersionSlice.actions;
export const channelReducer = channelSlice.reducer;
export const channelVersionReducer = channelVersionSlice.reducer;
