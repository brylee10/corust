import { UserInner } from "corust-components";
import React, { useCallback, useEffect } from "react";
import UserIconList from "./userIconList";
import {
  Alert,
  Box,
  Button,
  Grow,
  IconButton,
  Snackbar,
  Stack,
  Tooltip,
  styled,
  useTheme,
} from "@mui/material";
import LightModeIcon from "@mui/icons-material/LightMode";
import DarkModeIcon from "@mui/icons-material/DarkMode";
import PeopleIcon from "@mui/icons-material/People";
import { UserState } from "@/store/slices/userSlice";
import { useDispatch, useSelector } from "react-redux";
import { RootState } from "@/store/store";
import { toggleDarkMode } from "@/store/slices/display";
import GitHubIcon from "@mui/icons-material/GitHub";

// Define constants once
const CustomButton = styled(Button)(({ theme }) => {
  const isDarkMode = theme.palette.mode === "dark";
  // Custom darker hover color
  const lmHoverBackgroundColor = "#00000044";
  const dmHoverBackgroundColor = "#FFFFFF22";
  const hoverBackgroundColor = isDarkMode
    ? dmHoverBackgroundColor
    : lmHoverBackgroundColor;
  return {
    padding: "10px 20px",
    // Rust!
    backgroundColor: theme.palette.background.default,
    // Brighter in dark mode
    color: theme.palette.primary.main,
    borderRadius: "5px",
    cursor: "pointer",
    fontWeight: "bold",
    lineHeight: "1.25",
    height: 38,
    border: `2px solid ${theme.palette.primary.main}`,
    "&:hover": {
      backgroundColor: hoverBackgroundColor,
    },
  };
});

interface HeaderBarProps {
  RunButton: React.ReactNode;
  RunConfigButtons: React.ReactNode;
  userArr: UserInner[];
  currUser: UserState;
}

function HeaderBar({
  RunButton,
  RunConfigButtons,
  userArr,
  currUser,
}: HeaderBarProps) {
  const dispatch = useDispatch();
  const isDarkMode = useSelector(
    (state: RootState) => state.displaySelector.dark
  );
  const theme = useTheme();
  const [openCopyNotification, setOpenCopyNotification] = React.useState(false);
  const [buttonTheme, setButtonTheme] = React.useState({
    color: theme.palette.grey[200],
  });

  useEffect(() => {
    const darkButtonTheme = {
      color: theme.palette.grey[200],
    };
    const lightButtonTheme = {
      color: theme.palette.grey[800],
    };
    const buttonTheme = isDarkMode ? darkButtonTheme : lightButtonTheme;

    setButtonTheme(buttonTheme);
  }, [isDarkMode, theme]);

  const copyCorustLink = useCallback(() => {
    navigator.clipboard.writeText(window.location.href);
    setOpenCopyNotification(true);
  }, []);

  const renderThemeToggle = useCallback(() => {
    const toggleIcon = isDarkMode ? <LightModeIcon /> : <DarkModeIcon />;
    const toolTipTitle = isDarkMode ? "Toggle Light" : "Toggle Dark";

    return (
      <Tooltip title={toolTipTitle} arrow>
        <IconButton onClick={() => dispatch(toggleDarkMode())} sx={buttonTheme}>
          {toggleIcon}
        </IconButton>
      </Tooltip>
    );
  }, [isDarkMode, dispatch, buttonTheme]);

  const renderGithubButton = useCallback(() => {
    return (
      <Tooltip title="View the Code" arrow>
        <IconButton
          component="a"
          href="https://github.com/brylee10/corust"
          target="_blank"
          rel="noopener noreferrer"
          sx={buttonTheme}
        >
          <GitHubIcon />
        </IconButton>
      </Tooltip>
    );
  }, [buttonTheme]);

  return (
    <>
      <Box className="header-bar">
        <Stack direction="row" spacing={2} sx={{ alignItems: "center" }}>
          {RunButton}
          {RunConfigButtons}
        </Stack>
        <Box
          id="header-right"
          sx={{
            display: "flex",
            flexGrow: "1",
            justifyContent: "flex-end",
            alignItems: "center",
            gap: theme.spacing(1),
          }}
        >
          <UserIconList userArr={userArr} currUser={currUser} />
          <Tooltip title="Copy link to this Corust session" arrow>
            <CustomButton onClick={copyCorustLink} startIcon={<PeopleIcon />}>
              Share
            </CustomButton>
          </Tooltip>
          {renderThemeToggle()}
          {renderGithubButton()}
        </Box>
      </Box>
      <Snackbar
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
        open={openCopyNotification}
        autoHideDuration={5000}
        slots={{
          transition: Grow,
        }}
        onClose={() => setOpenCopyNotification(false)}
      >
        <Alert severity="success">Copied Corust Link to Clipboard</Alert>
      </Snackbar>
    </>
  );
}

export default HeaderBar;
