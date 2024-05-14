//!  Sample server to illustrate operational transform

use anyhow::Result;
use std::ops::{Deref, DerefMut};

use fnv::FnvHashMap;

use corust_transforms::xforms::{TextOperation, TextOperationError};
use thiserror::Error;

use crate::{
    network::{
        transform_cursor, transform_cursor_map, Activity, Component, ComponentId, ComponentKind,
        CursorMap, CursorPos, CursorTransformError, LocalMessage, Network, NetworkShared,
        RemoteUpdate, User, UserId,
    },
    Snapshot,
};

// Increments on each server document update
pub type StateId = u64;

#[derive(Debug)]
pub struct DocumentState {
    state_id: StateId,
    // Document state at ID `state_id`
    document: String,
    // Text operation to transformed document and cursors from previous state to this state
    text_op: TextOperation,
    cursor_map: CursorMap,
}

impl DocumentState {
    pub fn new(
        state_id: StateId,
        document: String,
        text_op: TextOperation,
        cursor_map: CursorMap,
    ) -> Self {
        DocumentState {
            state_id,
            document,
            text_op,
            cursor_map,
        }
    }

    pub fn state_id(&self) -> StateId {
        self.state_id
    }

    pub fn document(&self) -> &str {
        &self.document
    }

    pub fn cursor_map(&self) -> &CursorMap {
        &self.cursor_map
    }

    pub fn cursor_pos(&self, user_id: UserId) -> Option<&CursorPos> {
        self.cursor_map.get(&user_id)
    }
}

#[derive(Debug, Default)]
pub struct Server {
    // Map of state_id to server document state
    // Current document state is the value at `current_state_id`
    document_states: FnvHashMap<StateId, DocumentState>,
    // Current state_id
    current_state_id: StateId,
    // All users that have connected to the server (active or inactive)
    users: FnvHashMap<UserId, User>,
    next_id: UserId,
}

impl Server {
    pub fn new() -> Self {
        let mut document_states = FnvHashMap::default();
        // Doc starts with empty string
        document_states.insert(
            0,
            DocumentState::new(
                0,
                String::new(),
                TextOperation::default(),
                CursorMap::default(),
            ),
        );
        Server {
            document_states,
            current_state_id: 0,
            users: FnvHashMap::default(),
            next_id: 0,
        }
    }

