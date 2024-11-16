/**
 *  Holds metadata on the current user.
 */

import { createSelector, createSlice, PayloadAction } from "@reduxjs/toolkit";
import { RootState } from "../store";

// `UserState` type the user provides and is returned to the user
interface UserState {
  userId: bigint | undefined;
  username: string | undefined;
}

// Equivalent to `UserState` except all fields are defined
interface UserStateDefined {
  userId: bigint;
  username: string;
}

// `UserState` type Redux stores
interface UserStateRedux {
  userId: string | undefined;
  username: string | undefined;
}

const userSlice = createSlice({
  name: "user",
  initialState: {
    userId: undefined,
    username: undefined,
  } as UserStateRedux,
  reducers: {
    setUserState(state, action: PayloadAction<UserStateDefined>) {
      // BigInt is not serializable by Redux, store as string
      state.userId = action.payload.userId.toString();
      state.username = action.payload.username;
    },
  },
});

const rawUserStateSelector = (state: RootState) => state.userSelector;

// Convert `user_id` string back to BigInt
export const selectUserState = createSelector(
  rawUserStateSelector,
  (userSelector) =>
    ({
      userId: userSelector.userId ? BigInt(userSelector.userId) : undefined,
      username: userSelector.username,
    } as UserState)
);

export type { UserState, UserStateDefined };
export const { setUserState } = userSlice.actions;
export default userSlice.reducer;
