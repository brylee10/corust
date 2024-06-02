//! Client to track document updates.
//!
//! Includes a general [`Client`] with core state tracking and transform operations which is compiled into WebAssembly, and another [`ClientNetwork`]
//! which is a wrapper around the [`Client`] that can be added to a [`Network`] for testing.
//!
//! Notation:
//! The server will send a series of updates to the client, `[s1, s2, s3, ... sn]`.
//! After each of these updates, the server state is `[S1, S2, S3, ... Sn]`.
//!
//! This client will also be generating a series of updates, `[c1, c2, c3, ... cm]`.
//!
//! Clients only have one outstanding operation with the server at a time. This way, client operations always branch from a point in server history
//! so the server only needs to apply one iteration of OT per client operation and so the server needs to cache no additional state aside from the server's
//! own update history.

#[cfg(debug_assertions)]
use crate::network::sanity_check_overlapping_keys_match;
use anyhow::Result;

#[cfg(feature = "js")]
use crate::web_utils::{self, debug};
use crate::{
    network::{
        transform_cursor, transform_cursor_map, CursorMap, CursorPos, TextOpAndCursorMap, UserId,
    },
    ClientResponse, ClientResponseData, ClientResponseType, RemoteDocUpdate,
};
use crate::{
    network::{
        Component, ComponentId, ComponentKind, LocalMessage, Network, NetworkShared, RemoteUpdate,
    },
    server::StateId as ServerStateId,
    ServerMessage,
};
use crate::{BroadcastLocalDocUpdate, Snapshot};
use corust_transforms::xforms::{
    self, text_operation_text_updates, text_update_from_doc, TextOperation,
};
use corust_transforms::{ops, xforms::TextUpdate};
use std::collections::hash_map::Entry;
use std::{
    collections::VecDeque,
    ops::{Deref, DerefMut},
};
use thiserror::Error;
use wasm_bindgen::prelude::*;

/// Wrapper around a [`Client`] that can be added to a [`Network`]. Used in testing.
pub struct ClientNetwork {
    inner: Client,
    // Shared network state
    network_shared: NetworkShared,
    // Unique component id
    id: ComponentId,
}

impl ClientNetwork {
    /// Constructs a new client network component. The network is only used to assign a unique ID but the component
    /// is not immediately added to the network. This allows for late joiners:
    /// TODO: Check if that last statement is true. Maybe you could `add_component` here and for late joiners just
    /// construct them later.
    pub fn new(network: &mut Network) -> Self {
        let id = network.next_id();
        let network_shared = network.network_shared();
        Self {
            inner: Client::new(id),
            network_shared,
            id,
        }
    }

    // Sends a operation to the server
    fn schedule_remote_message(
        &self,
        operation: TextOperation,
        cursor_map: CursorMap,
        state_id: ServerStateId,
    ) {
        // A server ID will be assigned by this point after network is running
        let server_id = self.network_shared.server_id().borrow().unwrap();
        Network::schedule_remote_message(
            self.network_shared.clock(),
            self.network_shared.events(),
            self.network_shared.component_metadata(),
            RemoteUpdate {
                source: self.id(),
                dest: server_id,
                operation,
                cursor_map,
                state_id,
            },
        );
    }
}

