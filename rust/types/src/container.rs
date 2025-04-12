use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
};

use serde::{Deserialize, Serialize};

use crate::{
    ExecuteCommand,
    execution::ExecuteResponse,
    standalone::{StandaloneCommand, StandaloneResponse},
};

/// Represents a message sent to the container to execute code
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ContainerMessage {
    /// Runs a raw command interpreted as an command line program with arguments
    Standalone(StandaloneCommand),
    /// Command for code compilation
    Execute(ExecuteCommand),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ContainerResponse {
    Execute(ExecuteResponse),
    Standalone(StandaloneResponse),
}

impl Display for ContainerResponse {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerResponse::Execute(response) => {
                writeln!(f, "Execute response")?;
                // Convert bytes to string
                writeln!(f, "stdout: {}", String::from_utf8_lossy(&response.stdout))?;
                writeln!(f, "stderr: {}", String::from_utf8_lossy(&response.stderr))?;
                writeln!(f, "exit code: {:?}", response.exit_code)?;
            }
            ContainerResponse::Standalone(response) => {
                writeln!(f, "Standalone response")?;
                writeln!(f, "stdout: {}", String::from_utf8_lossy(&response.stdout))?;
                writeln!(f, "stderr: {}", String::from_utf8_lossy(&response.stderr))?;
                writeln!(f, "exit code: {:?}", response.exit_code)?;
            }
        }
        Ok(())
    }
}

/// A Rust compiler version
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Version {
    pub release: String,
    pub commit_hash: String,
    pub commit_date: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelVersions {
    pub rustc: Version,
}

/// Versions for each channel
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Versions {
    pub stable: ChannelVersions,
    pub beta: ChannelVersions,
    pub nightly: ChannelVersions,
}

impl Version {
    /// Parse a [`Version`] (commit hash, date, and release version) for rustc from a string of the form
    /// ```text
    /// rustc 1.86.0 (05f9846f8 2025-03-31)
    /// binary: rustc
    /// commit-hash: 05f9846f893b09a1be1fc8560e33fc3c815cfecb
    /// commit-date: 2025-03-31
    /// host: aarch64-apple-darwin
    /// release: 1.86.0
    /// LLVM version: 19.1.7
    /// ```
    pub fn parse_rustc_version_verbose(rustc_version: &str) -> Self {
        let fields: HashMap<&str, &str> = rustc_version
            .lines()
            .skip(1) // Skip the first line (summary line)
            .filter_map(|line| line.split_once(':').map(|(k, v)| (k.trim(), v.trim())))
            .collect();

        Self {
            release: fields.get("release").copied().unwrap_or_default().into(),
            commit_hash: fields
                .get("commit-hash")
                .copied()
                .unwrap_or_default()
                .into(),
            commit_date: fields
                .get("commit-date")
                .copied()
                .unwrap_or_default()
                .into(),
        }
    }
}
