/**
 * Selects the Cargo command to run.
 * Command can be Build, Run, or Test.
 */

import { createSlice } from "@reduxjs/toolkit";

enum CargoCommand {
  Build = "Build",
  Run = "Run",
  Test = "Test",
}

const cargoCommandSlice = createSlice({
  name: "cargoCommand",
  initialState: {
    command: CargoCommand.Build,
  },
  reducers: {
    setCargoCommand: (state, action) => {
      state.command = action.payload;
    },
  },
});

export { CargoCommand };
export const { setCargoCommand } = cargoCommandSlice.actions;
export default cargoCommandSlice.reducer;
