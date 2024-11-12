/**
 * Selects the optimization level to use.
 * Optimization level can be Debug or Release.
 */

import { createSlice } from "@reduxjs/toolkit";

enum OptLevel {
  Debug = "Debug",
  Release = "Release",
}

const optSlice = createSlice({
  name: "optimization",
  initialState: {
    level: OptLevel.Release,
  },
  reducers: {
    setOptLevel(state, action) {
      state.level = action.payload;
    },
  },
});

export { OptLevel };
export const { setOptLevel } = optSlice.actions;
export default optSlice.reducer;
