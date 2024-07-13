import { useCallback, useEffect, useState } from "react";
import App from "../App.tsx";
import { useParams } from "react-router-dom";

interface UserJoinResponse {
  // Rust server field names in snake case
  user_id: bigint;
}

function UserJoin() {
  // Route params
  const params = useParams();
  const [userId, setUserId] = useState<bigint | undefined>(undefined);

  const clientJoin = useCallback(
    async () => {
      const headers = new Headers();
      headers.append("Content-Type", "application/json");

      // Will only throw an error if network error encountered
      try {
        console.debug("Sending fetch request");
        // Get the user id for the session, if it exists
        const userId = sessionStorage.getItem(
          "sessionUserId_" + params.sessionId
        );
        let fetchUri;
        if (userId) {
          console.debug("User id found in local storage: ", userId);
          fetchUri = `${process.env.REACT_APP_ENDPOINT_URI}/join/${params.sessionId}/${userId}`;
        } else {
          fetchUri = `${process.env.REACT_APP_ENDPOINT_URI}/join/${params.sessionId}`;
        }

        console.debug("fetchUri: ", fetchUri);
        const response = await fetch(fetchUri, {
          method: "POST",
          headers: headers,
        });
        const text = await response.text();
        console.debug("Fetch Response: ", text);
        const userJoinResponse: UserJoinResponse = JSON.parse(
          text,
          (key, value) => {
            // `response` default is a `number`, but this will always be an integer
            // so cast `user_id` to BigInt
            if (key === "user_id") return BigInt(value);
            return value;
          }
        );

        // Initialize new client
        console.debug("Setting user id to: ", userJoinResponse.user_id);
        setUserId(userJoinResponse.user_id);
        sessionStorage.setItem(
          "sessionUserId_" + params.sessionId,
          userJoinResponse.user_id.toString()
        );
      } catch (error) {
        // TODO: user join failed, add retry or popup notification?
        console.error("Fetch error: ", error);
      }
    },
    // `sessionId` should not change, so this should only load once
    // eslint-disable-next-line react-hooks/exhaustive-deps
    []
  );

  // Initialize the `Client` when the component mounts.
  useEffect(() => {
    console.debug("Requesting client join");
    clientJoin();
  }, [clientJoin]);

  // Explicitly check for equivalence to `undefined` otherwise `userId = 0` is falsey as well
  // one of the few times React StrictMode hid a bug!
  return userId !== undefined ? <App userId={userId} /> : <div>Loading...</div>;
}

export default UserJoin;
