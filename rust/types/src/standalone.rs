use serde::{Deserialize, Serialize};

/// Represents a command to run as a standalone command line program with arguments
/// Commands which execute code, like cargo, are a special case of this command but use the [`ExecuteCommand`] type.
// Analogous to Rust Playground `ExecuteCommandRequest`
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StandaloneCommand {
    /// Command to run
    pub command: String,
    /// Arguments to pass to the command
    pub args: Vec<String>,
}

impl StandaloneCommand {
    /// Construct a [`StandaloneCommand`] from a string command and arguments
    pub fn simple<T: Into<String>, U: IntoIterator<Item = impl Into<String>>>(
        command: T,
        args: U,
    ) -> Self {
        let command: String = command.into();
        let args: Vec<String> = args.into_iter().map(|i| i.into()).collect();
        Self { command, args }
    }
}

/// Mirrors `std::process::Output`
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StandaloneResponse {
    /// Fields which are updated the command is executed
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
}
