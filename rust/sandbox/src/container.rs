//! Container module for running code in a sandboxed environment
//! Inspiration taken from Rust Playground `coordinator.rs`.

use std::{
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use chrono::Utc;
use corust_types::{
    CargoCommand, Channel, ContainerMessage, ContainerResponse, ExecuteCommand, ExecuteResponse,
    OptLevel,
};
use enumset::{EnumSet, EnumSetType};
use futures::{SinkExt, StreamExt};
use snafu::{OptionExt, ResultExt, Snafu};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command},
    sync::{OwnedSemaphorePermit, Semaphore, mpsc},
    task::JoinSet,
};
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::codec::{ContainerMessageCodec, ContainerResponseCodec};

pub const IO_COMPONENT_CHANNEL_SIZE: usize = 100;
// Max number of bytes a stdout/stderr can be before the process is killed
// This limit should be sufficient for any reasonable output
const STDOUT_ERR_BYTE_LIMIT: usize = 64_000;

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
    #[snafu(display(
        "Stdout too large. Max bytes {}, received {}",
        max_bytes,
        received_bytes
    ))]
    StdoutTooLarge {
        max_bytes: usize,
        received_bytes: usize,
    },
    #[snafu(display(
        "Stderr too large. Max bytes {}, received {}",
        max_bytes,
        received_bytes
    ))]
    StderrTooLarge {
        max_bytes: usize,
        received_bytes: usize,
    },
}

type Result<T, E = ContainerError> = std::result::Result<T, E>;

/// A backend run creates a [`tokio::process::Command`] and runs it,
/// providing handles to the IO file handles.
pub trait Backend {
    fn prepare_command(&self, channel: Channel) -> Command;

    // Starts `runner` process which asynchronously waits for messages via stdin
    fn start_runner_in_background(&self, channel: Channel) -> Result<RunContainerResult> {
        let mut cmd = self.prepare_command(channel);
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
    fn prepare_command(&self, channel: Channel) -> Command {
        let mut cmd = docker_utils::sandboxed_docker_command();
        let container_name = docker_utils::container_name();
        let image_name = format!("rust-{}", channel);

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
        let date_now_formatted = format!("{}", date_now.format("%Y%m%d-%H%M%S"));
        format!("corust-{}-{}", date_now_formatted, rand::random::<u32>())
    }
}

pub struct RunContainerResult {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    stderr: ChildStderr,
}

/// A factory for creating containers with a specific backend.
/// The factory controls the number of concurrent containers that can be run.
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

    /// Creates a container, waiting for a permit.
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

