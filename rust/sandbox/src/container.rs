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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Serialize, Deserialize, Clone)]
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

/// A backend run creates a [`tokio::process::Command`] and runs it,
/// providing handles to the IO file handles.
pub trait Backend {
    fn prepare_command(&self) -> Command;

    // Starts `runner` process which asynchronously waits for messages via stdin
    fn start_runner_in_background(&self) -> Result<RunContainerResult> {
        let mut cmd = self.prepare_command();
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
}

#[derive(Default)]
pub struct DockerBackend {}

impl DockerBackend {
    pub fn new() -> Self {
        DockerBackend {}
    }
}

impl Backend for DockerBackend {
    fn prepare_command(&self) -> Command {
        let mut cmd = docker_utils::sandboxed_docker_command();
        let container_name = docker_utils::container_name();
        let image_name = "corust";

        cmd.args(["-a", "stdin", "-a", "stdout", "-a", "stderr"])
            // Keep stdin open
            .arg("-i")
            .arg("--name")
            .arg(&container_name)
            .arg("--rm")
            .arg(image_name);
        cmd.arg("runner").arg("/corust");
        cmd
    }
}

mod docker_utils {
    use super::*;

    pub fn sandboxed_docker_command() -> Command {
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

    pub fn container_name() -> String {
        let date_now = Utc::now();
        // unwrap: date time from Utc::now() is not out of range
        let date_now_formatted = format!("{}", date_now.format("%Y%m%d-%H%M%S-%f"));
        format!("corust-{}-{}", date_now_formatted, rand::random::<u32>())
    }
}

pub struct RunContainerResult {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    stderr: ChildStderr,
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

    /// A container factory can generate containers with any backend.
    pub async fn create_container<B: Backend>(&self, backend: B) -> Result<Container<B>> {
        let run_container_permit = Arc::clone(&self.semaphore)
            .acquire_owned()
            .await
            .context(AcquireSemaphoreSnafu)?;
        Ok(Container::new(run_container_permit, backend))
    }

    /// Convenience method to a container with a [`DockerBackend`]
    pub async fn create_container_docker_backend(&self) -> Result<Container<DockerBackend>> {
        let docker_backend = DockerBackend::new();
        self.create_container(docker_backend).await
    }
}

#[derive(EnumSetType, Debug)]
enum ContainerState {
    // Indicates code is executing
    Executing,
}

// Runs a docker container and passes messages to the container via stdin
pub struct Container<B> {
    // The container can be in multiple states at once
    _states: EnumSet<ContainerState>,
    is_executing: AtomicBool,
    _run_permit: OwnedSemaphorePermit,
    backend: B,
}

impl<B: Backend> Container<B> {
    fn new(run_permit: OwnedSemaphorePermit, backend: B) -> Self {
        Container {
            _states: EnumSet::new(),
            is_executing: AtomicBool::new(false),
            _run_permit: run_permit,
            backend,
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
        let run_container = self.backend.start_runner_in_background()?;
        let RunContainerResult {
            stdin,
            stdout,
            stderr,
            child,
        } = run_container;

        let child_io = create_child_io(stdin, stdout, stderr)?;

        // Container is finished executing
        self.is_executing.store(false, Ordering::SeqCst);
        Ok(ContainerRunRet { child, child_io })
    }
}

pub struct ContainerRunRet {
    pub child: Child,
    pub child_io: ChildIo,
}

// Communicates with a component (e.g. container) via serialized
// [`ContainerMessage`]/[`ContainerResponse`] through stdin and stdout, respectively
pub struct ChildIo {
    // Handles to tasks sending to stdin and receiving from stdout and stderr
    pub tasks: JoinSet<Result<()>>,
    // Send messages to component stdin
    pub child_stdin_tx: mpsc::Sender<ContainerMessage>,
    // Receive responses to messages from component stdout
    pub child_stdout_rx: mpsc::Receiver<ContainerResponse>,
}

fn create_child_io(stdin: ChildStdin, stdout: ChildStdout, stderr: ChildStderr) -> Result<ChildIo> {
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

    Ok(ChildIo {
        tasks,
        child_stdin_tx,
        child_stdout_rx,
    })
}

#[cfg(test)]
mod test {
    use std::future::Future;
    use std::{path::PathBuf, sync::Once};

    use assertables::assert_contains;
    use assertables::assert_contains_as_result;
    use env_logger::Target;
    use tempfile::{tempdir, TempDir};

    use crate::init_logger;

    use super::*;

    static INIT_TEST_RUNNER: Once = Once::new();
    static INIT_RUST_PROJECT: Once = Once::new();
    static INIT_ENV_LOGGER: Once = Once::new();
    const TEST_MAX_CONCURRENT_CONTAINERS: usize = 2;
    const TEST_TIMEOUT_SEC: u64 = 60;

    struct TestContainerBackend {
        // Own temp dir so the directory is not dropped until the backend is dropped
        _temp_dir: TempDir,
        // A temporary project directory, holds a rust project
        test_project_dir: PathBuf,
    }

