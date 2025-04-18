/**
 * API call to join a user to a session
 */

interface UserJoinResponse {
  // Rust server field names are in snake case
  user_id: bigint;
  username: string;
}

export const clientJoin = async (
  sessionId: string
): Promise<UserJoinResponse> => {
  const headers = new Headers();
  headers.append("Content-Type", "application/json");

  try {
    // Get the user id for the session, if it exists
    const userId = sessionStorage.getItem("sessionUserId_" + sessionId);
    let fetchUri;
    if (userId) {
      fetchUri = `${process.env.NEXT_PUBLIC_ENDPOINT_URI}/join/${sessionId}/${userId}`;
    } else {
      fetchUri = `${process.env.NEXT_PUBLIC_ENDPOINT_URI}/join/${sessionId}`;
    }

    const response = await fetch(fetchUri, {
      method: "POST",
      headers: headers,
    });
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
    return userJoinResponse;
  } catch (error) {
    throw error;
  }
};
