//! API endpoints for querying sandbox metadata, like Rust versions

use std::{sync::Arc, time::Duration};

use corust_sandbox::container::{Backend, ContainerError, ContainerFactory, VersionsError};
use corust_types::container::Versions;
use thiserror::Error;

use crate::cache::CacheItem;

const ONE_HOUR: Duration = Duration::from_secs(60 * 60);
pub(crate) const SANDBOX_METADATA_TIME_TO_LIVE: Duration = ONE_HOUR;

pub type SharedSandboxMetadata = Arc<SandboxMetadata>;

#[derive(Debug, Error)]
pub enum SandboxMetadataError {
    #[error("Error retrieving the sandbox channel version")]
    RetrievingChannels,
    #[error(transparent)]
    ContainerError(#[from] ContainerError),
    #[error(transparent)]
    VersionsError(#[from] VersionsError),
}

#[derive(Debug, Default)]
pub struct SandboxMetadata {
    pub(crate) channel_versions: CacheItem<Versions>,
}

impl SandboxMetadata {
    pub async fn versions<B: Backend>(
        &self,
        factory: &ContainerFactory<B>,
    ) -> Result<Versions, SandboxMetadataError> {
        // Updates the cached versions if necessary from the factory, otherwise returns the cached value
        let versions = self
            .channel_versions
            .get_value(async move { factory.versions().await })
            .await?;
        Ok(versions)
    }
}
