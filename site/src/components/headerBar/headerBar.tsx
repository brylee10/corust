import { UserInner } from "corust-components";
import React, { useCallback } from "react";
import UserIconList from "./userIconList";
import {
  Alert,
  Box,
  Button,
  Grow,
  Snackbar,
  Stack,
  Tooltip,
  styled,
} from "@mui/material";
import PeopleIcon from "@mui/icons-material/People";
import { UserState } from "../../store/slices/userSlice";

// Define constants once
const CustomButton = styled(Button)({
  padding: "10px 20px",
  // Rust!
  backgroundColor: "white",
  color: "#CE412B",
  borderRadius: "5px",
  cursor: "pointer",
  fontWeight: "bold",
  lineHeight: "1.25",
  height: 38,
  border: "2px solid #CE412B",
  "&:hover": {
    backgroundColor: "white",
  },
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
  const [openCopyNotification, setOpenCopyNotification] = React.useState(false);

  const copyCorustLink = useCallback(() => {
    navigator.clipboard.writeText(window.location.href);
    setOpenCopyNotification(true);
  }, []);

  return (
    <>
      <Box className="header-bar">
        <Stack direction="row" spacing={2} sx={{ alignItems: "center" }}>
          {RunButton}
          {RunConfigButtons}
        </Stack>
        <Box className="header-right">
          <UserIconList userArr={userArr} currUser={currUser} />
          <Tooltip title="Copy Corust Link">
            <CustomButton onClick={copyCorustLink} startIcon={<PeopleIcon />}>
              Share
            </CustomButton>
          </Tooltip>
        </Box>
      </Box>
      <Snackbar
        anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
        open={openCopyNotification}
        autoHideDuration={5000}
        TransitionComponent={Grow}
        onClose={() => setOpenCopyNotification(false)}
      >
        <Alert severity="success">Copied Corust Link to Clipboard</Alert>
      </Snackbar>
    </>
  );
}

export default HeaderBar;