impl Deref for ClientNetwork {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for ClientNetwork {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

#[derive(Error, Debug)]
pub enum ClientError {
    #[error(transparent)]
    CursorError(#[from] anyhow::Error),
    #[error(
        "Unexpected cursor input. Expected cursor after transform is {expected:?} but received cursor {received:?}"
    )]
    UnexpectedCursor {
        expected: CursorPos,
        received: CursorPos,
    },
}

impl From<ClientError> for JsValue {
    fn from(val: ClientError) -> Self {
        let error_message = val.to_string();
        JsValue::from_str(&error_message)
    }
}
#[wasm_bindgen]
/// Standalone client with core operations and state for client document management with operational transform.
#[derive(Default)]
pub struct Client {
    document: String,
    // Starts at 0, represents the empty document state
    last_server_state_id: ServerStateId,
    // Let the last server update received by the client be `s_i` (the server state at time of `s_i` generation was `S_i`).
    // After applying the transformation of this operation (`s_i'`) locally, let the client's state be called `c_j`. The `client_bridge`
    // represents the series of operations that transforms the server state `S_i` to the client state `c_j`.
    //
    // OT transforms two different series of operations which began common starting state. The bridge is needed to use `S_i` as the starting state to
    // apply a transform of the future server update `s_{i+1}` to the client.
    //
    // Only bridge operations are sent to the server because they are properly transformed to branch from a current/historic server state.
    client_bridge: VecDeque<ClientOperation>,
    // Client user id
    user_id: UserId,
    // Map of all client cursors
    cursor_map: CursorMap,
}

#[wasm_bindgen]
impl Client {
    pub fn new(user_id: UserId) -> Self {
        // Forward Rust panics to JS console as errors
        console_error_panic_hook::set_once();

        Self {
            document: String::new(),
            last_server_state_id: 0,
            client_bridge: VecDeque::new(),
            user_id,
            // Default cursor map means no cursors are present on the screen,
            // including the current user
            cursor_map: CursorMap::default(),
        }
    }

    /// Represents the resulting document and cursor position after a client shifts their cursor/highlight or
    /// updates the text in a document with a transformation represented by [`TextUpdate`].
    ///
    /// To maintain the "maximal responsiveness" property, local updates are always allowed without latency.
    ///
    /// Returns a JSON string serialization of a [`BroadcastLocalDocUpdate`] which represents the text transformation turning
    /// the current document into the new document with the new cursor state.
    ///
    /// Single outstanding update property: The client only has one un-acked update at a time.
    /// This way, the server only needs to apply one iteration of OT per client operation.
    pub fn update_document_wasm(
        &mut self,
        new_document: &str,
        prev_doc_len: usize,
        text_updates: Vec<TextUpdate>,
        new_cursor_pos: &CursorPos,
    ) -> Result<String, ClientError> {
        let text_op = xforms::text_updates_to_text_operation(&text_updates, prev_doc_len);
        let text_op_and_cursor_map = self.update_document(new_document, text_op, new_cursor_pos)?;
        let doc_update = BroadcastLocalDocUpdate::new(
            text_op_and_cursor_map.text_op,
            self.last_server_state_id,
            text_op_and_cursor_map.cursor_map,
            self.user_id,
        );
        Ok(serde_json::to_string(&doc_update).unwrap())
    }

    /// Returns a bool indicating if the client should send its local update to the server.
    /// If the client has only one operation in the bridge, this is true, and the front operation is marked as [`OpState::Pending`].
    /// Otherwise, the client should buffer updates so this returns false.
    pub fn prepare_send_local_update(&mut self) -> bool {
        if self.client_bridge.len() == 1 {
            self.client_bridge.front_mut().unwrap().state = OpState::Pending;
            return true;
        }
        false
    }

    /// Updates client state based on a string which is assumed to be a JSON serialization of a `ServerMessage`.
    /// Returns a serialized doc update if there is a new local operation to send the server.
    /// This only occurs if `server_msg` is an ack to an buffered client operation.
    /// Otherwise returns `None`, indicating Client internal state is updated, but no operation should be sent across the WS.
    /// Note: JS cannot directly pass a `ServerMessage` because it is an enum with data, which is currently not supported by `wasm_bindgen`.
    ///
    /// This method is only called via WASM via JS.
    /// TODO: What to do about methods like this which are only called in JS but exposed publicly? `log` will not link against a proper JS function if called in Rust.
    // Allow clippy lint because not simple `let and return` when compiled with `metrics`
    #[allow(clippy::let_and_return)]
    pub fn handle_server_message(&mut self, server_msg: &str) -> Option<ClientResponse> {
        #[cfg(all(feature = "metrics", feature = "js"))]
        let start = web_utils::now(); // Start timing

        #[cfg(all(feature = "metrics", feature = "js"))]
        web_utils::log(&format!("Client received server message: {}", server_msg));
        let server_msg: ServerMessage = serde_json::from_str(server_msg).unwrap();
        // Prod sim difference: prod uses more triggered client updates (like `UserList`)
        let triggered_client_update = self.handle_server_message_inner(&server_msg);
        #[cfg(all(feature = "metrics", feature = "js"))]
        web_utils::log(&format!(
            "Client handle_server_message took: {}ms",
            web_utils::now() - start
        ));
        triggered_client_update
    }

