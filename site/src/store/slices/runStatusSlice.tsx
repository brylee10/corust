/**
 *  Holds metadata on the status of the previous run.
 *  Particularly the state on the alert panels these statuses trigger.
 */

import { createSlice } from "@reduxjs/toolkit";

const runStatusSlice = createSlice({
  name: "runStatus",
  initialState: {
    outputTooLargeOpen: false,
  },
  reducers: {
    setOutputTooLargeOpen(state) {
      state.outputTooLargeOpen = true;
    },
    setOutputTooLargeClose(state) {
      state.outputTooLargeOpen = false;
    },
  },
});

export const { setOutputTooLargeOpen, setOutputTooLargeClose } =
  runStatusSlice.actions;
export default runStatusSlice.reducer;