    impl TestContainerBackend {
        fn new(temp_dir: TempDir, test_project_dir: PathBuf) -> Self {
            INIT_ENV_LOGGER.call_once(|| {
                init_logger(Target::Stdout);
            });

            INIT_TEST_RUNNER.call_once(|| {
                // Initialize all binaries in the crate, including the test runner
                let mut cmd = std::process::Command::new("cargo");
                cmd.arg("build")
                    .output()
                    .expect("Failed to build test runner");
            });

            INIT_RUST_PROJECT.call_once(|| {
                // Initialize a rust project in the test project directory
                let mut cmd = std::process::Command::new("cargo");
                cmd.current_dir(&test_project_dir)
                    .arg("init")
                    .output()
                    .expect("Failed to initialize rust project");
            });

            TestContainerBackend {
                _temp_dir: temp_dir,
                test_project_dir,
            }
        }
    }

    impl Backend for TestContainerBackend {
        fn prepare_command(&self) -> Command {
            // Test runs with working directory of package root
            let mut cmd = if cfg!(target_os = "windows") {
                Command::new("../target/debug/runner.exe")
            } else {
                Command::new("../target/debug/runner")
            };
            cmd.arg(&self.test_project_dir);
            cmd
        }
    }

    fn init_test_backend() -> TestContainerBackend {
        let test_project_dir = PathBuf::from("test_project");
        let temp_dir = tempdir().expect("Error creating temporary directory");
        let test_project_dir = temp_dir.path().join(test_project_dir);
        let _ = std::fs::create_dir_all(&test_project_dir);
        TestContainerBackend::new(temp_dir, test_project_dir)
    }

    // Times out a test after a certain number of seconds
    trait Timeout: Future + Sized {
        fn with_timeout(self) -> tokio::time::Timeout<Self> {
            tokio::time::timeout(std::time::Duration::from_secs(TEST_TIMEOUT_SEC), self)
        }
    }

    impl<T: Future + Sized> Timeout for T {}

    #[tokio::test]
    async fn test_hello_world() {
        let backend = init_test_backend();
        let container_factory = ContainerFactory::new(TEST_MAX_CONCURRENT_CONTAINERS);
        let container = container_factory.create_container(backend).await.unwrap();
        let ContainerRunRet {
            mut child,
            mut child_io,
        } = container.run().await.unwrap();

        let execute_command = ExecuteCommand::new(
            "fn main() { println!(\"Hello world!\"); }".to_string(),
            TargetType::Binary,
            CargoCommand::Run,
        );
        let message = ContainerMessage::Execute(execute_command);
        child_io.child_stdin_tx.send(message).await.unwrap();
        // Sends second `ExecuteCommand` to test it is ignored because runner does not process any
        // stdin messages after first `ExecuteCommand`.
        let execute_command = ExecuteCommand::new(
            "fn main() {
                println!(\"Goodbye!\");
            }"
            .to_string(),
            TargetType::Binary,
            CargoCommand::Run,
        );
        let message = ContainerMessage::Execute(execute_command);
        child_io.child_stdin_tx.send(message).await.unwrap();

        let exit_code = child.wait().with_timeout().await.unwrap().unwrap();
        assert!(exit_code.success());

        // Get the last value
        let mut response = None;
        while let Some(value) = child_io.child_stdout_rx.recv().await {
            response = Some(value);
        }
        let response = response.unwrap();
        assert!(matches!(response, ContainerResponse::Execute(_)));
        match response {
            ContainerResponse::Execute(response) => {
                let stdout = String::from_utf8_lossy(&response.stdout);
                assert_contains!(stdout, "Hello world!\n");
            }
        }
    }

    #[tokio::test]
    async fn test_read_stderr() {
        let backend = init_test_backend();
        let container_factory = ContainerFactory::new(TEST_MAX_CONCURRENT_CONTAINERS);
        let container = container_factory.create_container(backend).await.unwrap();
        let ContainerRunRet {
            mut child,
            mut child_io,
        } = container.run().await.unwrap();

        let execute_command = ExecuteCommand::new(
            "fn main() { 
                panic!(\"An error occurred!\"); 
            }"
            .to_string(),
            TargetType::Binary,
            CargoCommand::Run,
        );
        let message = ContainerMessage::Execute(execute_command);
        child_io.child_stdin_tx.send(message).await.unwrap();

        let exit_code = child.wait().with_timeout().await.unwrap().unwrap();
        assert!(exit_code.success());

        // Get the last value
        let mut response = None;
        while let Some(value) = child_io.child_stdout_rx.recv().await {
            response = Some(value);
        }
        let response = response.unwrap();
        assert!(matches!(response, ContainerResponse::Execute(_)));
        match response {
            ContainerResponse::Execute(response) => {
                let stderr = String::from_utf8_lossy(&response.stderr);
                assert_contains!(stderr, "An error occurred!\n");
            }
        }
    }

    #[tokio::test]
    async fn test_long_test_error() {
        let res = tokio::time::sleep(std::time::Duration::from_secs(TEST_TIMEOUT_SEC + 1))
            .with_timeout()
            .await;
        assert!(matches!(res, Err(tokio::time::error::Elapsed { .. })));
    }
}