    pub fn last_server_state_id(&self) -> ServerStateId {
        self.last_server_state_id
    }

    /// Number of updates that have not been acked by the server (all except the first are unsent).
    pub fn buffer_len(&self) -> usize {
        self.client_bridge.len()
    }

    /// Get current client document
    pub fn document(&self) -> String {
        self.document.clone()
    }

    /// Get current client cursor position
    pub fn cursor_pos(&self) -> Option<CursorPos> {
        // Returns option since user would not have if they have not focused on the code section yet
        self.cursor_map.get(&self.user_id).copied()
    }

    /// Get all cursor positions in the cursor map except the current user.
    /// This is used to update the UI with the cursor positions of other users.
    /// Returns a structure iterable by JS.
    pub fn cursor_pos_vec(&self) -> Vec<UserCursorPos> {
        self.cursor_map
            .iter()
            .filter(|(id, _cursor_pos)| **id != self.user_id)
            .map(|(id, cursor_pos)| UserCursorPos {
                user_id: *id,
                cursor_pos: *cursor_pos,
            })
            .collect()
    }

    /// Get the user id of the client
    pub fn user_id(&self) -> UserId {
        self.user_id
    }
}

impl Client {
    /// Unified handler for a server message in local simulation and in the WASM client
    /// for increased test coverage.
    fn handle_server_message_inner(
        &mut self,
        server_msg: &ServerMessage,
    ) -> Option<ClientResponse> {
        let triggered_client_update = match &server_msg {
            ServerMessage::RemoteUpdate(remote_update) => {
                if self.try_ack_client_op(remote_update) {
                    self.advance_client_bridge().map(|client_op| {
                        // Use remote update state ID because `update_last_server_state_id`
                        // has not been called yet
                        let doc_update = BroadcastLocalDocUpdate::new(
                            client_op.bridge_op,
                            remote_update.state_id,
                            client_op.cursor_map,
                            self.user_id,
                        );
                        ClientResponse::new(
                            ClientResponseType::BroadcastLocalDocUpdate,
                            ClientResponseData::BroadcastLocalDocUpdate(doc_update),
                        )
                    })
                } else {
                    let server_op = self.apply_server_message(remote_update);
                    let text_updates = text_operation_text_updates(&server_op);
                    Some(ClientResponse::new(
                        ClientResponseType::RemoteDocUpdate,
                        ClientResponseData::RemoteDocUpdate(RemoteDocUpdate::new(text_updates)),
                    ))
                }
            }
            ServerMessage::Run(_) => None,
            ServerMessage::Snapshot(snapshot) => {
                self.document = snapshot.document.to_string();
                self.cursor_map = snapshot.cursor_map.clone();
                let text_updates = text_update_from_doc(&snapshot.document);
                Some(ClientResponse::new(
                    ClientResponseType::RemoteDocUpdate,
                    ClientResponseData::RemoteDocUpdate(RemoteDocUpdate::new(text_updates)),
                ))
            }
            ServerMessage::UserList(user_list) => {
                // `UserList` is only used for UI displays and does not affect the client state
                Some(ClientResponse::new(
                    ClientResponseType::UserList,
                    ClientResponseData::UserList(user_list.clone()),
                ))
            }
        };
        self.update_last_server_state_id(server_msg);
        triggered_client_update
    }
    /// Checks if the `server_msg` broadcast is an `ACK` for the outstanding client message. If so, the pending
    /// client operation is marked as [`OpState::Acked`] and this function returns `true`. Otherwise, returns `false`.
    fn try_ack_client_op(&mut self, server_msg: &RemoteUpdate) -> bool {
        // Client receives each server message sequentially, so server state should increment by 1 each time
        debug_assert!(server_msg.state_id == self.last_server_state_id + 1);
        #[cfg(feature = "js")]
        debug(&format!(
            "Client received server state: {}, msg_uuid: {}",
            server_msg.state_id,
            server_msg.operation.id(),
        ));

        let mut applied_client_op = false;
        if let Some(pending_client_update) = self.client_bridge.front_mut() {
            if pending_client_update.bridge_op.id() == server_msg.operation.id() {
                debug_assert!(pending_client_update.state == OpState::Pending);
                // Any operations of this client the server applies are in the bridge
                if pending_client_update.bridge_op != server_msg.operation {
                    #[cfg(feature = "js")]
                    debug(&format!(
                        "Client operation: {:?}, Server operation: {:?}",
                        pending_client_update.bridge_op, server_msg.operation
                    ));
                    panic!("Client operation does not match server operation");
                }

                // Server has applied the client's operation
                pending_client_update.state = OpState::Acked;
                applied_client_op = true;
            }
        }

        applied_client_op
    }

