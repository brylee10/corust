import { UserInner } from "../../../../rust/components/pkg/corust_components";
import { useEffect, useState } from "react";
import UserIcon from "./userIcon";
import { Box } from "@mui/material";
import { UserState } from "../../store/slices/userSlice";

interface UserIconListProps {
  userArr: UserInner[];
  // The user state of the current user
  currUser: UserState;
}

// Represents a list of user icons.
function UserIconList({ userArr, currUser }: UserIconListProps) {
  const [userArrSorted, setUserArrSorted] = useState<UserInner[]>(userArr);

  const styles = {
    container: {
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
    },
  };

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

  useEffect(() => {
    // Sort the user array
    // 1. Self user should be at the end
    // 2. Others sort by increasing user_id
    const sortedArr = [...userArr].sort((a, b) => {
      if (a.user_id() === currUser.userId) return 1;
      if (b.user_id() === currUser.userId) return -1;
      return a.user_id() < b.user_id() ? -1 : 1;
    });

    setUserArrSorted(sortedArr);
  }, [userArr, currUser]);

  return (
    <Box style={styles.container}>
      {userArrSorted.map((user) => generateUserIcon(user))}
    </Box>
  );
}

export default UserIconList;
