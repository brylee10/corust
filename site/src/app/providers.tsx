// src/app/providers.tsx
"use client";

import { createTheme, GlobalStyles, ThemeProvider } from "@mui/material";
import { useSelector } from "react-redux";
import { Provider } from "react-redux";
import { RootState, store } from "@/store/store";
import { useState, useEffect } from "react";

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

export function Providers({ children }: { children: React.ReactNode }) {
  // This needs to be wrapped in a client component
  return (
    <Provider store={store}>
      <ThemeHandler>{children}</ThemeHandler>
    </Provider>
  );
}

function ThemeHandler({ children }: { children: React.ReactNode }) {
  const isDarkMode = useSelector(
    (state: RootState) => state.displaySelector.dark
  );

  const [theme, setTheme] = useState(isDarkMode ? darkTheme : lightTheme);

  useEffect(() => {
    setTheme(isDarkMode ? darkTheme : lightTheme);
  }, [isDarkMode]);

  return (
    <ThemeProvider theme={theme}>
      <GlobalStyles
        styles={{
          "*": {
            transition: "background-color 0.3s ease, color 0.3s ease",
          },
        }}
      />
      {children}
    </ThemeProvider>
  );
}