    /// Run the container, returning the child process and IO handles.
    /// The channel specifies the environment to run the code in.
    pub async fn run(&self, channel: Channel) -> Result<ContainerRunRet> {
        // A container corresponds to one coding session, so it can only execute one
        // code file at a time. Check and set "is running" in one operation.
        if let Err(prev_val) =
            self.is_executing
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        {
            debug_assert!(prev_val, "Container should not already be executing");
            return Err(ContainerError::ContainerAlreadyExecuting);
        }

        // Run a docker container, returning the container stdin, stdout, and stderr
        let run_container = self.backend.start_runner_in_background(channel)?;
        let RunContainerResult {
            stdin,
            stdout,
            stderr,
            child,
        } = run_container;

        let child_io = create_child_io(stdin, stdout, stderr)?;

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
    // Option<T> so it can be taken out of the struct and manually dropped
    pub child_stdin_tx: Option<mpsc::Sender<ContainerMessage>>,
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
        let stdout = BufReader::new(stdout);
        let decoder = ContainerResponseCodec::new();
        let mut reader = FramedRead::new(stdout, decoder);
        while let Some(response) = reader.next().await {
            let response = response.context(BincodeSnafu)?;
            // Check if the stdout/stderr is too large
            match &response {
                ContainerResponse::Execute(ExecuteResponse { stdout, stderr, .. }) => {
                    if stdout.len() > STDOUT_ERR_BYTE_LIMIT {
                        log::error!("stdout/stderr too large, killing container");
                        return Err(ContainerError::StdoutTooLarge {
                            max_bytes: STDOUT_ERR_BYTE_LIMIT,
                            received_bytes: stdout.len(),
                        });
                    }

                    if stderr.len() > STDOUT_ERR_BYTE_LIMIT {
                        log::error!("stderr too large, killing container");
                        return Err(ContainerError::StderrTooLarge {
                            max_bytes: STDOUT_ERR_BYTE_LIMIT,
                            received_bytes: stderr.len(),
                        });
                    }
                }
            }
            child_stdout_tx
                .send(response)
                .await
                .context(SendMessageSnafu)?;
        }
        Ok(())
    });

    // Receive messages from `stdin_receiver`, write to stdin as bytes
    tasks.spawn(async move {
        let encoder = ContainerMessageCodec::new();
        let mut writer = FramedWrite::new(stdin, encoder);
        while let Some(msg) = child_stdin_rx.recv().await {
            log::debug!("Received message `msg` in stdin receiver: {:?}", msg);
            writer.send(msg).await.context(BincodeSnafu)?;
        }
        log::debug!("stdin receiver finished");
        Ok(())
    });

    // Receive messages from child stderr, prints these as log messages
    tasks.spawn(async move {
        let mut stderr = BufReader::new(stderr).lines();
        while let Some(line) = stderr.next_line().await.context(ReadStderrSnafu)? {
            log::debug!("{:?}", line);
        }
        log::debug!("stderr receiver finished");
        Ok(())
    });

    Ok(ChildIo {
        tasks,
        child_stdin_tx: Some(child_stdin_tx),
        child_stdout_rx,
    })
}

/// Converts an `ExecuteCommand` to a `Command` to be run by `cargo`.
/// Not implemented as `From` trait due to orphan rules.
pub fn execute_command_to_command(execute_command: &ExecuteCommand) -> Command {
    let mut command = Command::new("cargo");
    match &execute_command.channel {
        Channel::Stable => command.arg("+stable"),
        Channel::Beta => command.arg("+beta"),
        Channel::Nightly => command.arg("+nightly"),
    };
    match &execute_command.cargo_command {
        CargoCommand::Build => command.arg("build"),
        CargoCommand::Run => command.arg("run"),
        CargoCommand::Test => command.arg("test"),
        CargoCommand::Clippy => command.arg("clippy"),
    };
    match &execute_command.opt_level {
        OptLevel::Debug => &mut command,
        OptLevel::Release => command.arg("--release"),
    };
    command
}

#[cfg(test)]
mod test {
    use std::future::Future;
    use std::{path::PathBuf, sync::Once};

    use assertables::assert_contains;
    use assertables::assert_contains_as_result;
    use corust_types::{CargoCommand, ExecuteCommand, OptLevel, TargetType};
    use env_logger::Target;
    use tempfile::{TempDir, tempdir};

    use crate::init_logger;

    use super::*;

    static INIT_TEST_RUNNER: Once = Once::new();
    static INIT_ENV_LOGGER: Once = Once::new();
    const TEST_MAX_CONCURRENT_CONTAINERS: usize = 2;
    const TEST_TIMEOUT_SEC: u64 = 10;

    struct TestContainerBackend {
        // Own temp dir so the directory is not dropped until the backend is dropped
        _temp_dir: TempDir,
        // A temporary project directory, holds a rust project
        test_project_dir: PathBuf,
    }

    impl TestContainerBackend {
        fn new(temp_dir: TempDir, test_project_dir: PathBuf) -> Self {
            INIT_ENV_LOGGER.call_once(|| {
                init_logger(Target::Stdout, "info".to_string());
            });

            INIT_TEST_RUNNER.call_once(|| {
                // Initialize all binaries in this crate, including the test runner
                let mut cmd = std::process::Command::new("cargo");
                cmd.arg("build")
                    .output()
                    .expect("Failed to build test runner");
            });

            // Initialize a rust project in the test project directory. This occurs once per
            // test since the `TestContainerBackend` and `test_project_dir` is different for each.
            let mut cmd = std::process::Command::new("cargo");
            cmd.current_dir(&test_project_dir)
                .arg("init")
                .output()
                .expect("Failed to initialize rust project");

            TestContainerBackend {
                _temp_dir: temp_dir,
                test_project_dir,
            }
        }
    }

