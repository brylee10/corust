/**
 *  Records whether the window is a narrow screen or not.
 *  A narrow screen is defined as a screen with a width of `NARROW_SCREEN_PX` or less.
 */

import { createSlice } from "@reduxjs/toolkit";

const NARROW_SCREEN_PX = 1600;

const displaySlice = createSlice({
  name: "display",
  initialState: {
    isNarrowScreen: false,
    dark: false,
  },
  reducers: {
    setNarrowScreen: (state, action) => {
      state.isNarrowScreen = action.payload;
    },
    setDarkMode: (state) => {
      state.dark = true;
    },
    setLightMode: (state) => {
      state.dark = false;
    },
    toggleDarkMode: (state) => {
      state.dark = !state.dark;
    },
  },
});

export const { setNarrowScreen, setDarkMode, setLightMode, toggleDarkMode } =
  displaySlice.actions;
export { NARROW_SCREEN_PX };
export default displaySlice.reducer;
