"use client";

import { useEffect } from "react";
import { useRouter, usePathname } from "next/navigation";

// Takes only the floating point part of the random number and converts it to base 36, representing digits 0-9 and letters a-z
const generateSessionId = (length: number = 10) =>
  Math.random()
    .toString(36)
    .substring(2, length + 2);

// Redirects the user to a new session, used for the home page
function NewSessionHandler() {
  const router = useRouter();
  const pathname = usePathname();

  useEffect(() => {
    // Only redirect to a stored or new session if on home page, otherwise the user has already specified a session
    if (pathname !== "/") {
      return;
    }

    const sessionId = generateSessionId();

    router.push(`/${sessionId}`);
  }, [router, pathname]);

  return <></>;
}

export { NewSessionHandler };
