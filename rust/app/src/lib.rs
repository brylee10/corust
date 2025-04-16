#![deny(dead_code)]

use std::{path::PathBuf, sync::Arc};

use axum::{http::StatusCode, response::IntoResponse};
use corust_sandbox::container::{ContainerFactory, DockerBackend};
use sandbox_metadata::SandboxMetadata;
use sessions::SharedSessionMap;

pub mod background;
pub mod cache;
pub mod db;
pub mod execute;
pub mod sandbox_metadata;
pub mod sessions;
pub mod users;
pub mod websocket;

/// Returns a "connected" HTML message at the root path to sanity check server network connectivity
pub async fn root_page() -> impl IntoResponse {
    "Corust server connected"
}

pub async fn handler_404() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "Not found")
}

pub async fn favicon() -> impl IntoResponse {
    // The route exists but there's no content
    // this is called when browser requests the favicon
    StatusCode::NO_CONTENT
}

#[derive(Clone, Debug)]
pub struct AppState {
    pub session_map: SharedSessionMap,
    pub container_factory: Arc<ContainerFactory<DockerBackend>>,
    pub sandbox_metadata: Arc<SandboxMetadata>,
    pub db_path: PathBuf,
}
