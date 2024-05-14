use std::sync::Arc;

use tokio::sync::Mutex;

use serde::{Deserialize, Serialize};

use corust_components::{server::Server, RunnerOutput};

// Server side in memory storage of most recent code and run output for one session
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct AppState {
    pub last_code: CodeContainer,
    pub last_run: Option<RunnerOutput>,
}

pub type SharedAppState = Arc<Mutex<AppState>>;

pub type SharedServer = Arc<Mutex<Server>>;

// API for code changes
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct CodeContainer {
    pub code: String,
}
