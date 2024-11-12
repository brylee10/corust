/**
 * Selects parameters of code to display in the editor area.
 * View can be live (the currently edited code) or recent run (the code that was run last).
 * The recently run code is the code that was submitted for execution last.
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
    // Whether `recentRunCode` has ever been set
    recentRunCodeSet: false,
    // The code that was executed last
    recentRunCode: "",
    // The user who executed the most recent code
    executingUser: "",
  },
  reducers: {
    setLive: (state) => {
      state.type = SelectedCodeType.Live;
    },
    setRecentRun: (state) => {
      state.type = SelectedCodeType.RecentRun;
    },
    setRecentRunCode: (state, action) => {
      state.recentRunCode = action.payload;
      state.recentRunCodeSet = true;
    },
    setExecutingUser: (state, action) => {
      state.executingUser = action.payload;
    },
  },
});

export const { setLive, setRecentRun, setRecentRunCode, setExecutingUser } =
  codeSelectorSlice.actions;
export { SelectedCodeType };
export default codeSelectorSlice.reducer;
