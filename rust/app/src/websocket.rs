use std::path::{Path, PathBuf};
use std::sync::Arc;

use corust_components::network::{UserId, UserList};
use corust_components::server::ServerError;
use corust_components::BroadcastLocalDocUpdate;
use corust_sandbox::container::{ContainerError, ContainerMessage, ExecuteCommand};
use futures_util::stream::{SplitSink, SplitStream, StreamExt};
use futures_util::SinkExt;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, RwLock};

use crate::execute::runner::{
    bcast_notify_output_size_error, container_response_to_runner_output, run_code,
    ws_notify_concurrent_code_error, RunCodeError, RunType, SharedContainerFactory,
};
use crate::sessions::{
    mark_remove_inactive_users, MarkRemoveUsers, SessionId, SharedServer, SharedSession,
    SharedSessionMap,
};
use corust_components::{network::RemoteUpdate, ServerMessage, Snapshot};
use tokio::sync::broadcast::error::{RecvError, SendError};
use tokio::time::Duration;
use warp::{
    filters::ws::{Message, WebSocket},
    Filter,
};

// Frequency to send pings to each client, in seconds
pub const PING_INTERVAL_SEC: u64 = 10;
// Frequency to check for client inactivity, in seconds
// Note that inactive user check for each connection will check all users for inactivity
// If a connection did not end gracefully then the caller itself was unable to remove itself
const CHECK_INACTIVE_USERS_SEC: u64 = 30;
// Number of [`ContainerResponse`] messages that can be bufferred from a running container
// in the channel
const CONTAINER_RESPONSE_MSG_LIMIT: usize = 8;

/// Shared to concurrently listen to and handle different client and server messages
/// in the core server loop.
pub type SharedWsSender = Arc<RwLock<SplitSink<WebSocket, Message>>>;
type IsConnectionOpen = bool;

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
    db_path: PathBuf,
) {
    // unwrap: session_id is always valid, the user gets or creates a session on join, and the user joins
    // before connecting to the websocket
    // The acquired DashMap lock is only held for the duration of this call
    let session = session_map.get_session(&session_id).unwrap();
    let (bcast_tx, server) = (session.bcast_tx(), session.server());
    let bcast_rx = bcast_tx.subscribe();
    let (mut ws_tx, ws_rx) = websocket.split();
    // unwrap: `user_id` will correspond to a username in the session
    let username = server
        .read()
        .await
        .users()
        .get(&user_id)
        .unwrap()
        .username()
        .to_string();

    // Sync the late joiner with the current server doc state
    send_snapshot(Arc::clone(&server), &mut ws_tx).await;

    // User Update 1: On join, broadcast new user list
    {
        if let Err(_) = broadcast_user_list(bcast_tx.clone(), Arc::clone(&server)).await {
            return;
        }
    }

    let shared_ws_tx = Arc::new(RwLock::new(ws_tx));
    let ping_timer = tokio::time::interval(Duration::from_secs(PING_INTERVAL_SEC));
    let check_inactive_users = tokio::time::interval(Duration::from_secs(CHECK_INACTIVE_USERS_SEC));

    // Spawn a task to receive messages
    tokio::task::spawn(handle_messages(
        Arc::clone(&session),
        Arc::clone(&server),
        bcast_tx.clone(),
        Arc::clone(&shared_ws_tx),
        ws_rx,
        bcast_rx,
        ping_timer,
        check_inactive_users,
        user_id,
        session_id,
        Arc::clone(&container_factory),
        db_path,
        username,
    ));
}

async fn broadcast_user_list<'a>(
    bcast_tx: tokio::sync::broadcast::Sender<ServerMessage>,
    server: SharedServer,
) -> Result<(), WebSocketError> {
    let user_list = UserList::new(server.read().await.active_users());
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
    db_path: PathBuf,
    username: String,
) -> Result<(), WebSocketError> {
    'outer: loop {
        tokio::select! {
            next = ws_rx.next() => {
                let is_connection_open = handle_ws_message(
                    next,
                    Arc::clone(&server),
                    bcast_tx.clone(),
                    Arc::clone(&shared_ws_tx),
                    user_id,
                    session_id.clone(),
                    Arc::clone(&session),
                    Arc::clone(&container_factory),
                    &username,
                ).await?;
                // Terminate handler for this client connection. User has (un)gracefully
                // closed the websocket.
                if !is_connection_open {
                    break 'outer;
                }
            }
            msg = bcast_rx.recv() => {
                // Receive broadcast messages, forward to client
                forward_broadcast_message(msg, Arc::clone(&shared_ws_tx)).await?;
            }
            _ = ping_timer.tick() => {
                send_ping(Arc::clone(&shared_ws_tx), user_id, session_id.clone(), Arc::clone(&server)).await;
            },
            _ = check_inactive_users.tick() => {
                if let RemoveUsersRet::RemoveSelf = ws_mark_remove_inactive_users(
                        &session_id,
                        Arc::clone(&server),
                        user_id,
                        Arc::clone(&shared_ws_tx),
                        db_path.as_path()
                    ).await {
                    break 'outer;
                }
            },
        }
    }
    log::debug!("Closed websocket handling loop for user ID {user_id:?} in session {session_id:?}");
    Ok(())
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
    username: &str,
) -> Result<IsConnectionOpen, WebSocketError> {
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
                        username,
                    )
                    .await?;
                } else if msg.is_pong() {
                    handle_pong_message(server, user_id, session_id).await;
                } else if msg.is_close() {
                    handle_close_message(server, bcast_tx, user_id, session_id).await?;
                    return Ok(false);
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
                "User ID {user_id} in session ID {session_id} ws Stream exhausted, no more messages."
            );
            return Ok(false);
        }
    }
    Ok(true)
}

