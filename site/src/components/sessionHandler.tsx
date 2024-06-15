import { ReactNode, useEffect } from "react";
import { useLocation, useNavigate } from "react-router-dom";

// Takes only the floating point part of the random number and converts it to base 36, representing digits 0-9 and letters a-z
const generateSessionId = (length: number = 10) =>
  Math.random()
    .toString(36)
    .substring(2, length + 2);

interface SessionHandlerProps {
  children: ReactNode;
}

function SessionHandler({ children }: SessionHandlerProps) {
  const navigate = useNavigate();
  const location = useLocation();

  useEffect(() => {
    // Only redirect to a stored or new session if on home page, otherwise the user has already specified a session
    console.log(location.pathname);
    if (location.pathname !== "/") return;
    // let sessionId = localStorage.getItem("sessionId");

    // if (!sessionId) {
    const sessionId = generateSessionId();
    // localStorage.setItem("sessionId", sessionId);
    // }

    navigate(`/${sessionId}`, { replace: true });
  }, [navigate, location.pathname]);

  return children;
}

export { SessionHandler };
