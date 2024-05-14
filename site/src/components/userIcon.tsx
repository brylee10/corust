import { Tooltip } from "@mui/material";

interface UserIconProps {
  key: string;
  name: string;
  color: string;
}

// Represents a single user on the document.
function UserIcon({ name, color }: UserIconProps) {
  const styles = {
    image: {
      width: 40,
      borderRadius: "50%",
      height: 40,
      marginRight: 5,
      backgroundColor: color,
      // Centers text vertically and horizontally
      lineHeight: "40px",
      textAlign: "center" as const, // don't widen to a string
      fontWeight: "bold",
      color: "whitesmoke",
    },
  };

  return (
    <Tooltip title={name} placement="bottom">
      <div key={name} style={styles.image}>
        {name.charAt(0)}
      </div>
    </Tooltip>
  );
}

export default UserIcon;
