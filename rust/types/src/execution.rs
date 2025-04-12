use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::ContainerMessage;

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum TargetType {
    Library,
    Binary,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Hash, PartialEq, Eq)]
pub enum CargoCommand {
    Build,
    Run,
    Test,
    Clippy,
}

/// Categorizes [`CargoCommand`] into types
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum CargoCommandType {
    Execute,
    Clippy,
}

impl From<CargoCommand> for CargoCommandType {
    fn from(cargo_command: CargoCommand) -> Self {
        match cargo_command {
            CargoCommand::Build | CargoCommand::Run | CargoCommand::Test => {
                CargoCommandType::Execute
            }
            CargoCommand::Clippy => CargoCommandType::Clippy,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum OptLevel {
    Debug,
    Release,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum Channel {
    Stable,
    Beta,
    Nightly,
}

impl Display for Channel {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Channel::Stable => write!(f, "stable"),
            Channel::Beta => write!(f, "beta"),
            Channel::Nightly => write!(f, "nightly"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteCommand {
    /// The code that was executed. This will not necessarily be the same
    /// as the current document state if the document has been updated.
    pub code: String,
    pub target_type: TargetType,
    pub cargo_command: CargoCommand,
    pub opt_level: OptLevel,
    pub channel: Channel,
}

impl ExecuteCommand {
    pub fn new(
        code: String,
        target_type: TargetType,
        cargo_command: CargoCommand,
        opt_level: OptLevel,
        channel: Channel,
    ) -> Self {
        ExecuteCommand {
            code,
            target_type,
            cargo_command,
            opt_level,
            channel,
        }
    }
}

// Mirrors `std::process::Output`
// All executions start with empty stdout and stderr, clearing output from prior runs.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExecuteResponse {
    /// Fields which are updated as the code is executed
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
}

// Server side in memory storage of most recent code and run output for one session
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CodeOutputState {
    pub container_msg: ContainerMessage,
    pub runner_output: Option<RunnerOutput>,
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
