import { useCallback, useEffect } from "react";
import App from "../../App.tsx";
import { useParams } from "react-router-dom";
import { Box } from "@mui/material";
import { useDispatch, useSelector } from "react-redux";
import {
  selectUserState,
  setUserState,
  UserStateDefined,
} from "../../store/slices/userSlice.tsx";

interface UserJoinResponse {
  // Rust server field names in snake case
  user_id: bigint;
  username: string;
}

function UserJoin() {
  const dispatch = useDispatch();
  const userState = useSelector(selectUserState);
  // Route params
  const params = useParams();

  useEffect(() => {
    console.debug("UserState: ", userState);
  }, [userState]);

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
          console.debug("User id found in session storage: ", userId);
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
        dispatch(
          setUserState({
            userId: userJoinResponse.user_id.toString(),
            username: userJoinResponse.username,
          })
        );
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
  return userState.userId !== undefined && userState.username !== undefined ? (
    <App currUser={userState as UserStateDefined} />
  ) : (
    <Box>Failed to connect to server</Box>
  );
}

export default UserJoin;
