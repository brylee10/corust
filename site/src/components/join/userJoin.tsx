"use client";
import { useEffect } from "react";
import MainPage from "@/components/mainPage";
import { useParams } from "next/navigation";
import { Box } from "@mui/material";
import { useDispatch, useSelector } from "react-redux";
import {
  selectUserState,
  setUserState,
  UserStateDefined,
} from "@/store/slices/userSlice";
import { clientJoin } from "@/api/userJoin";

function UserJoin() {
  const dispatch = useDispatch();
  const userState = useSelector(selectUserState);
  // Route params
  const params = useParams();

  // Initialize the `Client` when the component mounts.
  useEffect(() => {
    console.debug("Requesting client join");

    async function joinSession() {
      try {
        const userJoinResponse = await clientJoin(params.sessionId as string);

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
        console.error("Failed to join session:", error);
      }
    }

    joinSession();
  }, [dispatch, params.sessionId]);

  // Explicitly check for equivalence to `undefined` otherwise `userId = 0` is falsey as well
  // one of the few times React StrictMode hid a bug!
  return userState.userId !== undefined && userState.username !== undefined ? (
    <MainPage currUser={userState as UserStateDefined} />
  ) : (
    <Box>Failed to connect to server</Box>
  );
}

export default UserJoin;
