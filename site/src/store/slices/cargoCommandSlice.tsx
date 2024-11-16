/**
 * Selects the Cargo command to run.
 * Command can be Build, Run, or Test.
 */

import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { set } from "lodash";

enum CargoCommand {
  Build = "Build",
  Run = "Run",
  Test = "Test",
}

interface CargoCommandState {
  command: CargoCommand;
  lastExecuteCommand: CargoCommand | null;
}

const cargoCommandSlice = createSlice({
  name: "cargoCommand",
  initialState: {
    command: CargoCommand.Run,
    lastExecuteCommand: null,
  } as CargoCommandState,
  reducers: {
    setCargoCommand: (state, action: PayloadAction<CargoCommand>) => {
      state.command = action.payload;
    },
    setLastExecuteCargoCommand(state, action: PayloadAction<CargoCommand>) {
      state.lastExecuteCommand = action.payload;
    },
  },
});

export { CargoCommand };
export const { setCargoCommand, setLastExecuteCargoCommand } =
  cargoCommandSlice.actions;
export type { CargoCommandState };
export default cargoCommandSlice.reducer;
