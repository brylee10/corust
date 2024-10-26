//! Sessions and users management module.
//!
//! A [`Session`] is a Corust collaborative editor session. Each has its own
//! [`SharedServer`] which is a [`Server`] which manages user states and
//! historical document states for the session.

use std::sync::Arc;

use corust_components::{server::Server, ServerMessage};
use corust_sandbox::container::CargoCommand;
use dashmap::DashMap;
use fnv::FnvHashMap;
use tokio::sync::{
    broadcast::{channel, Sender},
    RwLock,
};

use crate::{
    execute::runner::{CodeOutputState, ConcurrentRunChecker, SharedConcurrentRunChecker},
    messages::SharedServer,
};

pub type SessionId = String;
pub type SharedSessionMap = Arc<SessionMap>;
pub type SharedSession = Arc<Session>;

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

    pub fn get_or_create_session(&self, session_id: &SessionId) -> SharedSession {
        if !self.sessions.contains_key(session_id) {
            self.create_session(session_id);
        }
        // Session exists or was just created
        self.get_session(session_id).unwrap()
    }

    /// Creates a new session given a session ID. IDs are requested by the users and not assigned by the server
    pub fn create_session(&self, session_id: &SessionId) {
        let session = Session::new(session_id.clone());
        self.sessions.insert(session_id.clone(), Arc::new(session));
    }

    pub fn get_session(&self, session_id: &SessionId) -> Option<SharedSession> {
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
