//! Sessions and users management module.
//!
//! A [`Session`] is a Corust collaborative editor session. Each has its own
//! [`SharedServer`] which is a [`Server`] which manages user states and
//! historical document states for the session.

use std::{path::PathBuf, sync::Arc};

use corust_components::{
    network::UserId,
    server::{DocumentState, Server},
    ServerMessage,
};
use corust_sandbox::container::CargoCommand;
use dashmap::DashMap;
use fnv::FnvHashMap;
use tokio::sync::{
    broadcast::{channel, Sender},
    RwLock,
};

use crate::{
    db::{DocumentTable, DocumentTableKey, Table, UserTable, UserTableKey},
    execute::runner::{CodeOutputState, ConcurrentRunChecker, SharedConcurrentRunChecker},
    websocket::PING_INTERVAL_SEC,
};

/// The time in seconds the archiver runs to check and archive empty sessions.
/// This can happen relatively infrequently. This reduces memory usage and saves sessions to disk.
const ARCHIVE_EMPTY_SESSIONS_SEC: u64 = 60 * 10; // 10 minutes
/// Time in seconds to check for inactive users. This only configures the frequency of the periodic
/// background task. This clears users who are in sessions without any active users. In sessions with
/// active users, a more frequent check is run.
const CHECK_INACTIVE_USERS_SEC: u64 = 60 * 5 + 1; // 5m + 1s to not overlap with archive task
/// Users which have not responded to pings within this time will be marked as inactive.
/// Three ping cycles without a response.
const MARK_INACTIVE_USER_SEC: u64 = PING_INTERVAL_SEC * 3;
/// 30 minutes - This is the period a user claims the same username before being removed
/// Allows users who refresh their page to claim the same identity
const REMOVE_INACTIVE_USERS_SEC: u64 = 60 * 30;

pub type SessionId = String;
pub type SharedSessionMap = Arc<SessionMap>;
pub type SharedSession = Arc<Session>;
pub type SharedServer = Arc<RwLock<Server>>;

/// A concurrently accessible container for all Corust sessions.
pub struct SessionMap {
    sessions: DashMap<SessionId, SharedSession>,
}

impl Default for SessionMap {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionMap {
    pub fn new() -> Self {
        SessionMap {
            sessions: DashMap::default(),
        }
    }

    pub fn get_or_create_session(&self, session_id: &str) -> SharedSession {
        log::debug!("Getting or creating session with ID {}", session_id);
        if !self.sessions.contains_key(session_id) {
            self.create_session(session_id);
        }
        // Session exists or was just created
        self.get_session(session_id).unwrap()
    }

    /// Creates a new session given a session ID. IDs are requested by the users and not assigned by the server
    pub fn create_session(&self, session_id: &str) {
        log::debug!("Creating session with ID {}", session_id);
        let session = Session::new(session_id.to_string());
        self.sessions
            .insert(session_id.to_string(), Arc::new(session));
    }

    /// Creates a new session with a [`DocumentState`] given a session ID. IDs are requested by the users and not assigned by the server
    /// If a session with the same ID already exists, it will be retrieved.
    pub fn get_or_create_session_with_document_state(
        &self,
        session_id: &str,
        document_state: DocumentState,
    ) -> SharedSession {
        log::debug!(
            "Getting or creating session with ID {} and document",
            session_id
        );
        if !self.sessions.contains_key(session_id) {
            let session = Session::new_with_document_state(session_id.to_string(), document_state);
            self.sessions
                .insert(session_id.to_string(), Arc::new(session));
        }
        // Session exists or was just created
        self.get_session(session_id).unwrap()
    }

    pub fn get_session(&self, session_id: &str) -> Option<SharedSession> {
        log::debug!("Get session with ID {}", session_id);
        self.sessions.get(session_id).as_deref().map(Arc::clone)
    }
}

/// A single Corust session
pub struct Session {
    session_id: SessionId,
    server: SharedServer,
    bcast_tx: Sender<ServerMessage>,
    _code_output_state: FnvHashMap<CargoCommand, CodeOutputState>,
    concurrent_run_checker: SharedConcurrentRunChecker,
}

impl Session {
    /// Creates a new session and initializes the server.
    pub fn new(session_id: SessionId) -> Self {
        let server = Server::new();
        let server = Arc::new(RwLock::new(server));
        // Selected arbitrary max messages for broadcast channel
        // Chose broadcast because each connection will be a sender and receiver
        //
        // Capacity limit: Previously was much larger, but led to high memory
        // overhead for new sessions (100k caused 20MB memory increase per new session).
        // 1k will be sufficient because the most frequent message would be RemoteUpdates.
        // Even with 20 simultaneous collaborators editting quickly, each collaborator has 50
        // updates in the time it takes to empty the queue. We would rate limit edit frequency to
        // ~500 updates / min (avg 8 per second) which is more than the fastest typing speed WPM.
        // The channel should be able to deque up to 20 * 8 messages / second = 160 messages / second
        // (6ms per message avg would be extremely slow).
        let (bcast_tx, _) = channel(100);
        Session {
            session_id,
            server,
            bcast_tx,
            _code_output_state: FnvHashMap::default(),
            concurrent_run_checker: Arc::new(ConcurrentRunChecker::new()),
        }
    }

    /// Creates a new session where the server is initialized with a [`DocumentState`].
    pub fn new_with_document_state(session_id: SessionId, document_state: DocumentState) -> Self {
        let server = Server::new_with_document_state(document_state);
        let server = Arc::new(RwLock::new(server));
        let (bcast_tx, _) = channel(100);
        Session {
            session_id,
            server,
            bcast_tx,
            _code_output_state: FnvHashMap::default(),
            concurrent_run_checker: Arc::new(ConcurrentRunChecker::new()),
        }
    }