    /// Pops the top element from the `client_bridge` and returns the [`ClientOperation`] of the next element and marks
    /// the element state as [`OpState::Pending`], if such an element exists. This effectively advances the bridge to
    /// the next operation. This is used after the previous operation was `ACK`ed.
    ///
    /// Returns: `None` if there are no buffere bridge elements. Otherwise, returns the text operation of the next
    /// element.
    fn advance_client_bridge(&mut self) -> Option<ClientOperation> {
        // Remove the previously acked operation from the bridge
        if let Some(front_element) = self.client_bridge.pop_front() {
            debug_assert!(front_element.bridge_op.id() == front_element.original_op.id());
            // `advance` should only be called when the front operation is acked
            debug_assert!(front_element.state == OpState::Acked);
            #[cfg(feature = "js")]
            debug(&format!(
                "Client operation acked bridge op id: {:?}",
                front_element.bridge_op.id()
            ));
        } else {
            return None;
        }

        // Mark the next operation as pending. The `TextOperation` should be sent to the server after.
        if let Some(new_front_client_op) = self.client_bridge.front_mut() {
            new_front_client_op.state = OpState::Pending;
            return Some(new_front_client_op.clone());
        }
        None
    }

    /// Called after a new server message is received.
    fn update_last_server_state_id(&mut self, server_msg: &ServerMessage) {
        match server_msg {
            ServerMessage::RemoteUpdate(remote_update) => {
                // Client receives each server message sequentially, so server state should increment by 1 each time
                debug_assert!(remote_update.state_id == self.last_server_state_id + 1);
                self.last_server_state_id = remote_update.state_id;
            }
            ServerMessage::Run(_) => {}
            ServerMessage::Snapshot(snapshot) => {
                // Snapshots should occur when the client is a late joiner
                debug_assert!(self.last_server_state_id == 0);
                #[cfg(feature = "js")]
                debug(&format!(
                    "Client received snapshot with state id: {}",
                    snapshot.state_id
                ));
                self.last_server_state_id = snapshot.state_id;
            }
            ServerMessage::UserList(_) => {
                // UserList does represent a doc or cursor map update so it does not hold a state ID
            }
        }
    }