    impl Backend for TestContainerBackend {
        // The test backend runs the tests in the same environment
        // (assumes rustup has stable, beta, and nightly toolchains installed)
        fn prepare_command(&self, _channel: Channel) -> Command {
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
        } = container.run(Channel::Stable).await.unwrap();

        let execute_command = ExecuteCommand::new(
            "fn main() { println!(\"Hello world!\"); }".to_string(),
            TargetType::Binary,
            CargoCommand::Run,
            OptLevel::Release,
            Channel::Stable,
        );
        let message = ContainerMessage::Execute(execute_command);
        child_io
            .child_stdin_tx
            .as_ref()
            .unwrap()
            .send(message)
            .await
            .unwrap();
        // Sends second `ExecuteCommand` to test it is ignored because runner does not process any
        // stdin messages after first `ExecuteCommand`.
        let execute_command = ExecuteCommand::new(
            "fn main() {
                println!(\"Goodbye!\");
            }"
            .to_string(),
            TargetType::Binary,
            CargoCommand::Run,
            OptLevel::Release,
            Channel::Stable,
        );
        let message = ContainerMessage::Execute(execute_command);
        child_io
            .child_stdin_tx
            .as_ref()
            .unwrap()
            .send(message)
            .await
            .unwrap();

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
        } = container.run(Channel::Stable).await.unwrap();

        let execute_command = ExecuteCommand::new(
            "fn main() { 
                panic!(\"An error occurred!\"); 
            }"
            .to_string(),
            TargetType::Binary,
            CargoCommand::Run,
            OptLevel::Release,
            Channel::Stable,
        );
        let message = ContainerMessage::Execute(execute_command);
        child_io
            .child_stdin_tx
            .as_ref()
            .unwrap()
            .send(message)
            .await
            .unwrap();

        let exit_code = child.wait().with_timeout().await.unwrap().unwrap();
        assert!(exit_code.success());

