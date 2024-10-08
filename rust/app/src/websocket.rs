use std::sync::Arc;

use corust_components::network::{UserId, UserList};
use corust_components::server::{Server, ServerError};
use corust_components::BroadcastLocalDocUpdate;
use corust_sandbox::container::{ContainerError, ContainerMessage, ExecuteCommand};
use futures_util::stream::{SplitSink, SplitStream, StreamExt};
use futures_util::SinkExt;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, Mutex, MutexGuard};

use crate::execute::runner::{
    bcast_notify_output_size_error, container_response_to_runner_output, run_code,
    ws_notify_concurrent_code_error, RunCodeError, RunType, SharedContainerFactory,
};
use crate::messages::*;
use crate::sessions::{SessionId, SharedSession, SharedSessionMap};
use corust_components::{network::RemoteUpdate, ServerMessage, Snapshot};
use tokio::sync::broadcast::error::{RecvError, SendError};
use tokio::time::Duration;
use warp::{
    filters::ws::{Message, WebSocket},
    Filter,
};

// Frequency to send pings to each client, in seconds
const PING_INTERVAL_SEC: u64 = 10;
// Users which have not responded to pings within this time will be marked as inactive
const MARK_INACTIVE_USER_SEC: u64 = 15;
// 30 minutes - This is the period a user claims the same username before being removed
// Allows users who refresh their page to claim  the same identity
const REMOVE_INACTIVE_USERS_SEC: u64 = 60 * 30;
// Frequency to check for client inactivity, in seconds
// Note that inactive user check for each connection will check all users for inactivity
// If a connection did not end gracefully then the caller itself was unable to remove itself
const CHECK_INACTIVE_USERS_SEC: u64 = 30;
// Number of [`ContainerResponse`] messages that can be bufferred from a running container
// in the channel
const CONTAINER_RESPONSE_MSG_LIMIT: usize = 8;
// Maximum size of a resulting document after transformation.
// In practice, documents are not expected to be this large.
// This is sufficient for ~2k lines of code.
// const MAX_DOC_SIZE_CHARS: usize = 50_000;

pub type SharedWsSender = Arc<Mutex<SplitSink<WebSocket, Message>>>;

