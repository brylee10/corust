//! Sessions and users management module.

use std::sync::Arc;

use corust_components::{server::Server, ServerMessage};
use corust_sandbox::container::CargoCommand;
use fnv::FnvHashMap;
use tokio::sync::{
    broadcast::{channel, Sender},
    Mutex,
};

use crate::{messages::SharedServer, runner::compile::CodeOutputState};

pub type SessionId = String;
pub type SharedSessionMap = Arc<Mutex<SessionMap>>;
pub type SharedSession = Arc<Mutex<Session>>;

pub struct SessionMap {
    sessions: FnvHashMap<SessionId, SharedSession>,
}

impl Default for SessionMap {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionMap {
    pub fn new() -> Self {
        SessionMap {
            sessions: FnvHashMap::default(),
        }
    }

    pub fn get_or_create_session(&mut self, session_id: &SessionId) -> SharedSession {
        if !self.sessions.contains_key(session_id) {
            self.create_session(session_id);
        }
        // Session exists or was just created
        self.get_session(session_id).unwrap()
    }

    /// Creates a new session given a session ID. IDs are requested by the users and not assigned by the server
    pub fn create_session(&mut self, session_id: &SessionId) {
        let session = Session::new(session_id.clone());
        self.sessions
            .insert(session_id.clone(), Arc::new(Mutex::new(session)));
    }

    pub fn get_session(&self, session_id: &SessionId) -> Option<SharedSession> {
        self.sessions.get(session_id).map(Arc::clone)
    }
}

pub struct Session {
    #[allow(dead_code)]
    session_id: SessionId,
    server: SharedServer,
    bcast_tx: Sender<ServerMessage>,
    _code_output_state: FnvHashMap<CargoCommand, CodeOutputState>,
}

impl Session {
    /// Creates a new session and initializes the server.
    pub fn new(session_id: SessionId) -> Self {
        let server = Server::new();
        let server = Arc::new(Mutex::new(server));
        // Selected arbitrary max messages for broadcast channel
        // Chose broadcast because each connection will be a sender and receiver
        let (bcast_tx, _) = channel(100000);
        Session {
            session_id,
            server,
            bcast_tx,
            _code_output_state: FnvHashMap::default(),
        }
    }

    pub fn bcast_tx(&self) -> Sender<ServerMessage> {
        self.bcast_tx.clone()
    }

    pub fn server(&self) -> SharedServer {
        Arc::clone(&self.server)
    }
}
