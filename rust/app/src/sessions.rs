//! Sessions and users management module.
//!
//! A [`Session`] is a Corust collaborative editor session. Each has its own
//! [`SharedServer`] which is a [`Server`] which manages user states and
//! historical document states for the session.

use parking_lot::RwLock as BlockingRwLock;
use std::sync::Arc;

use corust_components::{
    RunConfig, ServerMessage,
    server::{DocumentState, Server},
};
use corust_types::{CodeOutputState, execution::CargoCommandType};
use dashmap::{DashMap, mapref::one::RefMut};
use tokio::sync::{
    RwLock,
    broadcast::{Sender, channel},
};

use crate::execute::runner::{ConcurrentRunChecker, SharedConcurrentRunChecker};

pub type SessionId = String;
pub type SharedSessionMap = Arc<SessionMap>;
pub type SharedSession = Arc<Session>;
pub type SharedServer = Arc<RwLock<Server>>;

/// A concurrently accessible container for all Corust sessions.
pub struct SessionMap {
    pub(crate) sessions: DashMap<SessionId, SharedSession>,
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
    code_output: DashMap<CargoCommandType, CodeOutputState>,
    concurrent_run_checker: SharedConcurrentRunChecker,
    run_config: BlockingRwLock<RunConfig>,
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
            code_output: DashMap::default(),
            concurrent_run_checker: Arc::new(ConcurrentRunChecker::new()),
            run_config: BlockingRwLock::new(RunConfig::default()),
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
            code_output: DashMap::default(),
            concurrent_run_checker: Arc::new(ConcurrentRunChecker::new()),
            run_config: BlockingRwLock::new(RunConfig::default()),
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

    /// Returns an owned copy of the current code output state for a command type.
    pub fn code_output_state(&self, command_type: &CargoCommandType) -> Option<CodeOutputState> {
        self.code_output
            .get(command_type)
            .map(|el| el.value().clone())
    }

    pub fn code_output_state_mut(
        &self,
        command_type: &CargoCommandType,
    ) -> Option<RefMut<'_, CargoCommandType, CodeOutputState>> {
        self.code_output.get_mut(command_type)
    }

    pub fn set_code_output_state(
        &self,
        command_type: CargoCommandType,
        code_output_state: CodeOutputState,
    ) {
        self.code_output.insert(command_type, code_output_state);
    }

    pub fn run_config(&self) -> RunConfig {
        // The blocking lock call is short lived
        *self.run_config.read()
    }

    pub fn set_run_config(&self, run_config: RunConfig) {
        // The blocking lock call is short lived
        *self.run_config.write() = run_config;
    }
}
