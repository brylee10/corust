/**
 * Selects the optimization level to use.
 * Optimization level can be Debug or Release.
 */

import { createSlice, PayloadAction } from "@reduxjs/toolkit";

enum OptLevel {
  Debug = "Debug",
  Release = "Release",
}

interface OptState {
  level: OptLevel;
  lastExecuteLevel: OptLevel | null;
}

const optSlice = createSlice({
  name: "optimization",
  initialState: {
    level: OptLevel.Release,
    lastExecuteLevel: null,
  } as OptState,
  reducers: {
    setOptLevel(state, action: PayloadAction<OptLevel>) {
      state.level = action.payload;
    },
    setLastExecuteOptLevel(state, action: PayloadAction<OptLevel>) {
      state.lastExecuteLevel = action.payload;
    },
  },
});

export { OptLevel };
export type { OptState };
export const { setOptLevel, setLastExecuteOptLevel } = optSlice.actions;
export default optSlice.reducer;
