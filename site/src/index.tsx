import React from "react";
import ReactDOM from "react-dom/client";
import "./index.css";
import reportWebVitals from "./reportWebVitals.js";
import { Provider } from "react-redux";
import store from "./store/store.tsx";
import App from "./App.tsx";

// Do not log INFO or DEBUG messages in production
if (process.env.REACT_APP_ENVIRONMENT?.toLowerCase() === "production") {
  console.log = () => {};
  console.debug = () => {};
}

// `root` is always present in `index.html`
const element = document.getElementById("root") as HTMLElement;

const root = ReactDOM.createRoot(element);
root.render(
  //   <React.StrictMode>
  <Provider store={store}>
    <App />
  </Provider>
  //   </React.StrictMode>
);

// If you want to start measuring performance in your app, pass a function
// to log results (for example: reportWebVitals(console.log))
// or send to an analytics endpoint. Learn more: https://bit.ly/CRA-vitals
reportWebVitals();
