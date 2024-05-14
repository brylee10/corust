use corust_components::{network::RemoteUpdate, server::Server};
use corust_transforms::xforms::TextOperation;
use serde::{Deserialize, Serialize};
use std::{process::Output, sync::Arc};
use tokio::sync::Mutex;
use wasm_bindgen::prelude::*;

use super::CursorMap;

// Server side in memory storage of most recent code and run output for one session
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct AppState {
    pub last_code: CodeContainer,
    pub last_run: Option<RunnerOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Message types that can be broadcast via internal server broadcast channel
pub enum ServerMessage {
    RemoteUpdate(RemoteUpdate),
    Compile(RunnerOutput),
}

pub type SharedAppState = Arc<Mutex<AppState>>;

pub type SharedServer = Arc<Mutex<Server>>;

// API for code changes
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct CodeContainer {
    pub code: String,
}

// API for execution output
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct RunnerOutput {
    stdout: String,
    stderr: String,
    status: i32,
}

impl RunnerOutput {
    pub fn new(stdout: String, stderr: String, status: i32) -> Self {
        RunnerOutput {
            stdout,
            stderr,
            status,
        }
    }

    pub fn stdout(&self) -> &str {
        &self.stdout
    }

    pub fn stderr(&self) -> &str {
        &self.stderr
    }

    pub fn status(&self) -> i32 {
        self.status
    }
}

impl From<Output> for RunnerOutput {
    fn from(output: Output) -> Self {
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let status = output
            .status
            .code()
            .unwrap_or_else(|| panic!("Error getting status code from output: {}", output.status));

        RunnerOutput {
            stdout,
            stderr,
            status,
        }
    }
}
