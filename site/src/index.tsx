import React from "react";
import ReactDOM from "react-dom/client";
import "./index.css";
import reportWebVitals from "./reportWebVitals.js";
import { BrowserRouter, Route, Routes } from "react-router-dom";
import { SessionHandler } from "./components/sessionHandler.tsx";
import UserJoin from "./components/userJoin.tsx";
import { createTheme, ThemeProvider } from "@mui/material";

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
      main: "#CE412B",
    },
    secondary: {
      main: "#F5EEE3",
    },
  },
});

const root = ReactDOM.createRoot(element);
root.render(
  <React.StrictMode>
    <ThemeProvider theme={theme}>
      <BrowserRouter>
        <SessionHandler>
          <Routes>
            <Route path="/:sessionId" element={<UserJoin />} />
          </Routes>
        </SessionHandler>
      </BrowserRouter>
    </ThemeProvider>
  </React.StrictMode>
);

// If you want to start measuring performance in your app, pass a function
// to log results (for example: reportWebVitals(console.log))
// or send to an analytics endpoint. Learn more: https://bit.ly/CRA-vitals
reportWebVitals();
