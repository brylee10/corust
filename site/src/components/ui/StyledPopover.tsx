import React, { useState, ReactNode, useEffect } from "react";
import { Popover, Grow, useTheme } from "@mui/material";
import { styled } from "@mui/material/styles";
import { useSelector } from "react-redux";
import { RootState } from "@/store/store";

// Styled arrow component that appears above the popover
const Arrow = styled("div")(({ theme, ...props }) => {
  const isDarkMode = theme.palette.mode === "dark";
  const backgroundColor = isDarkMode ? theme.palette.grey[800] : "white";
  return {
    ...props,
    width: 0,
    height: 0,
    borderLeft: "1rem solid transparent",
    borderRight: "1rem solid transparent",
    borderBottom: `1rem solid ${backgroundColor}`,
    position: "absolute",
    top: "-8px",
    left: "50%",
  };
});

interface StyledPopoverProps {
  id: string;
  open: boolean;
  anchorEl: HTMLElement | null;
  onClose: () => void;
  children: ReactNode;
}

const StyledPopover = ({
  id,
  open,
  anchorEl,
  onClose,
  children,
}: StyledPopoverProps) => {
  const isDarkMode = useSelector(
    (state: RootState) => state.displaySelector.dark
  );
  const theme = useTheme();
  const [arrowPosition, setArrowPosition] = useState<{
    top: number;
    left: number;
  } | null>(null);
  const [backgroundColor, setBackgroundColor] = useState<string>(
    theme.palette.grey[50]
  );

  useEffect(() => {
    if (anchorEl && open) {
      // Calculate position of the anchor element
      const rect = anchorEl.getBoundingClientRect();
      setArrowPosition({
        top: rect.bottom + window.scrollY, // Position arrow just below the anchor
        left: rect.left + rect.width / 2, // Center the arrow horizontally on the anchor
      });
    } else {
      setArrowPosition(null); // Hide arrow when Popover is closed or anchor is unavailable
    }
  }, [anchorEl, open]);

  useEffect(() => {
    setBackgroundColor(isDarkMode ? theme.palette.grey[800] : "white");
  }, [isDarkMode, theme]);

  return (
    <>
      {/* Conditionally render the Arrow below the anchorEl */}
      {arrowPosition && (
        <Grow
          in={open}
          timeout={300}
          style={{
            transformOrigin: "top center",
            transform: "translateX(-50%)",
          }}
        >
          <Arrow
            className="arrowPopover"
            style={{
              top: arrowPosition.top,
              left: arrowPosition.left,
              // Do not inherit any margin from parents
              margin: "0",
            }}
          />
        </Grow>
      )}

      <Popover
        id={id}
        open={open}
        anchorEl={anchorEl}
        onClose={onClose}
        anchorOrigin={{
          vertical: "bottom",
          horizontal: "center",
        }}
        transformOrigin={{
          vertical: "top",
          horizontal: "center",
        }}
        elevation={1}
        slotProps={{
          paper: {
            sx: {
              mt: "0.75rem",
              overflow: "visible",
              backgroundColor: backgroundColor,
            },
          },
        }}
      >
        {children}
      </Popover>
    </>
  );
};

export default StyledPopover;
