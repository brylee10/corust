//! Background tasks for the server, typically for "cleanup".
//!
//! Includes pruning document states, archiving empty sessions, and marking and removing inactive users.
//! Analogous to a "garbage collector" of sorts.

use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Duration};

use corust_components::network::UserId;
use tokio::time::Instant;

use crate::{
    db::{DocumentTable, DocumentTableKey, Table, UserTable, UserTableKey},
    sessions::{SharedServer, SharedSessionMap},
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
/// The time in seconds to prune document states from all sessions
const PRUNE_DOCUMENT_STATES_INTERVAL_SEC: u64 = 30 + 1; // Just over 30 seconds

/// Spawns background tasks to manage sessions, particularly to clean up inactive users and archive empty sessions.
pub fn spawn_background_session_managers(session_map: SharedSessionMap, db_path: PathBuf) {
    tokio::spawn(mark_remove_inactive_users_all_sessions(
        Arc::clone(&session_map),
        db_path.clone(),
    ));
    tokio::spawn(archive_remove_empty_sessions(
        db_path,
        Arc::clone(&session_map),
    ));
    tokio::spawn(spawn_prune_document_states_task(Arc::clone(&session_map)));
}

/// Prune unneeded document states from all sessions periodically
async fn spawn_prune_document_states_task(session_map: SharedSessionMap) {
    let mut prune_interval =
        tokio::time::interval(Duration::from_secs(PRUNE_DOCUMENT_STATES_INTERVAL_SEC));
    loop {
        tracing::debug!("Starting prune document states task");
        prune_interval.tick().await;
        let start_time = Instant::now();
        for session in session_map.sessions.iter_mut() {
            tracing::debug!("Pruning document states for session {}", session.key());
            session.server().write().await.prune_document_states();
        }
        let duration = start_time.elapsed();
        tracing::debug!("Pruned document states took {:?}us", duration.as_micros());
    }
}

pub struct MarkRemoveUsers {
    pub users_to_mark: HashSet<UserId>,
    pub users_to_remove: HashSet<UserId>,
}

/// Marks users as inactive if they have not responded to pings within a certain time.
/// Removes users who have been inactive for a long period. Removed users are archived to a database.
pub(crate) async fn mark_remove_inactive_users(
    server: SharedServer,
    session_id: &str,
    db_path: PathBuf,
) -> MarkRemoveUsers {
    let mut users_to_mark = HashSet::new();
    let mut users_to_remove = HashSet::new();

    for (id, user) in server.read().await.users() {
        let user_last_activity = user.activity.last_activity;
        if user_last_activity.elapsed().as_secs() > MARK_INACTIVE_USER_SEC {
            tracing::debug!(
                "User {user:?} in session ID {session_id} is inactive, marking as inactive"
            );
            users_to_mark.insert(*id);
        } else {
            // Not an error because the user may have gracefully left the session
        }
        if user_last_activity.elapsed().as_secs() > REMOVE_INACTIVE_USERS_SEC {
            tracing::debug!(
                "User {user:?} in session ID {session_id} has been inactive for {REMOVE_INACTIVE_USERS_SEC} sec, removing"
            );
            users_to_remove.insert(*id);
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
        tracing::debug!("Removing inactive user {user:?} from session ID {session_id}");

        // On removal, users are saved to the database
        let user_table = UserTable::new(db_path.to_path_buf());
        let user_key = UserTableKey {
            session_id: session_id.to_string(),
            user_id: user.user_id(),
        };
        if let Err(e) = user_table.insert_or_update(user_key.clone(), user) {
            // This error is not fatal, but the user will not be saved to the database
            tracing::error!("Error inserting user {user_key:?} into database on removal: {e}");
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
    tracing::debug!("Starting background task to mark and remove inactive users");
    loop {
        tracing::debug!("Checking for inactive users to mark and remove");
        for item in session_map.sessions.iter() {
            let (session_id, session) = item.pair();
            let server = Arc::clone(&session.server());
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
    tracing::debug!("Starting background task to archive empty sessions");
    loop {
        tracing::debug!("Checking for empty sessions to archive");
        let document_table = DocumentTable::new(db_path.clone());
        let user_table = UserTable::new(db_path.clone());
        let mut empty_sessions = Vec::new();
        for item in session_map.sessions.iter() {
            let (session_id, session) = item.pair();
            let server = Arc::clone(&session.server());
            let server = server.read().await;
            tracing::debug!(
                "Checking session_id {}, num active users: {}",
                session_id,
                server.active_users().len()
            );
            if server.active_users().is_empty() {
                empty_sessions.push(session_id.clone());
            }
        }
        for session_id in empty_sessions {
            tracing::debug!("Archiving empty session: {}", session_id);
            // unwrap: `session_id`` was just retrieved from the `session_map`
            let session = session_map.get_session(&session_id).unwrap();
            let server = Arc::clone(&session.server());
            let server = server.read().await;
            // Archive the document state
            let document_key = DocumentTableKey {
                session_id: session_id.clone(),
            };
            if let Err(e) = document_table
                .insert_or_update(document_key, server.current_document_state().clone())
            {
                tracing::error!(
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
                    tracing::error!(
                        "Error inserting or updating user while archiving empty sessions: {}",
                        e
                    );
                }
                tracing::debug!("Archived user {:?} in session id {}", user, session_id);
            }
            drop(server);
            // Remove the session from the session map
            session_map.sessions.remove(&session_id);
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(ARCHIVE_EMPTY_SESSIONS_SEC)).await;
    }
}
