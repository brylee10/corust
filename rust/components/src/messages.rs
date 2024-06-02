use crate::network::{ComponentId, CursorMap, RemoteUpdate, UserId, UserList};
use crate::server::StateId as ServerStateId;
use wasm_bindgen::prelude::*;

use corust_transforms::xforms::{TextOperation, TextUpdate};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Represents a local document update, sent to the server
// Compatible with JS camelCase field names
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadcastLocalDocUpdate {
    // The text operation representing the local document update
    text_operation: TextOperation,
    // The last common server state that this update branches off of
    last_server_state_id: ServerStateId,
    // The new cursor map following this update
    cursor_map: CursorMap,
    // The user ID of the client that sent this update
    user_id: UserId,
}

impl BroadcastLocalDocUpdate {
    pub fn new(
        text_operation: TextOperation,
        last_server_state_id: ServerStateId,
        cursor_map: CursorMap,
        user_id: UserId,
    ) -> Self {
        BroadcastLocalDocUpdate {
            text_operation,
            last_server_state_id,
            cursor_map,
            user_id,
        }
    }

    pub fn text_operation(&self) -> &TextOperation {
        &self.text_operation
    }

    pub fn last_server_state_id(&self) -> ServerStateId {
        self.last_server_state_id
    }

    pub fn cursor_map(&self) -> &CursorMap {
        &self.cursor_map
    }

    pub fn user_id(&self) -> UserId {
        self.user_id
    }
}

// `BroadcastLocalDocUpdate` is always JSON serialized
impl FromStr for BroadcastLocalDocUpdate {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

/// Represents a local document update triggered by a remote client's update, applied to client state
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct RemoteDocUpdate {
    // The text updates representing the local document update
    text_updates: Vec<TextUpdate>,
}

impl RemoteDocUpdate {
    pub fn new(text_updates: Vec<TextUpdate>) -> Self {
        RemoteDocUpdate { text_updates }
    }
}

#[wasm_bindgen]
impl RemoteDocUpdate {
    pub fn text_updates(&self) -> Vec<TextUpdate> {
        self.text_updates.clone()
    }
}

// // Custom deserialization to map string serialization of field into Rust type
// // Generic deserialization function to handle stringified JSON fields
// fn deserialize_from_string<'de, T, D>(deserializer: D) -> Result<T, D::Error>
// where
//     T: DeserializeOwned,
//     D: Deserializer<'de>,
// {
//     let s = String::deserialize(deserializer)?;
//     serde_json::from_str(&s).map_err(serde::de::Error::custom)
// }

/// Represents the data type of the [`ClientResponse`], in response to a
/// server message
#[wasm_bindgen]
#[derive(Debug, Clone, Copy)]
pub enum ClientResponseType {
    /// A server message was an ack of a client update,
    /// causes the client to broadcast a new update.
    BroadcastLocalDocUpdate,
    /// A server message was a new document update from another client,
    /// triggers a local client document and cursor map update.
    /// A remote snapshot is also mapped to this type, since a snapshot is
    /// a special case of an entire document update ("insertion"),
    RemoteDocUpdate,
    /// A server message sends a new snapshot of the UserList, causes
    /// the client to update its local user list.
    UserList,
}

/// Client updates triggered by [`ServerMessage`]. Simple types that are WASM compatible
/// and can be used to update the client state and UI. Effectively recreates a tagged C enum with data.
#[wasm_bindgen]
#[derive(Debug)]
pub struct ClientResponse {
    message_type: ClientResponseType,
    data: ClientResponseData,
}

impl ClientResponse {
    pub fn new(message_type: ClientResponseType, data: ClientResponseData) -> Self {
        ClientResponse { message_type, data }
    }

    pub fn data(&self) -> &ClientResponseData {
        &self.data
    }
}

#[wasm_bindgen]
impl ClientResponse {
    pub fn message_type(&self) -> ClientResponseType {
        self.message_type
    }