async fn handle_text_message(
    msg: Message,
    server: SharedServer,
    bcast_tx: tokio::sync::broadcast::Sender<ServerMessage>,
    shared_ws_tx: SharedWsSender,
    session: SharedSession,
    container_factory: SharedContainerFactory,
    username: &str,
) -> Result<(), WebSocketError> {
    // Convert network serialized method into native struct
    // to_str() is always valid because msg `is_text`
    // TODO: Replace this with `RemoteUpdate` for consistency
    let msg = msg.to_str().unwrap();
    log::trace!("Received raw message from client: {msg:?}");
    let client_ws_msg: WsClientTextMsg = serde_json::from_str(msg).unwrap();
    match client_ws_msg {
        WsClientTextMsg::BroadcastDocUpdate(doc_update_stringified) => {
            let msg: BroadcastLocalDocUpdate =
                serde_json::from_str(&doc_update_stringified.doc_update).unwrap();
            let res = server.write().await.apply_client_operation(
                msg.text_operation().clone(),
                msg.last_server_state_id(),
                msg.cursor_map(),
                msg.user_id(),
            );
            if let Err(e) = &res {
                log::error!("Error applying client operation to server: {e:?}");
            }
            let (text_op, cursor_map) = res.unwrap();

            log::trace!(
                "Current server document: {}",
                server.read().await.current_document_state().document()
            );
            debug_assert!(&cursor_map == server.read().await.current_document_state().cursor_map());

            let remote_update = RemoteUpdate {
                // Todo, replace with real IDs
                source: msg.user_id(),
                dest: 0,
                state_id: server.read().await.current_state_id(),
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
            tokio::spawn({
                let username = username.to_string();
                async move {
                    handle_execution(
                        execute_command,
                        session,
                        bcast_tx,
                        container_factory,
                        shared_ws_tx,
                        username,
                    )
                    .await;
                }
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
    match server.write().await.users_mut().get_mut(&user_id) {
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
    match server.write().await.mark_user_inactive(user_id) {
        Ok(_) => {}
        Err(ServerError::UserIdNotFound(user_id)) => panic!("Received close user ID {user_id} which does not exist in session ID {session_id} user map"),
        Err(e) => panic!("Error marking user inactive on close: {e:?}"),
    }
    broadcast_user_list(bcast_tx, server).await?;
    Ok(())
}

async fn forward_broadcast_message(
    msg: Result<ServerMessage, tokio::sync::broadcast::error::RecvError>,
    shared_ws_tx: SharedWsSender,
) -> Result<(), WebSocketError> {
    log::error!("Received broadcast message from server: {:?}", msg);
    match msg {
        Ok(msg) => {
            let msg = serde_json::to_string(&msg)
                .unwrap_or_else(|e| panic!("Error serializing string {msg:?}, error {e}"));
            let msg: Message = Message::text(msg);
            log::trace!("Sending message to clients: {msg:?}");
            if let Err(e) = shared_ws_tx.write().await.send(msg).await {
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
    if let Err(e) = shared_ws_tx
        .write()
        .await
        .send(Message::ping(Vec::new()))
        .await
    {
        log::error!("Failed to send ping: {e}");
        return;
    }

    // User Update 3: Broadcast periodically in case non-gracefully disconnected users are pruned
    let user_list = UserList::new(server.read().await.active_users());
    let msg = ServerMessage::UserList(user_list);
    let msg = serde_json::ser::to_string(&msg).unwrap();
    let msg: Message = Message::text(msg);
    if let Err(e) = shared_ws_tx.write().await.send(msg).await {
        log::error!("Failed to send ping: {e}");
    }
}

async fn handle_execution(
    execute_command: ExecuteCommand,
    session: SharedSession,
    bcast_tx: broadcast::Sender<ServerMessage>,
    container_factory: SharedContainerFactory,
    shared_ws_tx: SharedWsSender,
    username: String,
) {
    let container_msg = ContainerMessage::Execute(execute_command);
    let (container_response_tx, mut container_response_rx) =
        mpsc::channel(CONTAINER_RESPONSE_MSG_LIMIT);

    // Not spawn blocking because the executing container is eventually spawned as a child process
    let handle = tokio::spawn({
        let bcast_tx = bcast_tx.clone();
        async move {
            run_code(
                container_msg,
                Arc::clone(&session),
                Arc::clone(&container_factory),
                container_response_tx,
                bcast_tx,
                username,
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
async fn ws_mark_remove_inactive_users(
    session_id: &SessionId,
    server: SharedServer,
    user_id: UserId,
    ws_tx: SharedWsSender,
    db_path: &Path,
) -> RemoveUsersRet {
    log::debug!("Checking if users in session ID {session_id} are inactive");

    log::debug!(
        "All users (inactive + active) present in session ID {session_id}: {:?}",
        server.read().await.users()
    );

    let MarkRemoveUsers { users_to_mark, .. } =
        mark_remove_inactive_users(server, session_id, db_path.to_path_buf()).await;

    if users_to_mark.contains(&user_id) {
        // tx close would initiate close handshake with client, but
        match ws_tx.write().await.close().await {
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
    let server = server.read().await;
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
    db_path: PathBuf,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path("websocket")
        .and(warp::path::param())
        .and(warp::path::param())
        .and(warp::ws())
        .map(
            move |session_id: String, user_id: UserId, ws: warp::ws::Ws| {
                let session_map = Arc::clone(&session_map);
                let container_factory = Arc::clone(&container_factory);
                let db_path = db_path.clone();
                ws.on_upgrade(move |ws| {
                    handle_websocket(
                        ws,
                        session_map,
                        session_id,
                        user_id,
                        container_factory,
                        db_path,
                    )
                })
            },
        )
}
