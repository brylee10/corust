import React from "react";
import ReactDOM from "react-dom/client";
import "./index.css";
import reportWebVitals from "./reportWebVitals";
import { BrowserRouter, Route, Routes } from "react-router-dom";
import { SessionHandler } from "./components/sessionHandler.tsx";
import UserJoin from "./components/userJoin.tsx";

// `root` is always present in `index.html`
const element = document.getElementById("root") as HTMLElement;

const root = ReactDOM.createRoot(element);
root.render(
  <React.StrictMode>
    <BrowserRouter>
      <SessionHandler>
        <Routes>
          <Route path="/:sessionId" element={<UserJoin />} />
        </Routes>
      </SessionHandler>
    </BrowserRouter>
  </React.StrictMode>
);

// If you want to start measuring performance in your app, pass a function
// to log results (for example: reportWebVitals(console.log))
// or send to an analytics endpoint. Learn more: https://bit.ly/CRA-vitals
reportWebVitals();
