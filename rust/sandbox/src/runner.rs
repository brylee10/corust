use std::path::PathBuf;

use futures::{SinkExt, StreamExt};
use snafu::{ResultExt, Snafu};
use tokio::{
    io::{self},
    sync::mpsc,
    task::JoinHandle,
};

use corust_types::{ContainerMessage, ContainerResponse};
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::codec::{ContainerMessageCodec, ContainerResponseCodec};

#[derive(Debug, Snafu)]
pub enum RunnerError {
    #[snafu(display("Bincode (de)serialization error: {}", source))]
    Bincode { source: bincode::Error },
    #[snafu(display("Read from stdin error: {}", source))]
    ReadStdin { source: std::io::Error },
    #[snafu(visibility(pub))]
    #[snafu(display("Read from stdout error: {}", source))]
    ReadStdout { source: std::io::Error },
    #[snafu(visibility(pub))]
    #[snafu(display("Read from stderr error: {}", source))]
    ReadStderr { source: std::io::Error },
    #[snafu(display(
        "Incorrect message length. Expected {} bytes, got {} bytes",
        expected,
        received
    ))]
    IncorrectMessageLength { expected: usize, received: usize },
    #[snafu(display("Write to stdout error: {}", source))]
    WriteStdout { source: std::io::Error },
    #[snafu(display("Send message error: {}", source))]
    SendMessage {
        source: mpsc::error::SendError<ContainerMessage>,
    },
    #[snafu(visibility(pub))]
    #[snafu(display("Send response error: {}", source))]
    SendResponse {
        source: mpsc::error::SendError<ContainerResponse>,
    },
    #[snafu(visibility(pub))]
    #[snafu(display("Write code error at output: {:?}, source: {}", output, source))]
    WriteCodeError {
        output: PathBuf,
        source: std::io::Error,
    },
    #[snafu(visibility(pub))]
    #[snafu(display("Spawn child error: {}", source))]
    SpawnChild { source: std::io::Error },
    #[snafu(visibility(pub))]
    #[snafu(display("Wait child error: {}", source))]
    WaitChild { source: std::io::Error },
    #[snafu(visibility(pub))]
    #[snafu(display("Failed to capture stdout"))]
    StdoutCapture,
    #[snafu(visibility(pub))]
    #[snafu(display("Failed to capture stderr"))]
    StderrCapture,
    #[snafu(visibility(pub))]
    #[snafu(display("Join task error"))]
    JoinTask { source: tokio::task::JoinError },
}

pub type Result<T, E = RunnerError> = std::result::Result<T, E>;

pub struct RunnerIoComponent {
    pub stdin_handle: JoinHandle<Result<()>>,
    pub stdout_handle: JoinHandle<Result<()>>,
}

pub fn create_runner_io_component(
    stdin_tx: mpsc::Sender<ContainerMessage>,
    mut stdout_rx: mpsc::Receiver<ContainerResponse>,
) -> Result<RunnerIoComponent> {
    let stdin_handle = tokio::spawn(async move {
        // Read from stdin and send to `stdin_tx`
        let stdin = io::stdin();
        let decoder = ContainerMessageCodec::new();
        let mut reader = FramedRead::new(stdin, decoder);
        // Read from stdin and send to `stdin_tx`
        while let Some(msg) = reader.next().await {
            let msg = msg.context(BincodeSnafu)?;
            stdin_tx.send(msg).await.context(SendMessageSnafu)?;
        }
        Ok(())
    });

    let stdout_handle = tokio::spawn(async move {
        // Write responses to stdout
        let stdout = io::stdout();
        let encoder = ContainerResponseCodec::new();
        let mut writer = FramedWrite::new(stdout, encoder);
        loop {
            let response = stdout_rx.recv().await;
            match response {
                Some(response) => {
                    writer.send(response).await.context(BincodeSnafu)?;
                }
                None => {
                    // All `stdout_tx` have been dropped, occurs when certain messages in `stdin_rx` are received
                    break;
                }
            }
        }
        Ok(())
    });
    Ok(RunnerIoComponent {
        stdin_handle,
        stdout_handle,
    })
}