    pub fn bcast_tx(&self) -> Sender<ServerMessage> {
        self.bcast_tx.clone()
    }

    pub fn server(&self) -> SharedServer {
        Arc::clone(&self.server)
    }

    pub fn concurrent_run_checker(&self) -> SharedConcurrentRunChecker {
        Arc::clone(&self.concurrent_run_checker)
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id.clone()
    }
}

pub struct MarkRemoveUsers {
    pub users_to_mark: Vec<UserId>,
    pub users_to_remove: Vec<UserId>,
}

/// Marks users as inactive if they have not responded to pings within a certain time.
/// Removes users who have been inactive for a long period. Removed users are archived to a database.
pub(crate) async fn mark_remove_inactive_users(
    server: SharedServer,
    session_id: &str,
    db_path: PathBuf,
) -> MarkRemoveUsers {
    let mut users_to_mark = Vec::new();
    let mut users_to_remove = Vec::new();

    for (id, user) in server.read().await.users() {
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

        if user_last_activity.elapsed().as_secs() > REMOVE_INACTIVE_USERS_SEC {
            log::debug!("User {user:?} in session ID {session_id} has been inactive for {REMOVE_INACTIVE_USERS_SEC} sec, removing");
            users_to_remove.push(*id);
        }
    }

    for user_id in users_to_mark.iter() {
        // unwrap: user_id is only added to vector if it exists in the user map
        server.write().await.mark_user_inactive(*user_id).unwrap();
    }

    // Only users who are inactive for a long period are removed, freeing their username (their user ID is never reused though)
    for id in users_to_remove.iter() {
        // unwrap: user_id is only added to vector if it exists in the user map
        let user = server.write().await.users_mut().remove(id).unwrap();
        log::debug!("Removing inactive user {user:?} from session ID {session_id}");

        // On removal, users are saved to the database
        let user_table = UserTable::new(db_path.to_path_buf());
        let user_key = UserTableKey {
            session_id: session_id.to_string(),
            user_id: user.user_id(),
        };
        if let Err(e) = user_table.insert_or_update(user_key.clone(), user) {
            // This error is not fatal, but the user will not be saved to the database
            log::error!("Error inserting user {user_key:?} into database on removal: {e}");
        }
    }

    MarkRemoveUsers {
        users_to_mark,
        users_to_remove,
    }
}

/// Iterates over all sessions in the session map and marks users as inactive
/// if they have not responded to pings within a certain time.
/// Intended to run as a background task.
async fn mark_remove_inactive_users_all_sessions(session_map: SharedSessionMap, db_path: PathBuf) {
    log::debug!("Starting background task to mark and remove inactive users");
    loop {
        log::debug!("Checking for inactive users to mark and remove");
        for item in session_map.sessions.iter() {
            let (session_id, session) = item.pair();
            let server = session.server.clone();
            mark_remove_inactive_users(server, session_id, db_path.clone()).await;
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(CHECK_INACTIVE_USERS_SEC)).await;
    }
}

/// Iterates over all sessions in the session map and archives sessions with no active users.
/// Removes the empty sessions from the session map.
/// Intended to run as a background task. The data is archived to a database.
/// Archives both the document state and the user info.
async fn archive_remove_empty_sessions(db_path: PathBuf, session_map: SharedSessionMap) {
    log::debug!("Starting background task to archive empty sessions");
    loop {
        log::debug!("Checking for empty sessions to archive");
        let document_table = DocumentTable::new(db_path.clone());
        let user_table = UserTable::new(db_path.clone());
        let mut empty_sessions = Vec::new();
        for item in session_map.sessions.iter() {
            let (session_id, session) = item.pair();
            let server = session.server.read().await;
            log::debug!(
                "Checking session_id {}, num active users: {}",
                session_id,
                server.active_users().len()
            );
            if server.active_users().is_empty() {
                empty_sessions.push(session_id.clone());
            }
        }
        for session_id in empty_sessions {
            log::debug!("Archiving empty session: {}", session_id);
            // unwrap: `session_id`` was just retrieved from the `session_map`
            let session = session_map.get_session(&session_id).unwrap();
            let server = session.server.read().await;
            // Archive the document state
            let document_key = DocumentTableKey {
                session_id: session_id.clone(),
            };
            if let Err(e) = document_table
                .insert_or_update(document_key, server.current_document_state().clone())
            {
                log::error!(
                    "Error inserting or updating document while archiving empty sessions: {}",
                    e
                );
            }
            // Archive the user state
            for (user_id, user) in server.users() {
                let user_key = UserTableKey {
                    session_id: session_id.clone(),
                    user_id: *user_id,
                };
                if let Err(e) = user_table.insert_or_update(user_key, user.clone()) {
                    log::error!(
                        "Error inserting or updating user while archiving empty sessions: {}",
                        e
                    );
                }
                log::debug!("Archived user {:?} in session id {}", user, session_id);
            }
            drop(server);
            // Remove the session from the session map
            session_map.sessions.remove(&session_id);
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(ARCHIVE_EMPTY_SESSIONS_SEC)).await;
    }
}

/// Spawns background tasks to manage sessions, particularly to clean up inactive users and archive empty sessions.
pub fn spawn_background_session_managers(session_map: SharedSessionMap, db_path: PathBuf) {
    tokio::spawn(mark_remove_inactive_users_all_sessions(
        session_map.clone(),
        db_path.clone(),
    ));
    tokio::spawn(archive_remove_empty_sessions(db_path, session_map));
}
