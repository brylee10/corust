import { UserInner } from "../../../../rust/components/pkg/corust_components";
import { useEffect, useMemo, useState } from "react";
import UserIcon from "./userIcon";
import {
  Box,
  Typography,
  List,
  ListItem,
  ListItemAvatar,
  ListItemText,
  Tooltip,
  useTheme,
} from "@mui/material";
import { UserState } from "@/store/slices/userSlice";
import StyledPopover from "@/components/ui/StyledPopover";
import React from "react";

interface UserIconListProps {
  userArr: UserInner[];
  // The user state of the current user
  currUser: UserState;
}

// Represents a list of user icons.
function UserIconList({ userArr, currUser }: UserIconListProps) {
  const theme = useTheme();
  const [visibleUsers, setVisibleUsers] = useState<UserInner[]>([]);
  const [hiddenUsers, setHiddenUsers] = useState<UserInner[]>([]);
  const [anchorEl, setAnchorEl] = useState<HTMLElement | null>(null);
  const open = Boolean(anchorEl);

  // Maximum number of users to display before showing the +N indicator
  const MAX_VISIBLE_USERS = 5;

  const styles = useMemo(() => {
    return {
      container: {
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      },
      popoverContainer: {
        padding: 1,
        maxHeight: 300,
        overflow: "auto",
      },
      extraAvatarContainer: {
        width: 43,
        borderRadius: "50%",
        height: 43,
        marginRight: 1,
        // Centers text vertically and horizontally
        lineHeight: "43px",
        textAlign: "center" as const, // don't widen to a string
        fontWeight: "bold",
        color: "#F5F5F5", // whitesmoke
        cursor: "pointer",
        backgroundColor: theme.palette.secondary.dark,
        "&:hover": {
          backgroundColor: "#6C6C6C",
        },
      },
      overflowedAvatars: {
        width: 43,
        borderRadius: "50%",
        height: 43,
        // Centers text vertically and horizontally
        lineHeight: "43px",
        textAlign: "center" as const, // don't widen to a string
        fontWeight: "bold",
        color: "#F5F5F5", // whitesmoke
      },
    };
  }, [theme]);

  const generateUserIcon = (user: UserInner) => {
    return (
      <UserIcon
        key={user.username()}
        name={user.username()}
        color={user.color()}
        isSelf={user.user_id() === currUser.userId}
      />
    );
  };

  const handlePopoverOpen = (event: React.MouseEvent<HTMLDivElement>) => {
    setAnchorEl(event.currentTarget);
  };

  const handlePopoverClose = () => {
    setAnchorEl(null);
  };

  useEffect(() => {
    // Sort the user array
    // 1. Self user should be at the end
    // 2. Others sort by increasing user_id
    const sortedArr = [...userArr].sort((a, b) => {
      if (a.user_id() === currUser.userId) return 1;
      if (b.user_id() === currUser.userId) return -1;
      return a.user_id() < b.user_id() ? -1 : 1;
    });

    const totalUsers = sortedArr.length;
    setVisibleUsers(sortedArr.slice(-MAX_VISIBLE_USERS, totalUsers));
    setHiddenUsers(
      sortedArr.slice(0, Math.max(0, totalUsers - MAX_VISIBLE_USERS))
    );
  }, [userArr, currUser]);

  // The popover content with all users
  const UserListPopoverContent = () => (
    <List sx={styles.popoverContainer}>
      {hiddenUsers.map((user) => (
        <ListItem key={user.user_id()} sx={{ py: 0.5 }}>
          <ListItemAvatar>
            <Box
              sx={{
                backgroundColor: user.color(),
                ...styles.overflowedAvatars,
              }}
            >
              {user.username().charAt(0)}
            </Box>
          </ListItemAvatar>
          <ListItemText
            primary={
              <Typography variant="body1">
                {user.user_id() === currUser.userId
                  ? `${user.username()} (You)`
                  : user.username()}
              </Typography>
            }
          />
        </ListItem>
      ))}
    </List>
  );

  return (
    <Box style={styles.container}>
      {visibleUsers.map((user) => generateUserIcon(user))}

      {hiddenUsers.length > 0 && (
        <Tooltip
          title={`${hiddenUsers.length} more users`}
          placement="bottom"
          arrow
        >
          <Box onClick={handlePopoverOpen} sx={styles.extraAvatarContainer}>
            +{hiddenUsers.length}
          </Box>
        </Tooltip>
      )}

      <StyledPopover
        id={"user-list-popover"}
        open={open}
        anchorEl={anchorEl}
        onClose={handlePopoverClose}
      >
        <UserListPopoverContent />
      </StyledPopover>
    </Box>
  );
}

export default UserIconList;
