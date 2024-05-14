import { useCallback, useEffect, useState } from "react";
import App from "../App";
import * as wasm from "corust-components/corust_components.js";
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
        const response = await fetch(
          `http://127.0.0.1:8000/join/${params.sessionId}`,
          {
            method: "POST",
            headers: headers,
          }
        );
        const text = await response.text();
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
        setUserId(userJoinResponse.user_id);
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

  return userId ? <App userId={userId} /> : <div>Loading...</div>;
}

export default UserJoin;
