import React from "react";
import ReactDOM from "react-dom/client";
import "./index.css";
import reportWebVitals from "./reportWebVitals.js";
import { BrowserRouter, Route, Routes } from "react-router-dom";
import { NewSessionHandler } from "./components/join/newSessionHandler.tsx";
import UserJoin from "./components/join/userJoin.tsx";
import { createTheme, ThemeProvider } from "@mui/material";
import { Provider } from "react-redux";
import store from "./store/store.tsx";

// Do not log INFO or DEBUG messages in production
if (process.env.REACT_APP_ENVIRONMENT?.toLowerCase() === "production") {
  console.log = () => {};
  console.debug = () => {};
}

// `root` is always present in `index.html`
const element = document.getElementById("root") as HTMLElement;

const theme = createTheme({
  palette: {
    primary: {
      // Rust!
      light: "#C96556",
      main: "#CE412B",
      dark: "#902D1E",
    },
    secondary: {
      main: "#F5EEE3",
      dark: "#ECDDC6",
    },
  },
  // Default 8px spacing
  spacing: 8,
});

const root = ReactDOM.createRoot(element);
root.render(
  <React.StrictMode>
    <Provider store={store}>
      <ThemeProvider theme={theme}>
        <BrowserRouter>
          <Routes>
            <Route path="/" element={<NewSessionHandler />} />
            <Route path="/:sessionId" element={<UserJoin />} />
          </Routes>
        </BrowserRouter>
      </ThemeProvider>
    </Provider>
  </React.StrictMode>
);

// If you want to start measuring performance in your app, pass a function
// to log results (for example: reportWebVitals(console.log))
// or send to an analytics endpoint. Learn more: https://bit.ly/CRA-vitals
reportWebVitals();
