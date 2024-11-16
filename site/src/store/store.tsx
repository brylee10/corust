import { configureStore } from "@reduxjs/toolkit";
import codeSelectorReducer from "./slices/codeSelectorSlice";
import cargoCommandReducer from "./slices/cargoCommandSlice";
import { channelReducer, channelVersionReducer } from "./slices/channelSlice";
import optReducer from "./slices/optSlice";
import windowSizeReducer from "./slices/windowSize";
import userReducer from "./slices/userSlice";

// Redux store
export const store = configureStore({
  reducer: {
    codeSelector: codeSelectorReducer,
    cargoCommandSelector: cargoCommandReducer,
    channelSelector: channelReducer,
    channelVersionSelector: channelVersionReducer,
    optSelector: optReducer,
    windowSize: windowSizeReducer,
    userSelector: userReducer,
  },
});

export default store;

// Type annotations per: https://redux.js.org/tutorials/essentials/part-2-app-structure#creating-the-redux-store
// Infer the type of `store`
export type AppStore = typeof store;
export type RootState = ReturnType<AppStore["getState"]>;
// Infer the `AppDispatch` type from the store itself
export type AppDispatch = AppStore["dispatch"];