    /// If server message is not an `ACK` of a client message, then the server message is a new operation and is applied locally, transforming the
    /// bridge with respect to the new server operation.
    ///
    /// Returns a [`TextOperation`] representing the transformed server operation that was applied to the document and cursor map.
    fn apply_server_message(&mut self, server_msg: &RemoteUpdate) -> TextOperation {
        #[cfg(all(feature = "metrics", feature = "js"))]
        let start = web_utils::now(); // Start timing

        // If the server or any client bridge operation is a cursor only update, then the overlapping keys sanity check should not be run.
        let mut has_cursor_only_update = server_msg.operation.noop();
        // The operation is from a different client, so apply to this client.
        let mut server_op = server_msg.operation.clone();
        let mut server_cursor_map = server_msg.cursor_map.clone();
        // Update the client bridge
        let mut new_bridge = VecDeque::new();

        // Replace the client bridge with updated transformed operations
        let prev_client_bridge = std::mem::take(&mut self.client_bridge);
        for client_op in prev_client_bridge {
            has_cursor_only_update = has_cursor_only_update || client_op.original_op.noop();
            // The `transform` only fails if the operations apply to inputs of different lengths, which should never happen.
            // Panic is acceptable if this happens.
            let (server_op_transformed, client_op_transformed) =
                server_op.transform(&client_op.bridge_op).unwrap();

            // TODO: Check this unwrap, mainly for sanity check, unsure what to do if it fails
            // CAN REMOVE: Sanity check (which may not hold) - is the client cursor map with the transformed server op
            // the same as the server cursor map with teh transformed client op? It's possible they do not converge.
            let transformed_client_cursor_map =
                transform_cursor_map(&server_op_transformed, &client_op.cursor_map)
                    .expect("Client cursor map transform failed");
            server_cursor_map = transform_cursor_map(&client_op_transformed, &server_cursor_map)
                .expect("Server cursor map transform failed");

            // Keep user cursor since user may have shifted their cursor, or their cursor is not yet registered with the server
            if let Some(cursor_pos) = transformed_client_cursor_map.get(&self.user_id) {
                server_cursor_map.insert(self.user_id, *cursor_pos);
            }

            // Should hold if the text operation is not all retain (i.e. only a cursor update).
            // This should hold if a text operation caused the cursor shift, not only a cursor update.
            if !has_cursor_only_update {
                #[cfg(debug_assertions)]
                sanity_check_overlapping_keys_match(
                    &server_cursor_map,
                    &transformed_client_cursor_map,
                )
                .unwrap();
            }
            // This assertion is invalid because `server` map may have added/removed users which the client previously did not have
            // debug_assert!(transformed_client_cursor_map == server_cursor_map);

            new_bridge.push_back(ClientOperation {
                original_op: client_op.original_op,
                bridge_op: client_op_transformed,
                // Assigns the server map instead of client because server may have added/removed users which the client previously did not have
                cursor_map: server_cursor_map.clone(),
                state: client_op.state,
            });

            // Repeatedly transforms the server operation against the bridge. The last `server_op` can be applied to the current client state.
            server_op = server_op_transformed;
        }
        self.client_bridge = new_bridge;

        // Apply the transformed server operation to the client document
        // The `apply` only fails if the `server_op` length is not applicable to the client's document, which should never happen.
        let new_document = server_op.apply(&self.document).unwrap();
        log::debug!(
            "Old document: {}, New document: {}",
            &self.document,
            new_document
        );
        self.document = new_document;

        // Requirement 1: Transform the client map with the server op
        // Apply the same update to all cursor positions
        let transformed_client_cursor_map = transform_cursor_map(&server_op, &self.cursor_map)
            .expect("Client cursor map transform failed");
        // Requirement 2: Transform the server map with the client bridge (this is done above)
        // Requirement 3: Update the client cursor map, taking all cursors from the server map
        // except the client's itself, which uses the user cursor map
        let mut new_cursor_map = server_cursor_map;
        // Keep user cursor since user may have shifted their cursor, or their cursor is not yet registered with the server
        if let Some(cursor_pos) = transformed_client_cursor_map.get(&self.user_id) {
            new_cursor_map.insert(self.user_id, *cursor_pos);
        }
        log::debug!(
            "Old cursor map: {:?}, New cursor map: {:?}",
            &self.cursor_map,
            new_cursor_map
        );

        // This only applies if the the server operation was not a cursor only update.
        if !has_cursor_only_update {
            #[cfg(debug_assertions)]
            sanity_check_overlapping_keys_match(&new_cursor_map, &transformed_client_cursor_map)
                .unwrap();
        }

        self.cursor_map = new_cursor_map;

        #[cfg(all(feature = "metrics", feature = "js"))]
        web_utils::log(&format!(
            "Client apply_server_message took: {}ms",
            web_utils::now() - start
        ));

        server_op
    }

