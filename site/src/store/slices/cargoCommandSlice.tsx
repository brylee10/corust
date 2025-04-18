/**
 * Selects the Cargo command to run.
 * Command can be Build, Run, or Test.
 */

import { createSlice, PayloadAction } from "@reduxjs/toolkit";

enum CargoCommand {
  Build = "Build",
  Run = "Run",
  Test = "Test",
}

interface CargoCommandState {
  command: CargoCommand;
  // Whether the default command was overridden by a user or collaborator
  // Otherwise, use heuristics to select sensible default command
  // (build for lib, run for bin)
  defaultOverridden: boolean;
}

const cargoCommandSlice = createSlice({
  name: "cargoCommand",
  initialState: {
    command: CargoCommand.Run,
    defaultOverridden: false,
  } as CargoCommandState,
  reducers: {
    setCargoCommand: (state, action: PayloadAction<CargoCommand>) => {
      state.command = action.payload;
    },
    setDefaultCommandOverridden: (state) => {
      state.defaultOverridden = true;
    },
  },
});

export { CargoCommand };
export const { setCargoCommand, setDefaultCommandOverridden } =
  cargoCommandSlice.actions;
export type { CargoCommandState };
export default cargoCommandSlice.reducer;