// These errors terminate the websocket connection
#[derive(Debug, Error)]
pub enum WebSocketError {
    #[error(transparent)]
    WarpError(#[from] warp::Error),
    #[error(transparent)]
    SendError(#[from] SendError<ServerMessage>),
    #[error(transparent)]
    RecvError(#[from] RecvError),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsClientTextMsg {
    #[serde(rename = "wsDocUpdate")]
    BroadcastDocUpdate(LocalUpdateStringified),
    #[serde(rename = "wsExecuteCommand")]
    Execute(ExecuteCommand),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalUpdateStringified {
    // Rust doc update `BroadcastLocalDocUpdate` serialized
    doc_update: String,
}

pub(crate) async fn handle_websocket(
    websocket: WebSocket,
    session_map: SharedSessionMap,
    session_id: SessionId,
    user_id: UserId,
    container_factory: SharedContainerFactory,
) {
    // unwrap: session_id is always valid, the user gets or creates a session on join, and the user joins
    // before connecting to the websocket
    let session = session_map.lock().await.get_session(&session_id).unwrap();
    let (bcast_tx, server) = {
        let session = session.lock().await;
        (session.bcast_tx(), session.server())
    };
    let bcast_rx = bcast_tx.subscribe();
    let (mut ws_tx, ws_rx) = websocket.split();

    // Sync the late joiner with the current server doc state
    send_snapshot(server.clone(), &mut ws_tx).await;

    // User Update 1: On join, broadcast new user list
    {
        let server = server.lock().await;
        if let Err(_) = broadcast_user_list(bcast_tx.clone(), &server).await {
            return;
        }
    }

    let shared_ws_tx = Arc::new(Mutex::new(ws_tx));
    let ping_timer = tokio::time::interval(Duration::from_secs(PING_INTERVAL_SEC));
    let check_inactive_users = tokio::time::interval(Duration::from_secs(CHECK_INACTIVE_USERS_SEC));

    // Spawn a task to receive messages
    tokio::task::spawn(handle_messages(
        session.clone(),
        server.clone(),
        bcast_tx.clone(),
        shared_ws_tx.clone(),
        ws_rx,
        bcast_rx,
        ping_timer,
        check_inactive_users,
        user_id,
        session_id,
        container_factory.clone(),
    ));
}

async fn broadcast_user_list<'a>(
    bcast_tx: tokio::sync::broadcast::Sender<ServerMessage>,
    server: &MutexGuard<'a, Server>,
) -> Result<(), WebSocketError> {
    let user_list = UserList::new(server.active_users());
    let msg = ServerMessage::UserList(user_list);

    if let Err(e) = bcast_tx.send(msg) {
        log::error!("All receiver handles have been closed. {e:?}");
        return Err(e)?;
    }
    Ok(())
}

async fn handle_messages(
    session: SharedSession,
    server: SharedServer,
    bcast_tx: tokio::sync::broadcast::Sender<ServerMessage>,
    shared_ws_tx: SharedWsSender,
    mut ws_rx: SplitStream<WebSocket>,
    mut bcast_rx: tokio::sync::broadcast::Receiver<ServerMessage>,
    mut ping_timer: tokio::time::Interval,
    mut check_inactive_users: tokio::time::Interval,
    user_id: UserId,
    session_id: SessionId,
    container_factory: SharedContainerFactory,
) -> Result<(), WebSocketError> {
    loop {
        tokio::select! {
            next = ws_rx.next() => {
                handle_ws_message(
                    next,
                    Arc::clone(&server),
                    bcast_tx.clone(),
                    Arc::clone(&shared_ws_tx),
                    user_id,
                    session_id.clone(),
                    Arc::clone(&session),
                    Arc::clone(&container_factory)
                ).await?;
            }
            msg = bcast_rx.recv() => {
                // Receive broadcast messages, forward to client
                forward_broadcast_message(msg, Arc::clone(&shared_ws_tx)).await?;
            }
            _ = ping_timer.tick() => {
                send_ping(Arc::clone(&shared_ws_tx), user_id, session_id.clone(), Arc::clone(&server)).await;
            },
            _ = check_inactive_users.tick() => {
                if let RemoveUsersRet::RemoveSelf = mark_remove_inactive_users(
                        &session_id,
                        Arc::clone(&server),
                        user_id,
                        Arc::clone(&shared_ws_tx)).await {
                    return Ok(());
                }
            },
        }
    }
}

async fn handle_ws_message(
    next: Option<Result<Message, warp::Error>>,
    server: SharedServer,
    bcast_tx: tokio::sync::broadcast::Sender<ServerMessage>,
    shared_ws_tx: SharedWsSender,
    user_id: UserId,
    session_id: SessionId,
    session: SharedSession,
    container_factory: SharedContainerFactory,
) -> Result<(), WebSocketError> {
    // handle client ws messages, broadcast to others
    match next {
        Some(msg) => match msg {
            Ok(msg) => {
                if msg.is_text() {
                    handle_text_message(
                        msg,
                        server,
                        bcast_tx,
                        shared_ws_tx,
                        session,
                        container_factory,
                    )
                    .await?;
                } else if msg.is_pong() {
                    handle_pong_message(server, user_id, session_id).await;
                } else if msg.is_close() {
                    handle_close_message(server, bcast_tx, user_id, session_id).await?;
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
            log::info!(
                "User ID {user_id} in session ID {session_id} gracefully closed ws connection"
            );
            return Ok(());
        }
    }
    Ok(())
}

async fn handle_text_message(
    msg: Message,
    server: SharedServer,
    bcast_tx: tokio::sync::broadcast::Sender<ServerMessage>,
    shared_ws_tx: SharedWsSender,
    session: SharedSession,
    container_factory: SharedContainerFactory,
) -> Result<(), WebSocketError> {
    let mut server = server.lock().await;
    // Convert network serialized method into native struct
    // to_str() is always valid because msg `is_text`
    // TODO: Replace this with `RemoteUpdate` for consistency
    let msg = msg.to_str().unwrap();
    log::debug!("Received raw message from client: {msg:?}");
    let client_ws_msg: WsClientTextMsg = serde_json::from_str(msg).unwrap();
    match client_ws_msg {
        WsClientTextMsg::BroadcastDocUpdate(doc_update_stringified) => {
            let msg: BroadcastLocalDocUpdate =
                serde_json::from_str(&doc_update_stringified.doc_update).unwrap();
            let res = server.apply_client_operation(
                msg.text_operation().clone(),
                msg.last_server_state_id(),
                msg.cursor_map(),
                msg.user_id(),
            );
            if let Err(e) = &res {
                log::error!("Error applying client operation to server: {e:?}");
            }
            let (text_op, cursor_map) = res.unwrap();

            log::debug!(
                "Current server document: {}",
                server.current_document_state().document()
            );
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
                log::info!("All receiver handles have been closed. {e:?}");
                // Handle error (e.g., all receiver handles have been closed)
                return Err(e)?;
            }
        }
        WsClientTextMsg::Execute(execute_command) => {
            log::debug!("Received Execute Command from client: {execute_command:?}");
            // Spawn new task for execution to allow processing other ws messages
            let session = Arc::clone(&session);
            let container_factory = Arc::clone(&container_factory);
            let bcast_tx = bcast_tx.clone();
            let shared_ws_tx = Arc::clone(&shared_ws_tx);
            tokio::spawn(async move {
                handle_execution(
                    execute_command,
                    session,
                    bcast_tx,
                    container_factory,
                    shared_ws_tx,
                )
                .await;
            });
        }
    };
    Ok(())
}

async fn handle_pong_message(server: SharedServer, user_id: UserId, session_id: SessionId) {
    let activity_time = std::time::Instant::now();
    log::debug!(
        "Received pong from client {user_id} in session ID {session_id} at time {activity_time:?}"
    );
    let mut server = server.lock().await;
    match server.users_mut().get_mut(&user_id) {
        Some(user) => {
            user.activity.active = true;
            user.activity.last_activity = activity_time;
        }
        None => panic!("Received pong for user ID {user_id} which does not exist in session ID {session_id} user map"),
    }
}

async fn handle_close_message(
    server: SharedServer,
    bcast_tx: tokio::sync::broadcast::Sender<ServerMessage>,
    user_id: UserId,
    session_id: SessionId,
) -> Result<(), WebSocketError> {
    log::info!("Received graceful close message from client {user_id} in session ID {session_id}, removing user");
    let mut server = server.lock().await;
    match server.mark_user_inactive(user_id) {
        Ok(_) => {}
        Err(ServerError::UserIdNotFound(user_id)) => panic!("Received close user ID {user_id} which does not exist in session ID {session_id} user map"),
        Err(e) => panic!("Error marking user inactive on close: {e:?}"),
    }
    broadcast_user_list(bcast_tx, &server).await?;
    Ok(())
}

async fn forward_broadcast_message(
    msg: Result<ServerMessage, tokio::sync::broadcast::error::RecvError>,
    shared_ws_tx: SharedWsSender,
) -> Result<(), WebSocketError> {
    match msg {
        Ok(msg) => {
            let msg = serde_json::to_string(&msg)
                .unwrap_or_else(|e| panic!("Error serializing string {msg:?}, error {e}"));
            let msg: Message = Message::text(msg);
            log::trace!("Sending message to clients: {msg:?}");
            let mut ws_tx = shared_ws_tx.lock().await;
            if let Err(e) = ws_tx.send(msg).await {
                // User has ungracefully terminated their websocket connection.
                // This may be due to refreshing the page. This is expected to occur, so
                // the server will close its message handler on this connection.
                log::debug!("Receiver websocket closed. {e:?}");
                return Err(e)?;
            }
        }
        Err(e) => match e {
            RecvError::Closed => {
                log::info!("All senders have been dropped, receiver closing");
                return Err(e)?;
            }
            RecvError::Lagged(msg_cnt) => {
                log::error!("Receiver has lagged {msg_cnt} messages");
                return Err(e)?;
            }
        },
    }
    Ok(())
}

async fn send_ping(
    shared_ws_tx: SharedWsSender,
    user_id: UserId,
    session_id: SessionId,
    server: SharedServer,
) {
    log::debug!("Sending ping to user ID {user_id} in session ID {session_id}");
    let mut ws_tx = shared_ws_tx.lock().await;
    if let Err(e) = ws_tx.send(Message::ping(Vec::new())).await {
        log::error!("Failed to send ping: {e}");
        return;
    }

    // User Update 3: Broadcast periodically in case non-gracefully disconnected users are pruned
    let server = server.lock().await;
    let user_list = UserList::new(server.active_users());
    let msg = ServerMessage::UserList(user_list);
    let msg = serde_json::ser::to_string(&msg).unwrap();
    let msg: Message = Message::text(msg);
    if let Err(e) = ws_tx.send(msg).await {
        log::error!("Failed to send ping: {e}");
    }
}

async fn handle_execution(
    execute_command: ExecuteCommand,
    session: SharedSession,
    bcast_tx: broadcast::Sender<ServerMessage>,
    container_factory: SharedContainerFactory,
    shared_ws_tx: SharedWsSender,
) {
    let container_msg = ContainerMessage::Execute(execute_command);
    let (container_response_tx, mut container_response_rx) =
        mpsc::channel(CONTAINER_RESPONSE_MSG_LIMIT);

    let handle = tokio::spawn({
        let bcast_tx = bcast_tx.clone();
        async move {
            run_code(
                container_msg,
                Arc::clone(&session),
                Arc::clone(&container_factory),
                container_response_tx,
                bcast_tx,
            )
            .await
        }
    });

    while let Some(container_response) = container_response_rx.recv().await {
        let runner_output = container_response_to_runner_output(&container_response);
        let msg = ServerMessage::Run(runner_output);
        log::debug!("Sending run output to clients");
        log::trace!("{msg:?}");
        // Broadcast the message to other clients
        if let Err(e) = bcast_tx.send(msg) {
            // Not an error, just means all receiver handles have been closed
            log::debug!("All receiver handles have been closed. {e:?}");
            // Handle error (e.g., all receiver handles have been closed)
            break;
        }
    }
    // This should exit immediately since the container response channel is closed
    log::debug!("Waiting for task execution to complete");
    match handle.await.unwrap() {
        Ok(_) => {}
        Err(e) => {
            log::error!("Error running code: {e:?}");
            match e {
                RunCodeError::ConcurrentCompilation(run_type) => {
                    // Broadcast error back to client
                    ws_notify_concurrent_code_error(shared_ws_tx, run_type).await;
                }
                RunCodeError::ContainerError(ContainerError::StderrTooLarge { .. })
                | RunCodeError::ContainerError(ContainerError::StdoutTooLarge { .. }) => {
                    // TODO: Use the correct execute type or make runtype optional in the ws message
                    bcast_notify_output_size_error(bcast_tx.clone(), RunType::Execute).await;
                }
                _ => {}
            }
        }
    };
}

enum RemoveUsersRet {
    RemoveSelf,
    Continue,
}

// Mark users as inactive if they have not responded to pings within a certain interval.
// Inactive users will not be broadcast to other clients.
// If a user is inactive for a long period, remove them permanently from the session.
async fn mark_remove_inactive_users(
    session_id: &SessionId,
    server: SharedServer,
    user_id: UserId,
    ws_tx: SharedWsSender,
) -> RemoveUsersRet {
    log::debug!("Checking if users in session ID {session_id} are inactive");
    let mut server = server.lock().await;

    log::debug!(
        "All users (inactive + active) present in session ID {session_id}: {:?}",
        server.users()
    );

    let mut users_to_mark = Vec::new();
    let mut users_to_remove = Vec::new();

    for (id, user) in server.users() {
        let user_last_activity = user.activity.last_activity;
        if user_last_activity.elapsed().as_secs() > MARK_INACTIVE_USER_SEC {
            log::debug!(
                "User {user:?} in session ID {session_id} is inactive, marking as inactive"
            );
            users_to_mark.push(*id);
        } else {
            // Not an error because the user may have gracefully left the session
        }
        if user_last_activity.elapsed().as_secs() > REMOVE_INACTIVE_USERS_SEC {
            log::debug!("User {user:?} in session ID {session_id} has been inactive for {REMOVE_INACTIVE_USERS_SEC} sec, removing");
            users_to_remove.push(*id);
        }
    }

    for user_id in users_to_mark.iter() {
        // unwrap: user_id is only added to vector if it exists in the user map
        server.mark_user_inactive(*user_id).unwrap();
    }

    // Only users who are inactive for a long period are removed, freeing their username (their user ID is never reused though)
    for id in users_to_remove.iter() {
        // unwrap: user_id is only added to vector if it exists in the user map
        let user = server.users_mut().remove(id).unwrap();
        log::debug!("Removing inactive user {user:?} from session ID {session_id}");
    }

    if users_to_mark.contains(&user_id) {
        // tx close would initiate close handshake with client, but
        let mut ws_tx = ws_tx.lock().await;
        match ws_tx.close().await {
            Ok(_) => {
                log::debug!("Closed websocket for inactive current user {user_id} from session ID {session_id}");
            }
            Err(e) => {
                log::error!("Failed to close websocket for current user {user_id} from session ID {session_id}: {e}");
            }
        }
        // Exit since the current user is inactive
        return RemoveUsersRet::RemoveSelf;
    }
    RemoveUsersRet::Continue
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
    let user_list = UserList::new(server.active_users());

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
            log::error!("Sending error, all receiver handles have been closed. {e:?}");
            // Handle error (e.g., all receiver handles have been closed)
        }
    }
}

pub fn websocket_route(
    session_map: SharedSessionMap,
    container_factory: SharedContainerFactory,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path("websocket")
        .and(warp::path::param())
        .and(warp::path::param())
        .and(warp::ws())
        .map(
            move |session_id: String, user_id: UserId, ws: warp::ws::Ws| {
                let session_map = Arc::clone(&session_map);
                let container_factory = Arc::clone(&container_factory);
                ws.on_upgrade(move |ws| {
                    handle_websocket(ws, session_map, session_id, user_id, container_factory)
                })
            },
        )
}