    // Given a client operation (`client_op`) which the user intended to apply to a given server doc state (`state_id`),
    // transform the client operation to apply to the current server doc state (`current_state_id`). Also given the
    // `user_id` of the user who applied the operation with the `cursor_map` indicating the map of all cursor positions
    // *after* the operation.
    // Stores the new server state. Returns the applied client op and the resulting cursor map to broadcast to all clients.
    pub fn apply_client_operation(
        &mut self,
        client_op: TextOperation,
        state_id: StateId,
        cursor_map: &CursorMap,
        user_id: UserId,
    ) -> Result<(TextOperation, CursorMap), ServerError> {
        // `state_id` will map to a historical state (last doc state in common with the client)
        let current_server_doc_state = self
            .document_states
            .get(&self.current_state_id)
            // This should never fail, `current_state_id` should always exists in the document map
            .ok_or(ServerError::StateIdNotFound(self.current_state_id))?;
        log::debug!(
            "Applying client operation. Incoming state_id: {state_id}, current state_id: {}",
            self.current_state_id
        );
        // `server_op_transformed` - server operation to convert from client state space to server state space. Only returned if client
        // is not up to date with the server.
        let (new_server_doc_state, server_op_transformed, client_op_applied) =
            if self.current_state_id == state_id {
                // Client is up to date with the server, can directly apply the `client_op`
                (
                    client_op.apply(current_server_doc_state.document())?,
                    None,
                    client_op,
                )
            } else {
                // Single operation which transforms the server document at `state_id` to the current server document state
                let composed_server_edit_ops =
                    self.compose_text_ops_between_states(state_id, self.current_state_id)?;
                // Both the server and client ops act on the doc state at `state_id`, so no length incompatibilities should occur.
                // Transformed server op is not needed, so it is discarded.
                let (server_op_transformed, client_op_transformed) =
                    composed_server_edit_ops.transform(&client_op)?;
                log::debug!("Transformed client operation: {client_op_transformed:?}");
                // The transformed client op is applicable to the current server doc state
                (
                    client_op_transformed.apply(current_server_doc_state.document())?,
                    Some(server_op_transformed),
                    client_op_transformed,
                )
            };

        // Transform input client `cursor_map` with server text transforms
        // Cannot use `current_server_doc_state.cursor_pos` because client may have changed cursor via non-text operations
        // so text op would be "retain all" and cursor change is only reflected in `cursor_map`.
        let transformed_user_cursor_map = match server_op_transformed {
            Some(server_op_transformed) => {
                // Applies the server transform to map client space cursor map into server space
                let mut transformed_user_cursor_map = FnvHashMap::default();
                for (user_id, cursor_pos) in cursor_map.iter() {
                    let new_cursor_pos = transform_cursor(&server_op_transformed, cursor_pos)?;
                    let _ = transformed_user_cursor_map.insert(*user_id, new_cursor_pos);
                }
                transformed_user_cursor_map
            }
            None => cursor_map.clone(),
        };

        // Apply the user transformation to all server cursors
        let mut new_cursor_map = transform_cursor_map(
            &client_op_applied,
            &self.current_document_state().cursor_map,
        )?;
        // While the user sends cursors of all users, only the current user cursor is taken from the user cursor map.
        // This is for correctness, since the user may have only changed cursor location, so a text op would be "retain all" and
        // the cursor change is only reflected in the user cursor map.
        //
        // This is more secure so the only the original user can change their cursor position in the server.
        // This may also avoid edge case where a user has an out of date set of collaborators' cursors and
        // overrides the server values when they were previously deleted.
        let _ = new_cursor_map.insert(user_id, *transformed_user_cursor_map.get(&user_id).unwrap());

        self.current_state_id += 1;
        // Perf note: this requires an allocation of a new cursor hashmap each time
        let new_server_doc_state = DocumentState::new(
            self.current_state_id,
            new_server_doc_state,
            client_op_applied.clone(),
            new_cursor_map.clone(),
        );
        self.document_states
            .insert(self.current_state_id, new_server_doc_state);

        Ok((client_op_applied, new_cursor_map))
    }

    pub fn add_user(&mut self, user: User) -> Result<(), ServerError> {
        if self.users.contains_key(&user.user_id()) {
            return Err(ServerError::DuplicateUserId(user.user_id()));
        }
        self.users.insert(user.user_id(), user);
        Ok(())
    }

    // Composes the text operations between two valid server state ids (`(start, end]`) into a single text operation.
    // This is not inclusive of `start` because the text op of the document state represents the transformation from the
    // previous state, so to transform from `start` to `end` we do not need the text op which transforms into `start`.
    // This represents the operation that transforms a server state `start` to server state `end`.
    fn compose_text_ops_between_states(
        &self,
        start: StateId,
        end: StateId,
    ) -> Result<TextOperation, ServerError> {
        // Start cannot be equal to end, as the range is exclusive. This means the states are equal and the text op
        // transformation would be a no-op retain.
        if start >= end {
            return Err(ServerError::InvalidStateIdRange { start, end });
        }

        let mut composed_text_op = self
            .document_states
            .get(&(start + 1))
            .ok_or(ServerError::StateIdNotFound(start))?
            .text_op
            .clone();

        for state_id in (start + 2)..=end {
            let doc_state = self
                .document_states
                .get(&state_id)
                .ok_or(ServerError::StateIdNotFound(state_id))?;
            composed_text_op = composed_text_op.compose(&doc_state.text_op)?;
        }
        Ok(composed_text_op)
    }

    /// Returns a unique user id of the next user.
    pub fn next_user_id(&mut self) -> UserId {
        let next_id = self.next_id;
        self.next_id += 1;
        next_id
    }

    pub fn current_state_id(&self) -> StateId {
        self.current_state_id
    }

    pub fn current_document_state(&self) -> &DocumentState {
        // `current_state_id` will exist in the document states map
        self.document_states.get(&self.current_state_id).unwrap()
    }

