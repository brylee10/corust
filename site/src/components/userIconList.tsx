import { UserInner, UserList } from "corust-components/corust_components";
import { useState } from "react";
import UserIcon from "./userIcon";

interface UserIconListProps {
  userArr: UserInner[];
}

// Represents a list of user icons.
function UserIconList({ userArr }: UserIconListProps) {
  const styles = {
    container: {
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      paddingTop: "10px",
      paddingLeft: "20px",
      paddingRight: "20px",
    },
  };

  const generateUserIcon = (user: UserInner) => {
    return (
      <UserIcon
        key={user.username()}
        name={user.username()}
        color={user.color()}
      />
    );
  };

  return (
    <div style={styles.container}>
      {userArr.map((user) => generateUserIcon(user))}
    </div>
  );
}

export default UserIconList;
