//! Utility for getting metadata about the server
//! Currently gets rustc versions for various channels.

use corust_types::container::Versions;
use thiserror::Error;
use warp::Filter;

use crate::sandbox_metadata::{SandboxMetadataError, SharedSandboxMetadata};

use super::runner::SharedContainerFactory;

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error(transparent)]
    SandboxMetadataError(#[from] SandboxMetadataError),
}

impl warp::reject::Reject for MetadataError {}

/// Returns a filter for the metadata routes (currently only versions)
pub fn metadata_routes(
    sandbox_metadata: SharedSandboxMetadata,
    container_factory: SharedContainerFactory,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path("metadata")
        .and(warp::path("versions"))
        .and(warp::get())
        .and_then(move || {
            let sandbox_metadata = sandbox_metadata.clone();
            let container_factory = container_factory.clone();

            async move {
                match get_versions(sandbox_metadata.clone(), container_factory.clone()).await {
                    Ok(versions) => Ok(warp::reply::json(&versions)),
                    Err(e) => Err(warp::reject::custom(e)),
                }
            }
        })
}

async fn get_versions(
    sandbox_metadata: SharedSandboxMetadata,
    container_factory: SharedContainerFactory,
) -> Result<Versions, MetadataError> {
    Ok(sandbox_metadata.versions(&container_factory).await?)
}
