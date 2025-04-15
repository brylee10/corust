import { Box, Tooltip } from "@mui/material";
import { useEffect, useMemo, useState } from "react";
import { darkenRgb } from "../colorUtils";
import { RootState } from "../../store/store";
import { useSelector } from "react-redux";

interface UserIconProps {
  key: string;
  name: string;
  color: string;
  isSelf: boolean;
}

// Represents a single user on the document.
function UserIcon({ name, color, isSelf }: UserIconProps) {
  const isDarkMode = useSelector(
    (state: RootState) => state.displaySelector.dark
  );
  const [userName, setUserName] = useState(name);

  const styles = useMemo(() => {
    const darkRgb = darkenRgb(color);
    const adjustedColor = isDarkMode ? darkRgb : color;
    return {
      collaborator: {
        width: 43,
        borderRadius: "50%",
        height: 43,
        marginRight: 1,
        backgroundColor: adjustedColor,
        // Centers text vertically and horizontally
        lineHeight: "43px",
        textAlign: "center" as const, // don't widen to a string
        fontWeight: "bold",
        color: "#F5F5F5", // whitesmoke
      },
      self: {
        width: 35,
        borderRadius: "50%",
        height: 35,
        marginRight: 1,
        backgroundColor: "transparent",
        // Centers text vertically and horizontally
        lineHeight: "35px",
        textAlign: "center" as const, // don't widen to a string
        fontWeight: "bold",
        color: adjustedColor,
        border: `4px solid ${adjustedColor}`,
      },
    };
  }, [isDarkMode, color]);

  useEffect(() => {
    if (isSelf) {
      setUserName(`${name} (You)`);
    }
  }, [name, isSelf]);

  return (
    <Tooltip title={userName} placement="bottom" arrow>
      <Box key={name} sx={isSelf ? styles.self : styles.collaborator}>
        {name.charAt(0)}
      </Box>
    </Tooltip>
  );
}

export default UserIcon;