        // Get the last value
        let mut response: Option<ContainerResponse> = None;
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
        // "A meta test". Tests that the test infrastructure helper `with_timeout` works properly by timing out
        // any task that takes longer than `TEST_TIMEOUT_SEC` seconds.
        let res = tokio::time::sleep(std::time::Duration::from_secs(TEST_TIMEOUT_SEC + 1))
            .with_timeout()
            .await;
        assert!(matches!(res, Err(tokio::time::error::Elapsed { .. })));
    }

    #[tokio::test]
    async fn test_library_crate() {
        // Tests a library target type can be compiled
        let backend = init_test_backend();
        let container_factory = ContainerFactory::new(TEST_MAX_CONCURRENT_CONTAINERS);
        let container = container_factory.create_container(backend).await.unwrap();
        let ContainerRunRet {
            mut child,
            mut child_io,
        } = container.run(Channel::Stable).await.unwrap();

        let execute_command = ExecuteCommand::new(
            "struct Test { x: i32 }".to_string(),
            TargetType::Library,
            CargoCommand::Build,
            OptLevel::Release,
            Channel::Stable,
        );

        let message = ContainerMessage::Execute(execute_command);
        child_io
            .child_stdin_tx
            .as_ref()
            .unwrap()
            .send(message)
            .await
            .unwrap();

        let exit_code = child.wait().with_timeout().await.unwrap().unwrap();
        assert!(exit_code.success());

        // Get the last value
        let mut response: Option<ContainerResponse> = None;
        while let Some(value) = child_io.child_stdout_rx.recv().await {
            response = Some(value);
        }
        let response = response.unwrap();
        assert!(matches!(response, ContainerResponse::Execute(_)));
        match response {
            ContainerResponse::Execute(response) => {
                let stderr = String::from_utf8_lossy(&response.stderr);
                assert_contains!(stderr, "Finished `release` profile");
            }
        }
    }

    #[tokio::test]
    async fn test_opt_level_build() {
        // Tests code is compiled in debug mode when [`OptLevel::Debug`] is passed
        // and in release mode when [`OptLevel::Release`] is passed.
        // The code will panic in debug mode but not in release mode.
        let container_factory = ContainerFactory::new(TEST_MAX_CONCURRENT_CONTAINERS);
        struct ExpectedOutput {
            stderr: String,
            stdout: String,
            exit_code: i32,
        }

        let expected_output = [
            ExpectedOutput {
                stderr: "thread 'main' panicked".to_string(),
                stdout: "".to_string(),
                exit_code: 101,
            },
            ExpectedOutput {
                stderr: "".to_string(),
                stdout: "Hello world".to_string(),
                exit_code: 0,
            },
        ];

        for (opt_level, expected_output) in [OptLevel::Debug, OptLevel::Release]
            .iter()
            .zip(expected_output.iter())
        {
            // One container (which maps 1-1 with a runner) must be created for each run
            let backend = init_test_backend();
            let container = container_factory.create_container(backend).await.unwrap();
            let ContainerRunRet {
                mut child,
                mut child_io,
            } = container.run(Channel::Stable).await.unwrap();

            // Only panics in debug mode, prints "Hello world" in release mode
            let execute_command = ExecuteCommand::new(
                r#"fn main() { debug_assert!(false); println!("Hello world") }"#.to_string(),
                TargetType::Binary,
                CargoCommand::Run,
                *opt_level,
                Channel::Stable,
            );

            let message = ContainerMessage::Execute(execute_command);
            child_io
                .child_stdin_tx
                .as_ref()
                .unwrap()
                .send(message)
                .await
                .unwrap();

            // Child process succeeds, but the code it runs will panic
            let exit_code = child.wait().with_timeout().await.unwrap().unwrap();
            assert!(exit_code.success());

            // Get the last value, the output is built up incrementally
            let mut response: Option<ContainerResponse> = None;
            while let Some(value) = child_io.child_stdout_rx.recv().await {
                response = Some(value);
            }

            let response = response.unwrap();
            assert!(matches!(response, ContainerResponse::Execute(_)));
            match response {
                ContainerResponse::Execute(response) => {
                    let stderr = String::from_utf8_lossy(&response.stderr);
                    assert_contains!(stderr, &expected_output.stderr);
                    let stdout = String::from_utf8_lossy(&response.stdout);
                    assert_contains!(stdout, &expected_output.stdout);
                    let exit_code = response.exit_code.unwrap();
                    // Exit code, convention for panic:
                    // https://users.rust-lang.org/t/solved-why-101-exit-code-when-use-panic/80061
                    assert_eq!(exit_code, expected_output.exit_code);
                }
            }
        }
    }

    #[tokio::test]
    async fn test_cargo_test() {
        // Test code compiled with `cargo test` runs tests
        let backend = init_test_backend();
        let container_factory = ContainerFactory::new(TEST_MAX_CONCURRENT_CONTAINERS);
        let container = container_factory.create_container(backend).await.unwrap();
        let ContainerRunRet {
            mut child,
            mut child_io,
        } = container.run(Channel::Stable).await.unwrap();

        let execute_command = ExecuteCommand::new(
            r#"
            #[cfg(test)]
            mod tests {
                #[test]
                fn it_works() {
                    assert_eq!(2 + 2, 4);
                }
            }
            "#
            .to_string(),
            TargetType::Library,
            CargoCommand::Test,
            OptLevel::Release,
            Channel::Stable,
        );

        let message = ContainerMessage::Execute(execute_command);
        child_io
            .child_stdin_tx
            .as_ref()
            .unwrap()
            .send(message)
            .await
            .unwrap();

        let exit_code = child.wait().with_timeout().await.unwrap().unwrap();
        assert!(exit_code.success());

        // Get the last value
        let mut response: Option<ContainerResponse> = None;
        while let Some(value) = child_io.child_stdout_rx.recv().await {
            response = Some(value);
        }
        let response = response.unwrap();
        assert!(matches!(response, ContainerResponse::Execute(_)));
        match response {
            ContainerResponse::Execute(response) => {
                let stdout = String::from_utf8_lossy(&response.stdout);
                assert_contains!(stdout, "running 1 test");
                assert_contains!(stdout, "test tests::it_works ... ok");
            }
        }
    }

    #[tokio::test]
    async fn test_nightly_build() {
        // Gate test to only run on nightly channel
        let version = rustc_version::version_meta().unwrap();
        if !matches!(version.channel, rustc_version::Channel::Nightly) {
            return;
        }

        // Test code compiled with nightly toolchain builds (allows nightly flags)
        let backend = init_test_backend();
        let container_factory = ContainerFactory::new(TEST_MAX_CONCURRENT_CONTAINERS);
        let container = container_factory.create_container(backend).await.unwrap();
        // Note that the channel configuration in tests does not have an affect, but would in production docker containers.
        let ContainerRunRet {
            mut child,
            mut child_io,
        } = container.run(Channel::Nightly).await.unwrap();

        // Select an internal Rust function that does not have a stable counterpart and
        // should not be stabilized in the future. This should only compile and run on nightly.
        // https://doc.rust-lang.org/std/intrinsics/fn.unlikely.html
        let execute_command = ExecuteCommand::new(
            r#"
            #![feature(core_intrinsics)]
            fn main() {
                let _ = std::intrinsics::unlikely(false);
            }
            "#
            .to_string(),
            TargetType::Binary,
            CargoCommand::Build,
            OptLevel::Release,
            Channel::Nightly,
        );

        let message = ContainerMessage::Execute(execute_command);
        child_io
            .child_stdin_tx
            .as_ref()
            .unwrap()
            .send(message)
            .await
            .unwrap();

        let exit_code = child.wait().with_timeout().await.unwrap().unwrap();
        assert!(exit_code.success());

        // Get the last value
        let mut response: Option<ContainerResponse> = None;
        while let Some(value) = child_io.child_stdout_rx.recv().await {
            response = Some(value);
        }
        let response = response.unwrap();
        assert!(matches!(response, ContainerResponse::Execute(_)));
        match response {
            ContainerResponse::Execute(response) => {
                let stderr = String::from_utf8_lossy(&response.stderr);
                assert_contains!(stderr, "Finished `release` profile");
                let exit_code = response.exit_code.unwrap();
                assert_eq!(exit_code, 0);
            }
        }
    }

    #[tokio::test]
    async fn test_beta_build() {
        // Gate test to only run on beta channel
        let version = rustc_version::version_meta().unwrap();
        if !matches!(version.channel, rustc_version::Channel::Beta) {
            return;
        }
        // Test Corust can run the beta toolchain
        let backend = init_test_backend();
        let container_factory = ContainerFactory::new(TEST_MAX_CONCURRENT_CONTAINERS);
        let container = container_factory.create_container(backend).await.unwrap();
        let ContainerRunRet {
            mut child,
            mut child_io,
        } = container.run(Channel::Beta).await.unwrap();

        let execute_command = ExecuteCommand::new(
            "fn main() {
                if let Ok(toolchain) = std::env::var(\"RUSTUP_TOOLCHAIN\") {
                    println!(\"{}\", toolchain);
                }
            }"
            .to_string(),
            TargetType::Binary,
            CargoCommand::Run,
            OptLevel::Release,
            Channel::Beta,
        );

        let message = ContainerMessage::Execute(execute_command);
        child_io
            .child_stdin_tx
            .as_ref()
            .unwrap()
            .send(message)
            .await
            .unwrap();

        let exit_code = child.wait().with_timeout().await.unwrap().unwrap();
        assert!(exit_code.success());

        // Get the last value
        let mut response: Option<ContainerResponse> = None;
        while let Some(value) = child_io.child_stdout_rx.recv().await {
            response = Some(value);
        }
        let response = response.unwrap();
        assert!(matches!(response, ContainerResponse::Execute(_)));
        match response {
            ContainerResponse::Execute(response) => {
                let stdout = String::from_utf8_lossy(&response.stdout);
                assert_contains!(stdout, "beta");
                let exit_code = response.exit_code.unwrap();
                assert_eq!(exit_code, 0);
            }
        }
    }
}
