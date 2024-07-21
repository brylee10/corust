import { UserInner } from "corust-components";
import React, { useCallback } from "react";
import UserIconList from "./userIconList";
import { Alert, Button, Grow, Snackbar, Tooltip, styled } from "@mui/material";
import PeopleIcon from "@mui/icons-material/People";

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
  renderRunButton: () => React.ReactNode;
  userArr: UserInner[];
  selfUserId: bigint;
}

function HeaderBar({ renderRunButton, userArr, selfUserId }: HeaderBarProps) {
  const [openCopyNotification, setOpenCopyNotification] = React.useState(false);

  const copyCorustLink = useCallback(() => {
    navigator.clipboard.writeText(window.location.href);
    setOpenCopyNotification(true);
  }, []);

  return (
    <>
      <div className="header-bar">
        {renderRunButton()}
        <div className="header-left">
          <UserIconList userArr={userArr} selfUserId={selfUserId} />
          <Tooltip title="Copy Corust Link">
            <CustomButton onClick={copyCorustLink} startIcon={<PeopleIcon />}>
              Share
            </CustomButton>
          </Tooltip>
        </div>
      </div>
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
