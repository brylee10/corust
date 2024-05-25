//! Container module for running code in a sandboxed environment
//! Inspiration taken from Rust Playground `coordinator.rs`.

use std::{
    fmt::{Display, Formatter},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use chrono::Utc;
use enumset::{EnumSet, EnumSetType};
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt, Snafu};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command},
    sync::{mpsc, OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
};

use crate::MESSAGE_BUF_SIZE_BYTES;

pub const IO_COMPONENT_CHANNEL_SIZE: usize = 100;

pub type SharedContainerFactory = Arc<ContainerFactory>;

#[derive(Debug, Snafu)]
pub enum ContainerError {
    #[snafu(display("Command failed to start: {}", source))]
    SpawnChild { source: std::io::Error },
    #[snafu(display("Run child error: {}", source))]
    RunChild { source: std::io::Error },
    #[snafu(display("Failed to capture stdin"))]
    StdinCapture,
    #[snafu(display("Failed to capture stdout"))]
    StdoutCapture,
    #[snafu(display("Failed to capture stderr"))]
    StderrCapture,
    #[snafu(display("Bincode (de)serialization error: {}", source))]
    Bincode { source: bincode::Error },
    #[snafu(display("Container is already executing"))]
    ContainerAlreadyExecuting,
    #[snafu(display("Acquire sempahore error: {}", source))]
    AcquireSemaphore { source: tokio::sync::AcquireError },
    #[snafu(display("Read from stdout error: {}", source))]
    ReadStdout { source: std::io::Error },
    #[snafu(display("Read from stderr error: {}", source))]
    ReadStderr { source: std::io::Error },
    #[snafu(display(
        "Incorrect message length. Expected {} bytes, got {} bytes",
        expected,
        received
    ))]
    IncorrectMessageLength { expected: usize, received: usize },
    #[snafu(display("Send message error: {}", source))]
    SendMessage {
        source: mpsc::error::SendError<ContainerResponse>,
    },
}

