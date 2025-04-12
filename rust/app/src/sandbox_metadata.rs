//! API endpoints for querying sandbox metadata, like Rust versions

use std::time::Duration;

use corust_sandbox::container::ContainerError;
use corust_types::container::ChannelVersions;
use thiserror::Error;

use crate::cache::CacheItem;

const ONE_HOUR: Duration = Duration::from_secs(60 * 60);
pub(crate) const SANDBOX_METADATA_TIME_TO_LIVE: Duration = ONE_HOUR;

#[derive(Debug, Error)]
pub enum SandboxMetadataError {
    #[error("Error retrieving the sandbox channel version")]
    RetrievingChannels,
    #[error(transparent)]
    ContainerError(#[from] ContainerError),
}

#[derive(Debug, Default)]
pub struct SandboxMetadata {
    pub(crate) channel_versions: CacheItem<ChannelVersions>,
}