    // `text_op` applied to the current document gives the new document
    pub(crate) fn update_document(
        &mut self,
        new_document: &str,
        text_op: TextOperation,
        new_cursor_pos: &CursorPos,
    ) -> Result<TextOpAndCursorMap> {
        #[cfg(all(feature = "metrics", feature = "js"))]
        let start = web_utils::now(); // Start timing

        let new_document = new_document.to_string();

        #[cfg(all(feature = "metrics", feature = "js"))]
        let pre_edit_ops = web_utils::now();
        #[cfg(all(feature = "metrics", feature = "js"))]
        web_utils::debug(&format!(
            "Client update_document finished edit_ops: {}ms",
            web_utils::now() - pre_edit_ops
        ));
        self.document = new_document;

        // Treat this user's cursor as a special case to allow sanity check that `input_cursor == transformed_cursor`
        let this_cursor = self.cursor_map.entry(self.user_id);
        match this_cursor {
            Entry::Vacant(e) => {
                e.insert(*new_cursor_pos);
            }
            Entry::Occupied(mut e) => {
                let this_cursor = e.get_mut();
                // This assumption is not correct: for example, `|` -> `(|)`. The cursor is moved INTO the parentheses and not
                // `()|` as this sanity check assumes.
                // // The transformed cursor position should be the same as the user input **if the operation is not a noop**,
                // // i.e. exclusively a cursor update like a change position or rehighlight.
                // let transformed_cursor_pos = transform_cursor(&text_op, this_cursor)?;
                // #[cfg(feature = "js")]
                // web_utils::debug(&format!(
                //     "This cursor: {:?}, Transformed cursor: {:?}, New Cursor: {:?}, Text Op: {:?}",
                //     this_cursor, transformed_cursor_pos, new_cursor_pos, text_op
                // ));
                // if !text_op.noop() && transformed_cursor_pos != *new_cursor_pos {
                //     // Encountering this error puts the client in an inconsistent state
                //     // because no transformed text operation is returned
                //     return Err(ClientError::UnexpectedCursor {
                //         expected: transformed_cursor_pos,
                //         received: *new_cursor_pos,
                //     })?;
                // }
                *this_cursor = *new_cursor_pos;
            }
        }

        // Apply transform to all other users' cursor positions
        for (_, cursor_pos) in self
            .cursor_map
            .iter_mut()
            .filter(|(id, _)| *id != &self.user_id)
        {
            #[cfg(feature = "js")]
            web_utils::debug(&format!(
                "Transforming cursor: {:?}, Text Op: {:?}",
                cursor_pos, text_op
            ));
            let transformed_cursor_pos = transform_cursor(&text_op, cursor_pos)?;
            *cursor_pos = transformed_cursor_pos;
        }

        self.client_bridge.push_back(ClientOperation {
            original_op: text_op.clone(),
            bridge_op: text_op.clone(),
            cursor_map: self.cursor_map.clone(),
            state: OpState::Unsent,
        });

        #[cfg(all(feature = "metrics", feature = "js"))]
        web_utils::debug(&format!(
            "Client update_document took: {}ms",
            web_utils::now() - start
        ));

        Ok(TextOpAndCursorMap {
            text_op,
            cursor_map: self.cursor_map.clone(),
        })
    }
}

impl Component for ClientNetwork {
    // Represents a client receiving an update event from the network from the server
    fn remote_update(&mut self, remote_update: RemoteUpdate) -> Result<()> {
        let state_id = remote_update.state_id;
        let server_msg = ServerMessage::RemoteUpdate(remote_update);
        let next_op = self.handle_server_message_inner(&server_msg);
        if let Some(next_op) = next_op {
            if let Some(broadcast_doc_update) = next_op.get_broadcast_doc_update() {
                let broadcast_doc_update: BroadcastLocalDocUpdate =
                    serde_json::from_str(&broadcast_doc_update).unwrap();
                // Clones not necessary, could move out of `broadcast_doc_update` if fields were `pub(crate)`
                self.schedule_remote_message(
                    broadcast_doc_update.text_operation().clone(),
                    broadcast_doc_update.cursor_map().clone(),
                    state_id,
                );
            }
        }
        Ok(())
    }

    fn recv_snapshot(&mut self, snapshot: Snapshot) {
        let server_msg = ServerMessage::Snapshot(snapshot);
        self.handle_server_message_inner(&server_msg);
    }

