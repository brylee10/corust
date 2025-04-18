import { Box, Typography } from "@mui/material";
import Image from "next/image";

export default function WaitingForSession() {
  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        height: "100vh",
      }}
    >
      <Typography variant="h6">Connecting to session...</Typography>
      {/* No lazy loading to improve the largest contentful paint*/}
      <Image
        src="/ferris512.png"
        alt="Ferris!"
        width={100}
        height={100}
        priority
      />
    </Box>
  );
}
