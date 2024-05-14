use corust_components::network::{UserId, UserList};
use futures_util::stream::{SplitSink, StreamExt};
use futures_util::SinkExt;

use crate::messages::*;
use crate::sessions::{SessionId, SharedSessionMap};
use corust_components::{network::RemoteUpdate, BroadcastLocalDocUpdate, ServerMessage, Snapshot};
use tokio::sync::broadcast::error::RecvError;
use tokio::time::Duration;
use warp::{
    filters::ws::{Message, WebSocket},
    Filter,
};

// Frequency to send pings to each client, in seconds
const PING_INTERVAL_SEC: u64 = 10;
// Users which have not responded to pings within this time will be marked as inactive
const REMOVE_INACTIVE_USER_SEC: u64 = 30;
// Frequency to check for client inactivity, in seconds
// Note that inactive user check for each connection will check all users for inactivity
// If a connection did not end gracefully then the caller itself was unable to remove itself
const CHECK_INACTIVE_USERS_SEC: u64 = 30;

pub(crate) async fn handle_websocket(
    websocket: WebSocket,
    session_map: SharedSessionMap,
    session_id: SessionId,
    user_id: UserId,
) {
    // unwrap: session_id is always valid, the user gets or creates a session on join, and the user joins
    // before connecting to the websocket
    let session = session_map.lock().await.get_session(&session_id).unwrap();
    let (bcast_tx, server) = {
        let session = session.lock().await;
        (session.bcast_tx(), session.server())
    };
    let mut bcast_rx = bcast_tx.subscribe();
    let (mut ws_tx, mut ws_rx) = websocket.split();

    // Sync the late joiner with the current server doc state
    send_snapshot(server.clone(), &mut ws_tx).await;

    // User Update 1: On join, broadcast new user list
    {
        let server = server.lock().await;
        let user_list = UserList::new(server.users().values().cloned());
        let msg = ServerMessage::UserList(user_list);

        if let Err(e) = bcast_tx.send(msg) {
            // Not an error, just means all receiver handles have been closed
            log::info!("All receiver handles have been closed. {e:?}");
            // Handle error (e.g., all receiver handles have been closed)
            return;
        }
    }

    let mut ping_timer = tokio::time::interval(Duration::from_secs(PING_INTERVAL_SEC));
    let mut check_inactive_users =
        tokio::time::interval(Duration::from_secs(CHECK_INACTIVE_USERS_SEC));

    // Spawn a task to receive messages
    tokio::task::spawn(async move {
        loop {
            tokio::select! {
                next = ws_rx.next() => {
                    // handle client ws messages, broadcast to others
                    match next {
                        Some(msg) => match msg {
                            Ok(msg) => {
                                if msg.is_text() {
                                    let mut server = server.lock().await;
                                    // Convert network serialized method into native struct
                                    // to_str() is always valid because msg `is_text`
                                    // TODO: Replace this with `RemoteUpdate` for consistency
                                    let msg = msg.to_str().unwrap();
                                    log::debug!("Received raw message from client: {msg:?}");
                                    let msg: BroadcastLocalDocUpdate = serde_json::from_str(msg).unwrap();

                                    let res = server.apply_client_operation(msg.text_operation().clone(), msg.last_server_state_id(), msg.cursor_map(), msg.user_id());
                                    if let Err(e) = &res {
                                        log::error!("Error applying client operation to server: {e:?}");
                                    }
                                    let (text_op, cursor_map) = res.unwrap();

                                    log::debug!("Current server document: {}", server.current_document_state().document());
                                    debug_assert!(&cursor_map == server.current_document_state().cursor_map());

                                    let remote_update = RemoteUpdate {
                                        // Todo, replace with real IDs
                                        source: msg.user_id(),
                                        dest: 0,
                                        state_id: server.current_state_id(),
                                        operation: text_op,
                                        cursor_map,
                                    };
                                    let msg = ServerMessage::RemoteUpdate(remote_update);

                                    // Broadcast the message to other clients
                                    if let Err(e) = bcast_tx.send(msg) {
                                        // Not an error, just means all receiver handles have been closed
                                        log::info!(
                                            "All receiver handles have been closed. {e:?}"
                                        );
                                        // Handle error (e.g., all receiver handles have been closed)
                                        return;
                                    }
                                } else if msg.is_pong() {
                                    let activity_time = std::time::Instant::now();
                                    log::debug!("Received pong from client {user_id} in session ID {session_id} at time {activity_time:?}");
                                    let mut server = server.lock().await;
                                    // Update the user's last activity time
                                    match server.users_mut().get_mut(&user_id) {
                                        Some(user) => {
                                            user.activity.last_activity = activity_time;
                                        }
                                        // This should never occur, the user_id is created on client join
                                        None => panic!("Received pong for user ID {user_id} which does not exist in session ID {session_id} user map"),
                                    }
                                } else if msg.is_close() {
                                    log::info!("Received graceful close message from client {user_id} in session ID {session_id}, removing user");
                                    let mut server = server.lock().await;
                                    // Remove user from session on disconnect
                                    match server.users_mut().remove(&user_id) {
                                        Some(_) => {
                                            // Expected, user is present
                                        }
                                        // This should never occur, the user_id is created on client join
                                        None => panic!("Received close user ID {user_id} which does not exist in session ID {session_id} user map"),
                                    }
                                    // User Update 2: On leave, broadcast user list
                                    let user_list = UserList::new(server.users().values().cloned());
                                    let msg = ServerMessage::UserList(user_list);

                                    // Broadcast the message to other clients
                                    if let Err(e) = bcast_tx.send(msg) {
                                        // Not an error, just means all receiver handles have been closed
                                        log::info!(
                                            "All receiver handles have been closed. {e:?}"
                                        );
                                        // Handle error (e.g., all receiver handles have been closed)
                                        return;
                                    }

                                    return;
                                }
                            }
                            Err(e) => {
                                // Handle error (e.g., parse error). Log error but continue connection.
                                log::error!("Error parsing received message on ws, {e:?}");
                            }
                        },
                        // Connection closed
                        // Close frame should be received before this point and exit early, so this typically will not occur
                        None => {
                            log::info!("User ID {user_id} in session ID {session_id} gracefully closed ws connection");
                            return
                        },
                    }
                }
                msg = bcast_rx.recv() => {
                    // Receive broadcast messages, forward to client
                    match msg {
                        Ok(msg) => {
                            let msg: String = serde_json::ser::to_string(&msg).unwrap_or_else(|e| panic!("Error serializing string {msg:?}, error {e}"));
                            let msg: Message = Message::text(msg);
                            log::debug!("Sending message to clients: {msg:?}");
                            if let Err(e) = ws_tx.send(msg).await {
                                log::info!("Sending error, all receiver handles have been closed. {e:?}");
                                // Handle error (e.g., all receiver handles have been closed)
                                return;
                            }
                        }
                        Err(e) => {
                            // Either all send handlers are gone, or the receiver has lagged
                            match e {
                                RecvError::Closed => {
                                    log::info!("All senders have been dropped, receiver closing");
                                    return;
                                }
                                RecvError::Lagged(msg_cnt) => {
                                    log::error!("Receiver has lagged {msg_cnt} messages")
                                }
                            }
                        }
                    }
                }
                _ = ping_timer.tick() => {
                    log::debug!("Sending ping to user ID {user_id} in session ID {session_id}");
                    if let Err(e) = ws_tx.send(Message::ping(Vec::new())).await {
                        log::error!("Failed to send ping: {e}");
                        return;  // Exit the task if the websocket is closed or an error occurs
                    }

                    // User Update 3: Broadcast periodically in case non-gracefully disconnected users are pruned
                    let server = server.lock().await;
                    let user_list = UserList::new(server.users().values().cloned());
                    let msg = ServerMessage::UserList(user_list);
                    let msg = serde_json::ser::to_string(&msg).unwrap();
                    let msg: Message = Message::text(msg);
                    if let Err(e) = ws_tx.send(msg).await {
                        log::error!("Failed to send ping: {e}");
                        return;  // Exit the task if the websocket is closed or an error occurs
                    }
                },
                _ = check_inactive_users.tick() => {
                    log::debug!("Checking if users in session ID {session_id} are inactive");
                    let mut server = server.lock().await;

                    log::debug!("Users present in session ID {session_id}: {:?}", server.users());

                    let mut users_to_remove = Vec::new();

                    for (id, user) in server.users() {
                        let user_last_activity = user.activity.last_activity;
                        if user_last_activity.elapsed().as_secs() > REMOVE_INACTIVE_USER_SEC {
                            log::debug!("User {user:?} in session ID {session_id} is inactive, marking for removal");
                            users_to_remove.push(*id);
                        }
                    }

                    for id in users_to_remove.iter() {
                        let user = server.users_mut().remove(id).unwrap();
                        log::debug!("Removing inactive user {user:?} from session ID {session_id}");
                    }

                    if users_to_remove.contains(&user_id) {
                        // tx close would initiate close handshake with client, but
                        match ws_tx.close().await {
                            Ok(_) => {
                                log::debug!("Closed websocket for inactive current user {user_id} from session ID {session_id}");
                            }
                            Err(e) => {
                                log::error!("Failed to close websocket for current user {user_id} from session ID {session_id}: {e}");
                            }
                        }
                        // Exit since the current user is inactive
                        return;
                    }
                },
            }
        }
    });
}

