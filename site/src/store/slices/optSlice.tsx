/**
 * Selects the optimization level to use.
 * Optimization level can be Debug or Release.
 */

import { createSlice } from "@reduxjs/toolkit";

enum OptimizationLevel {
  Debug = "Debug",
  Release = "Release",
}

const optSlice = createSlice({
  name: "optimization",
  initialState: {
    level: OptimizationLevel.Release,
  },
  reducers: {
    setOptLevel(state, action) {
      state.level = action.payload;
    },
  },
});

export { OptimizationLevel };
export const { setOptLevel } = optSlice.actions;
export default optSlice.reducer;