type Result<T, E = ContainerError> = std::result::Result<T, E>;

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum TargetType {
    Library,
    Binary,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum CargoCommand {
    Build,
    Run,
    Test,
    Clippy,
}

impl From<CargoCommand> for Command {
    fn from(cargo_command: CargoCommand) -> Self {
        let mut command = Command::new("cargo");
        match cargo_command {
            CargoCommand::Build => command.arg("build"),
            CargoCommand::Run => command.arg("run").arg("--release"),
            CargoCommand::Test => command.arg("test"),
            CargoCommand::Clippy => command.arg("clippy"),
        };
        command
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExecuteCommand {
    pub code: String,
    pub target_type: TargetType,
    pub cargo_command: CargoCommand,
}

impl ExecuteCommand {
    pub fn new(code: String, target_type: TargetType, cargo_command: CargoCommand) -> Self {
        ExecuteCommand {
            code,
            target_type,
            cargo_command,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ContainerMessage {
    Execute(ExecuteCommand),
}

// Mirrors `std::process::Output`
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExecuteResponse {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
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

fn sandboxed_docker_command() -> Command {
    let mut cmd = Command::new("docker");
    cmd.arg("run")
        // Drop all capabilities
        // https://man7.org/linux/man-pages/man7/capabilities.7.html
        .arg("--cap-drop")
        .arg("ALL")
        // Disable network access, only loopback is allowed
        // https://docs.docker.com/network/drivers/none/
        .arg("--network")
        .arg("none")
        .arg("--memory")
        .arg("512m")
        // Allow some memory to be swapped to disk
        // https://docs.docker.com/config/containers/resource_constraints/#--memory-swap-details
        .arg("--memory-swap")
        .arg("512m")
        .arg("--pids-limit")
        .arg("128")
        // OOM kill priority for this container set to highest
        .arg("--oom-score-adj")
        .arg("1000");
    // Other defaults:
    // - `cpu-shares`: default is equal relative weight among all containers (value 1024 for each)
    // - `privileged`: default is false, does not give extended privileges to this container

    cmd
}

fn container_name() -> String {
    let date_now = Utc::now();
    // unwrap: date time from Utc::now() is not out of range
    let date_now_formatted = format!("{}", date_now.format("%Y%m%d-%H%M%S-%f"));
    format!("corust-{}-{}", date_now_formatted, rand::random::<u32>())
}

fn prepare_command() -> Command {
    let mut cmd = sandboxed_docker_command();
    let container_name = container_name();
    let image_name = "corust";

    cmd.args(["-a", "stdin", "-a", "stdout", "-a", "stderr"])
        // Keep stdin open
        .arg("-i")
        .arg("--name")
        .arg(&container_name)
        .arg("--rm")
        .arg(image_name);
    cmd
}

struct RunContainerResult {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    stderr: ChildStderr,
}

// Starts `runner` process which asynchronously waits for messages via stdin
fn start_runner_in_background() -> Result<RunContainerResult> {
    let mut cmd = prepare_command();
    cmd.arg("runner").arg("/corust");
    log::debug!("Running command: {:?}", cmd);

    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(SpawnChildSnafu {})?;

    // https://docs.rs/tokio/latest/tokio/process/struct.Child.html#fields
    let stdin = child.stdin.take().context(StdinCaptureSnafu)?;
    let stdout = child.stdout.take().context(StdoutCaptureSnafu)?;
    let stderr = child.stderr.take().context(StderrCaptureSnafu)?;

    let run_container_result = RunContainerResult {
        child,
        stdin,
        stdout,
        stderr,
    };

    Ok(run_container_result)
}

pub struct ContainerFactory {
    // Controls number of concurrent containers
    semaphore: Arc<Semaphore>,
}

impl ContainerFactory {
    pub fn new(max_concurrent_containers: usize) -> Self {
        ContainerFactory {
            semaphore: Arc::new(Semaphore::new(max_concurrent_containers)),
        }
    }

    pub async fn create_container(&self) -> Result<Container> {
        let run_container_permit = Arc::clone(&self.semaphore)
            .acquire_owned()
            .await
            .context(AcquireSemaphoreSnafu)?;
        Ok(Container::new(run_container_permit))
    }
}

#[derive(EnumSetType, Debug)]
enum ContainerState {
    // Indicates code is executing
    Executing,
}

// Runs a docker container and passes messages to the container via stdin
pub struct Container {
    // The container can be in multiple states at once
    _states: EnumSet<ContainerState>,
    is_executing: AtomicBool,
    _run_permit: OwnedSemaphorePermit,
}

impl Container {
    fn new(run_permit: OwnedSemaphorePermit) -> Self {
        Container {
            _states: EnumSet::new(),
            is_executing: AtomicBool::new(false),
            _run_permit: run_permit,
        }
    }

    pub async fn run(&self) -> Result<ContainerRunRet> {
        // A container corresponds to one coding session, so it can only execute one
        // code file at a time. Check and set "is running" in one operation.
        if let Err(prev_val) =
            self.is_executing
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        {
            debug_assert!(prev_val, "Container should already be executing");
            return Err(ContainerError::ContainerAlreadyExecuting);
        }

        // Run a docker container, returning the container stdin, stdout, and stderr
        let run_container = start_runner_in_background()?;
        let RunContainerResult {
            stdin,
            stdout,
            stderr,
            child,
        } = run_container;

        let io_component = create_io_component(stdin, stdout, stderr)?;

        // Container is finished executing
        self.is_executing.store(false, Ordering::SeqCst);
        Ok(ContainerRunRet {
            child,
            io_component,
        })
    }
}

pub struct ContainerRunRet {
    pub child: Child,
    pub io_component: IoComponent,
}

// Communicates with a component (e.g. container) via serialized
// [`ContainerMessage`]/[`ContainerResponse`] through stdin and stdout, respectively
pub struct IoComponent {
    // Handles to tasks sending to stdin and receiving from stdout and stderr
    pub tasks: JoinSet<Result<()>>,
    // Send messages to component stdin
    pub child_stdin_tx: mpsc::Sender<ContainerMessage>,
    // Receive responses to messages from component stdout
    pub child_stdout_rx: mpsc::Receiver<ContainerResponse>,
}

fn create_io_component(
    stdin: ChildStdin,
    stdout: ChildStdout,
    stderr: ChildStderr,
) -> Result<IoComponent> {
    let (child_stdin_tx, mut child_stdin_rx) =
        mpsc::channel::<ContainerMessage>(IO_COMPONENT_CHANNEL_SIZE);
    let (child_stdout_tx, child_stdout_rx) =
        mpsc::channel::<ContainerResponse>(IO_COMPONENT_CHANNEL_SIZE);
    let mut tasks = JoinSet::new();

    // Read bytes from Child stdout and send to the `stdout_rx`
    // Code execution results are written to stdout
    tasks.spawn(async move {
        let mut stdout = BufReader::new(stdout);
        // Reads buffer size
        let mut msg_size_buf = [0u8; MESSAGE_BUF_SIZE_BYTES];
        loop {
            // Message size is sent as 4 byte little endian
            let msg_size_buf_bytes = stdout
                .read(&mut msg_size_buf)
                .await
                .context(ReadStdoutSnafu)?;

            if msg_size_buf_bytes == 0 {
                log::debug!("Child stdout has closed");
                return Ok(());
            }

            if msg_size_buf_bytes != MESSAGE_BUF_SIZE_BYTES {
                log::error!(
                    "Incorrect message size length. Expected {} bytes, got {} bytes",
                    MESSAGE_BUF_SIZE_BYTES,
                    msg_size_buf_bytes
                );
                return Err(ContainerError::IncorrectMessageLength {
                    expected: MESSAGE_BUF_SIZE_BYTES,
                    received: msg_size_buf_bytes,
                });
            }

            // `usize` is 8 bytes on 64-bit systems
            let expected_msg_sz_bytes = usize::try_from(u32::from_le_bytes(msg_size_buf)).unwrap();
            let mut msg_buf = vec![0u8; expected_msg_sz_bytes];
            let msg_sz_bytes = stdout
                .read_exact(&mut msg_buf)
                .await
                .context(ReadStdoutSnafu)?;

            if msg_sz_bytes != expected_msg_sz_bytes {
                log::error!(
                    "Incorrect message length. Expected {} bytes, got {} bytes",
                    expected_msg_sz_bytes,
                    msg_sz_bytes
                );
                return Err(ContainerError::IncorrectMessageLength {
                    expected: expected_msg_sz_bytes,
                    received: msg_sz_bytes,
                });
            }

            let msg = bincode::deserialize(&msg_buf).context(BincodeSnafu)?;
            log::debug!(
                "Received message of size {:?} bytes from stdout, msg: {}",
                msg_sz_bytes,
                msg
            );
            child_stdout_tx.send(msg).await.context(SendMessageSnafu)?;
        }
    });

    // Receive messages from `stdin_receiver`, write to stdin as bytes
    tasks.spawn(async move {
        let mut stdin = BufWriter::new(stdin);
        while let Some(msg) = child_stdin_rx.recv().await {
            log::debug!("Received message `msg` in stdin receiver: {:?}", msg);
            // Serialize message into buffer, with a 4 byte prefix for the size of the message
            // as required by `AsyncBincodeReader`:
            // https://docs.rs/async-bincode/latest/async_bincode/futures/struct.AsyncBincodeReader.html
            let mut buffer = vec![];
            // unwrap u32: the size of message will not overflow u32
            let serialized_size =
                u32::try_from(bincode::serialized_size(&msg).context(BincodeSnafu)?).unwrap();
            // Convert u64 to a 4 byte array in little endian
            let size_bytes: [u8; 4] = serialized_size.to_le_bytes();
            buffer.extend_from_slice(&size_bytes);
            bincode::serialize_into(&mut buffer, &msg).context(BincodeSnafu)?;
            log::debug!(
                "Sending byte serialized message `msg` to container: {:?}",
                buffer
            );
            stdin.write_all(&buffer).await.unwrap();
            log::debug!("Wrote message to container");
            stdin.flush().await.unwrap();
            log::debug!("Flushed message to container");
        }
        Ok(())
    });

    // Receive messages from child stderr, prints these as log messages
    tasks.spawn(async move {
        let mut stderr = BufReader::new(stderr).lines();
        while let Some(line) = stderr.next_line().await.context(ReadStderrSnafu)? {
            log::debug!("{:?}", line);
        }
        Ok(())
    });

    Ok(IoComponent {
        tasks,
        child_stdin_tx,
        child_stdout_rx,
    })
}
