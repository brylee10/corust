/**
 *  Records whether the window is a narrow screen or not.
 *  A narrow screen is defined as a screen with a width of `NARROW_SCREEN_PX` or less.
 */

import { createSlice } from "@reduxjs/toolkit";

const NARROW_SCREEN_PX = 1600;

const windowSizeSlice = createSlice({
  name: "windowSize",
  initialState: {
    isNarrowScreen: false,
  },
  reducers: {
    setNarrowScreen: (state, action) => {
      state.isNarrowScreen = action.payload;
    },
  },
});

export const { setNarrowScreen } = windowSizeSlice.actions;
export { NARROW_SCREEN_PX };
export default windowSizeSlice.reducer;