    pub fn users(&self) -> &FnvHashMap<UserId, User> {
        &self.users
    }

    pub fn users_mut(&mut self) -> &mut FnvHashMap<UserId, User> {
        &mut self.users
    }
}

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("State ID not found: {0}")]
    StateIdNotFound(StateId),
    #[error("Start state ID must be less than end state ID: {start} >= {end}")]
    InvalidStateIdRange { start: StateId, end: StateId },
    #[error(transparent)]
    TextOperationError(#[from] TextOperationError),
    #[error(transparent)]
    CursorTransformError(#[from] CursorTransformError),
    #[error("User ID already taken: {0}")]
    DuplicateUserId(UserId),
}

pub struct ServerNetwork {
    inner: Server,
    // Set of shared state from the network
    network_shared: NetworkShared,
    // Component ID
    id: ComponentId,
}

impl ServerNetwork {
    pub fn new(network: &mut Network) -> Self {
        let id = network.next_id();
        let network_shared = network.network_shared();
        ServerNetwork {
            inner: Server::new(),
            network_shared,
            id,
        }
    }
    /// Sends a transformation ([`TextOperation`]) to all clients on the network
    pub fn broadcast(&mut self, op: TextOperation, cursor_map: CursorMap) {
        let client_ids = self.network_shared.client_ids();
        let client_ids = client_ids.borrow_mut();
        let server_id = self.id();
        for client_id in client_ids.iter() {
            Network::schedule_remote_message(
                self.network_shared.clock(),
                self.network_shared.events(),
                self.network_shared.component_metadata(),
                RemoteUpdate {
                    source: server_id,
                    dest: *client_id,
                    state_id: self.current_state_id,
                    operation: op.clone(),
                    cursor_map: cursor_map.clone(),
                },
            );
        }
    }
}

impl Component for ServerNetwork {
    fn remote_update(&mut self, message: RemoteUpdate) -> Result<()> {
        // All incoming messages must be derived from prior server states
        debug_assert!(message.state_id <= self.current_state_id);
        let (network_message, cursor_map) = self.apply_client_operation(
            message.operation,
            message.state_id,
            &message.cursor_map,
            message.source,
        )?;
        self.broadcast(network_message, cursor_map);
        Ok(())
    }

    fn recv_snapshot(&mut self, _snapshot: Snapshot) {
        panic!("Server should not receive snapshots");
    }

    fn local_update(&mut self, _: LocalMessage) {
        panic!("Server should not receive local updates");
    }

    fn new_client(&mut self, client_id: ComponentId, late_joiner: bool) {
        let current_doc_state = self.current_document_state();
        let snapshot = Snapshot {
            source: self.id(),
            dest: client_id,
            document: current_doc_state.document().to_string(),
            cursor_map: current_doc_state.cursor_map().clone(),
            state_id: self.current_state_id,
        };

        // Username and color are not assigned for the testing network
        // unwrap: testing network will not create clients with duplicate ids
        let activity = Activity {
            active: true,
            last_activity: std::time::Instant::now(),
        };
        self.add_user(User::new(
            client_id,
            "".to_string(),
            "".to_string(),
            activity,
        ))
        .unwrap();

        Network::schedule_snapshot(
            self.network_shared.clock(),
            self.network_shared.events(),
            self.id(),
            client_id,
            self.network_shared.component_metadata(),
            snapshot,
            late_joiner,
        );
    }

    fn id(&self) -> ComponentId {
        self.id
    }

    fn kind(&self) -> ComponentKind {
        ComponentKind::Server
    }

    fn document(&self) -> &str {
        self.document_states
            .get(&self.current_state_id)
            .unwrap()
            .document()
    }

    /// The server has one cursor map for each state ID. Returns only the cursor map fo the current server state.
    fn cursor_pos(&self, user_id: UserId) -> Option<CursorPos> {
        self.document_states
            .get(&self.current_state_id)
            .unwrap()
            .cursor_map()
            .get(&user_id)
            .copied()
    }
}

impl Deref for ServerNetwork {
    type Target = Server;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for ServerNetwork {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