    /// Deserializes data as `UserList` if the data type is [`ClientResponseType::UserList`].
    /// Otherwise returns None.
    pub fn get_user_list(&self) -> Option<UserList> {
        match self.message_type {
            ClientResponseType::UserList => self.data.user_list().cloned(),
            _ => None,
        }
    }

    /// Returns the `BroadcastLocalDocUpdate` data if the message type is [`ClientResponseType::BroadcastLocalDocUpdate`].
    /// The data is `BroadcastLocalDocUpdate` JSON stringified. Otherwise returns None.
    pub fn get_broadcast_doc_update(&self) -> Option<String> {
        match self.message_type {
            ClientResponseType::BroadcastLocalDocUpdate => {
                self.data
                    .broadcast_doc_update()
                    .map(|op: &BroadcastLocalDocUpdate| {
                        serde_json::to_string(op)
                            .expect("Error serializing BroadcastLocalDocUpdate")
                    })
            }
            _ => None,
        }
    }

    pub fn get_remote_doc_update(&self) -> Option<RemoteDocUpdate> {
        match self.message_type {
            ClientResponseType::RemoteDocUpdate => self.data.remote_doc_update().cloned(),
            _ => None,
        }
    }
}

/// Possible data values for [`ClientResponse`]
#[derive(Debug)]
pub enum ClientResponseData {
    BroadcastLocalDocUpdate(BroadcastLocalDocUpdate),
    RemoteDocUpdate(RemoteDocUpdate),
    UserList(UserList),
}

impl ClientResponseData {
    pub fn broadcast_doc_update(&self) -> Option<&BroadcastLocalDocUpdate> {
        match self {
            ClientResponseData::BroadcastLocalDocUpdate(op) => Some(op),
            _ => None,
        }
    }

    pub fn remote_doc_update(&self) -> Option<&RemoteDocUpdate> {
        match self {
            ClientResponseData::RemoteDocUpdate(op) => Some(op),
            _ => None,
        }
    }

    pub fn user_list(&self) -> Option<&UserList> {
        match self {
            ClientResponseData::UserList(list) => Some(list),
            _ => None,
        }
    }
}

/// Message types that can be broadcast via internal server broadcast channel
// Does not use `#[serde(tag = "type")]` due to error deserializing inner HashMap
// types where the key is not a String.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    RemoteUpdate(RemoteUpdate),
    Run(RunnerOutput),
    Snapshot(Snapshot),
    // Split into separate message so it is usable across snapshot, updates,
    // and pruning non-gracefully disconnected users. When paired with a snapshot
    // or state update, the UserList is sent AFTER
    UserList(UserList),
}

/// Message sent to late joiners to sync their document
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub source: ComponentId,
    pub dest: ComponentId,
    pub document: String,
    pub cursor_map: CursorMap,
    pub state_id: ServerStateId,
}

// API for execution output
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RunnerOutput {
    pub run_type: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

// impl RunnerOutput {
//     pub fn new(run_type: String, stdout: String, stderr: String, status: i32) -> Self {
//         RunnerOutput {
//             run_type,
//             stdout,
//             stderr,
//             status,
//         }
//     }

//     pub fn stdout(&self) -> &str {
//         &self.stdout
//     }

//     pub fn stderr(&self) -> &str {
//         &self.stderr
//     }

//     pub fn status(&self) -> i32 {
//         self.status
//     }

//     pub fn run_type(&self) -> &str {
//         &self.run_type
//     }

//     pub fn from_output(output: Output, run_type: String) -> Self {
//         RunnerOutput {
//             run_type,
//             stdout: String::from_utf8_lossy(&output.stdout).to_string(),
//             stderr: String::from_utf8_lossy(&output.stderr).to_string(),
//             status: output.status.code().unwrap_or_else(|| {
//                 panic!("Error getting status code from output: {}", output.status)
//             }),
//         }
//     }
// }
