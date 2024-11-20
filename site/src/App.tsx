import { createTheme, GlobalStyles, ThemeProvider } from "@mui/material";
import React, { useEffect, useState } from "react";
import { BrowserRouter, Route, Routes } from "react-router-dom";
import { RootState } from "./store/store";
import { useSelector } from "react-redux";
import { NewSessionHandler } from "./components/join/newSessionHandler";
import UserJoin from "./components/join/userJoin";

const lightTheme = createTheme({
  palette: {
    mode: "light",
    primary: {
      // Rust!
      light: "#CEA6A0",
      main: "#CE412B",
      dark: "#902D1E",
    },
    secondary: {
      main: "#F5EEE3",
      dark: "#C7BAA6",
    },
    background: {
      // Whitesmoke, slightly easier on the eyes than white
      default: "#F5F5F5",
    },
  },
  // Default 8px spacing
  spacing: 8,
});

const darkTheme = createTheme({
  palette: {
    mode: "dark",
    primary: {
      light: "#CEA6A0DD",
      main: "#CE412BDD",
      dark: "#902D1EDD",
    },
    secondary: {
      main: "#F5EEE3DD",
      dark: "#C7BAA6DD",
    },
    background: {
      default: "#282c34",
    },
  },
  spacing: 8,
});

function App() {
  const isDarkMode = useSelector(
    (state: RootState) => state.displaySelector.dark
  );

  const [theme, setTheme] = useState(isDarkMode ? darkTheme : lightTheme);

  useEffect(
    function updateTheme() {
      setTheme(isDarkMode ? darkTheme : lightTheme);
    },
    [isDarkMode]
  );

  return (
    <ThemeProvider theme={theme}>
      <GlobalStyles
        // Transition between light and dark modes
        styles={{
          "*": {
            transition: "background-color 0.3s ease, color 0.3s ease",
          },
        }}
      />
      <BrowserRouter>
        <Routes>
          <Route path="/" element={<NewSessionHandler />} />
          <Route path="/:sessionId" element={<UserJoin />} />
        </Routes>
      </BrowserRouter>
    </ThemeProvider>
  );
}

export default App;
