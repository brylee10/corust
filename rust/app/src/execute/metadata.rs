//! Utility for getting metadata about the server
//! Currently gets rustc versions for various channels.

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use corust_types::container::Versions;
use thiserror::Error;

use crate::{
    AppState,
    sandbox_metadata::{SandboxMetadataError, SharedSandboxMetadata},
};

use super::runner::SharedContainerFactory;

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error(transparent)]
    SandboxMetadataError(#[from] SandboxMetadataError),
}

async fn get_versions(
    sandbox_metadata: SharedSandboxMetadata,
    container_factory: SharedContainerFactory,
) -> Result<Versions, MetadataError> {
    Ok(sandbox_metadata.versions(&container_factory).await?)
}

pub async fn metadata(
    State(state): State<AppState>,
) -> Result<Json<Versions>, (StatusCode, String)> {
    match get_versions(
        Arc::clone(&state.sandbox_metadata),
        Arc::clone(&state.container_factory),
    )
    .await
    {
        Ok(versions) => Ok(Json(versions)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}