    fn local_update(&mut self, message: LocalMessage) {
        debug_assert!(message.client_id == self.id());
        // NOTE: Local tests use edit distance as a heuristic to generate text changes. This simplifies tests so text operations do not need
        // to be explicitly listed. Edit distance does not guarantee a unique transformation from one document to another, but test case strings
        // give a unique transformation (e.g. "aa" -> "aaa" could be due to insertion at any of 3 locations).
        let text_ops = ops::edit_ops(&self.document, &message.document);
        let text_op = TextOperation::from_ops(text_ops.into_iter(), None, false);
        // Unwraps error since `local_update` is infalliable. The client is in an inconsistent state if an error is returned, so
        // better to panic.
        let text_op_and_cursor_map = self
            .update_document(&message.document, text_op, &message.cursor_pos)
            .unwrap();
        let TextOpAndCursorMap {
            text_op: operation,
            cursor_map,
        } = text_op_and_cursor_map;
        // Single outstanding update property: Send the operation immediately to the server if it is the only operation in the bridge
        if self.prepare_send_local_update() {
            self.schedule_remote_message(operation, cursor_map, self.last_server_state_id);
        }
    }

    fn new_client(&mut self, _client_id: ComponentId, _late_joiner: bool) {
        // No-op for clients
        panic!("ClientNetwork should not receive new_client messages");
    }

    fn kind(&self) -> ComponentKind {
        ComponentKind::Client
    }

    fn id(&self) -> ComponentId {
        self.id
    }

    fn document(&self) -> &str {
        &self.document
    }

    fn cursor_pos(&self, user_id: UserId) -> Option<CursorPos> {
        self.cursor_map.get(&user_id).copied()
    }
}

// Light wrapper around the `TextOperation` to track the state of the operation's application to the server
// The `operation` is never a transformed operation (i.e. it is always a client-generated operation).
#[derive(Clone, Debug)]
struct ClientOperation {
    // The (untransformed) operation applied to the client's document
    original_op: TextOperation,
    // The (possibly transformed) version of original operation to be applied to the server's document.
    // Bridge operations can always be applied in sequence to transform a historical server state to the client's current state.
    bridge_op: TextOperation,
    // Cursor map *after* the applied operation
    cursor_map: CursorMap,
    // The state of this operation's application to the server
    state: OpState,
}

/// [`ClientOperation`] which can be sent over the JS WASM boundary. Uses `JsValue` in place of complex types.
#[wasm_bindgen]
#[derive(Clone, Debug)]
pub struct ClientOpJs {
    original_op: JsValue,
    bridge_op: JsValue,
    cursor_map: JsValue,
    state: OpState,
}

impl TryFrom<ClientOperation> for ClientOpJs {
    type Error = serde_wasm_bindgen::Error;

    fn try_from(client_op: ClientOperation) -> Result<Self, Self::Error> {
        Ok(Self {
            original_op: serde_wasm_bindgen::to_value(&client_op.original_op)?,
            bridge_op: serde_wasm_bindgen::to_value(&client_op.bridge_op)?,
            cursor_map: serde_wasm_bindgen::to_value(&client_op.cursor_map)?,
            state: client_op.state,
        })
    }
}

impl TryFrom<ClientOpJs> for ClientOperation {
    type Error = serde_wasm_bindgen::Error;

    fn try_from(client_op_js: ClientOpJs) -> Result<Self, Self::Error> {
        Ok(Self {
            original_op: serde_wasm_bindgen::from_value(client_op_js.original_op)?,
            bridge_op: serde_wasm_bindgen::from_value(client_op_js.bridge_op)?,
            cursor_map: serde_wasm_bindgen::from_value(client_op_js.cursor_map)?,
            state: client_op_js.state,
        })
    }
}

#[wasm_bindgen]
// Represents whether a client has sent an update to a server and, if so, received a confirmation the update was applied.
// Only used locally by clients to track operation state, so this enum is private.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum OpState {
    // Server has applied this operation, can be removed from operation buffer
    Acked,
    // Sent, but not acked by server
    Pending,
    Unsent,
}

/// A [`CursorPos`] with the corresponding User's user id.
#[wasm_bindgen]
pub struct UserCursorPos {
    user_id: UserId,
    cursor_pos: CursorPos,
}

#[wasm_bindgen]
impl UserCursorPos {
    pub fn user_id(&self) -> UserId {
        self.user_id
    }

    pub fn cursor_pos(&self) -> CursorPos {
        self.cursor_pos
    }
}