async fn send_snapshot(server: SharedServer, ws_tx: &mut SplitSink<WebSocket, Message>) {
    let server = server.lock().await;
    let snapshot = Snapshot {
        // ID fields currently not used in live implementation
        source: 0,
        dest: 0,
        document: server.current_document_state().document().to_string(),
        cursor_map: server.current_document_state().cursor_map().clone(),
        state_id: server.current_state_id(),
    };
    // User Update 4: On join, send new user the UserList
    let user_list = UserList::new(server.users().values().cloned());

    // TODO: Check if this is needed. Arbitrary order may work.
    // UserList is broadcast after the Snapshot such that all user cursor positions
    // are present before the client user list is updated
    let messages = [
        ServerMessage::Snapshot(snapshot),
        ServerMessage::UserList(user_list),
    ];

    for msg in messages {
        let msg = serde_json::ser::to_string(&msg).unwrap();
        let msg: Message = Message::text(msg);
        log::debug!("Send snapshot to new client: {msg:?}");
        if let Err(e) = ws_tx.send(msg).await {
            log::info!("Sending error, all receiver handles have been closed. {e:?}");
            // Handle error (e.g., all receiver handles have been closed)
        }
    }
}

pub(crate) fn websocket_route(
    session_map: SharedSessionMap,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path("websocket")
        .and(warp::path::param())
        .and(warp::path::param())
        .and(warp::ws())
        .map(
            move |session_id: String, user_id: UserId, ws: warp::ws::Ws| {
                let session_map = session_map.clone();
                ws.on_upgrade(move |ws| {
                    handle_websocket(ws, session_map.clone(), session_id, user_id)
                })
            },
        )
}
