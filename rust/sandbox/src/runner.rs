use std::path::PathBuf;

use snafu::{ResultExt, Snafu};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter},
    sync::mpsc,
    task::JoinHandle,
};

use crate::{
    container::{ContainerMessage, ContainerResponse},
    MESSAGE_BUF_SIZE_BYTES,
};

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
        let mut stdin = BufReader::new(stdin);
        let mut msg_size_buf = [0u8; MESSAGE_BUF_SIZE_BYTES];
        // Read from stdin and send to `stdin_tx`
        loop {
            // Message size is sent as 4 byte little endian
            // Equal to 4
            let msg_size_buf_bytes = stdin
                .read(&mut msg_size_buf)
                .await
                .context(ReadStdinSnafu)?;

            if msg_size_buf_bytes != MESSAGE_BUF_SIZE_BYTES {
                log::error!(
                    "Incorrect message size length. Expected {} bytes, got {} bytes",
                    MESSAGE_BUF_SIZE_BYTES,
                    msg_size_buf_bytes
                );
                return Err(RunnerError::IncorrectMessageLength {
                    expected: MESSAGE_BUF_SIZE_BYTES,
                    received: msg_size_buf_bytes,
                });
            }

            // `usize` is 8 bytes on 64-bit systems
            let expected_msg_sz_bytes = usize::try_from(u32::from_le_bytes(msg_size_buf)).unwrap();
            let mut msg_buf = vec![0u8; expected_msg_sz_bytes];
            let msg_sz_bytes = stdin
                .read_exact(&mut msg_buf)
                .await
                .context(ReadStdinSnafu)?;

            if msg_sz_bytes != expected_msg_sz_bytes {
                log::error!(
                    "Incorrect message length. Expected {} bytes, got {} bytes",
                    expected_msg_sz_bytes,
                    msg_sz_bytes
                );
                return Err(RunnerError::IncorrectMessageLength {
                    expected: expected_msg_sz_bytes,
                    received: msg_sz_bytes,
                });
            }

            let msg = bincode::deserialize(&msg_buf).context(BincodeSnafu)?;
            log::debug!(
                "Received message of size {:?} bytes from stdin, msg: {:?}",
                msg_sz_bytes,
                msg
            );
            stdin_tx.send(msg).await.context(SendMessageSnafu)?;
        }
    });

    let stdout_handle = tokio::spawn(async move {
        // Write responses to stdout
        let stdout = io::stdout();
        let mut stdout = BufWriter::new(stdout);
        loop {
            let response = stdout_rx.recv().await;
            match response {
                Some(response) => {
                    let mut buffer = vec![];
                    // unwrap u32: the size of message will not overflow u32
                    let serialized_size =
                        u32::try_from(bincode::serialized_size(&response).context(BincodeSnafu)?)
                            .unwrap();
                    // Convert u64 to a 4 byte array in little endian
                    let size_bytes: [u8; 4] = serialized_size.to_le_bytes();
                    buffer.extend_from_slice(&size_bytes);
                    bincode::serialize_into(&mut buffer, &response).context(BincodeSnafu)?;
                    log::debug!(
                        "Sending byte serialized response from runner to container: {:?}, length in bytes: {}",
                        buffer, buffer.len()
                    );
                    stdout.write_all(&buffer).await.unwrap();
                    log::debug!("Wrote response to container");
                    stdout.flush().await.unwrap();
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
