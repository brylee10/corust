/**
 * Selects the active editor view.
 * View can be live (the currently edited code) or recent run (the code that was run last).
 */

import { createSlice } from "@reduxjs/toolkit";

// Indicates whether the user code editor is displaying the live code
// or the code from the most recent run.
enum SelectedCodeType {
  Live = "Live",
  RecentRun = "Recent Run",
}

const codeSelectorSlice = createSlice({
  name: "codeSelector",
  initialState: {
    type: SelectedCodeType.Live,
  },
  reducers: {
    setLive: (state) => {
      state.type = SelectedCodeType.Live;
    },
    setRecentRun: (state) => {
      state.type = SelectedCodeType.RecentRun;
    },
  },
});

export const { setLive, setRecentRun } = codeSelectorSlice.actions;
export { SelectedCodeType };
export default codeSelectorSlice.reducer;
