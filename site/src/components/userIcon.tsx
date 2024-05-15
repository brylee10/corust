import { Tooltip } from "@mui/material";
import { useEffect, useState } from "react";

interface UserIconProps {
  key: string;
  name: string;
  color: string;
  isSelf: boolean;
}

// Represents a single user on the document.
function UserIcon({ name, color, isSelf }: UserIconProps) {
  const [userName, setUserName] = useState(name);

  const styles = {
    collaborator: {
      width: 35,
      borderRadius: "50%",
      height: 35,
      marginRight: 5,
      backgroundColor: color,
      // Centers text vertically and horizontally
      lineHeight: "35px",
      textAlign: "center" as const, // don't widen to a string
      fontWeight: "bold",
      color: "whitesmoke",
      borderWidth: 4,
      borderColor: color,
      borderStyle: "solid",
    },
    self: {
      width: 35,
      borderRadius: "50%",
      height: 35,
      marginRight: 5,
      backgroundColor: "whitesmoke",
      // Centers text vertically and horizontally
      lineHeight: "35px",
      textAlign: "center" as const, // don't widen to a string
      fontWeight: "bold",
      color: color,
      borderWidth: 4,
      borderColor: color,
      borderStyle: "solid",
    },
  };

  useEffect(() => {
    if (isSelf) {
      setUserName(`${name} (You)`);
    }
  }, [name, isSelf]);

  return (
    <Tooltip title={userName} placement="bottom">
      <div key={name} style={isSelf ? styles.self : styles.collaborator}>
        {name.charAt(0)}
      </div>
    </Tooltip>
  );
}

export default UserIcon;
