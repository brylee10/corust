use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{execution::ExecuteResponse, CargoCommand, Channel, ExecuteCommand, OptLevel};

/// Represents a message sent to the container to execute code
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ContainerMessage {
    Execute(ExecuteCommand),
}

// `ContainerMessage` represents a code execution, so all `ContainerMessage` should share these features
impl ContainerMessage {
    pub fn channel(&self) -> Channel {
        match self {
            ContainerMessage::Execute(execute_command) => execute_command.channel,
        }
    }

    pub fn cargo_command(&self) -> CargoCommand {
        match self {
            ContainerMessage::Execute(execute_command) => execute_command.cargo_command,
        }
    }

    pub fn opt_level(&self) -> OptLevel {
        match self {
            ContainerMessage::Execute(execute_command) => execute_command.opt_level,
        }
    }

    pub fn code(&self) -> &str {
        match self {
            ContainerMessage::Execute(execute_command) => &execute_command.code,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ContainerResponse {
    Execute(ExecuteResponse),
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
        }
        Ok(())
    }
}
