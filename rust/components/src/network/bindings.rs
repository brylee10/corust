//! This module contains the types which use WASM bindings for the network module.

use super::{ComponentId, CursorMap};
use crate::server::StateId as ServerStateId;
use corust_transforms::xforms::TextOperation;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct RemoteUpdate {
    pub source: ComponentId,
    pub dest: ComponentId,
    pub state_id: ServerStateId,
    #[wasm_bindgen(skip)]
    // Do not generate a default getter and setter. TextOperation is not Copy.
    pub operation: TextOperation,
    // Server cursor map at the time of the operation
    #[wasm_bindgen(skip)]
    pub cursor_map: CursorMap,
}

#[wasm_bindgen]
impl RemoteUpdate {
    #[wasm_bindgen(getter)]
    pub fn operation(&self) -> TextOperation {
        self.operation.clone()
    }

    #[wasm_bindgen(setter)]
    pub fn set_operation(&mut self, operation: TextOperation) {
        self.operation = operation
    }
}

/// Cannot be converted to a native JS type, but is serialized and deserialized via `JsValue`
#[derive(Debug, Serialize, Deserialize)]
pub struct TextOpAndCursorMap {
    pub text_op: TextOperation,
    pub cursor_map: CursorMap,
}
