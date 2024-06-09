// Inspiration taken from Rust Playground `worker.rs`:
// https://github.com/rust-lang/rust-playground/blob/main/compiler/base/orchestrator/src/worker.rs

use corust_sandbox::container::{
    ContainerMessage, ContainerResponse, ExecuteCommand, ExecuteResponse, TargetType,
    IO_COMPONENT_CHANNEL_SIZE,
};
use corust_sandbox::init_logger;
use corust_sandbox::runner::{
    create_runner_io_component, JoinTaskSnafu, ReadStdoutSnafu, Result, RunnerError,
    RunnerIoComponent, SendResponseSnafu, SpawnChildSnafu, StderrCaptureSnafu, StdoutCaptureSnafu,
    WaitChildSnafu, WriteCodeSnafu,
};
use env_logger::Target;
use snafu::{OptionExt, ResultExt};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::{env, process::Stdio};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::join;
use tokio::process::Command;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

// Number of bytes to read from child stdout and stderr at a time
const CHILD_PIPE_BUFFER_SIZE: usize = 1024;

const BINARY_OUTPUT: &str = "src/main.rs";
const LIB_OUTPUT: &str = "src/lib.rs";

// Read [`ContainerMessage`]s to execute from `stdin`
async fn listen<P: AsRef<Path>>(
    project_dir: P,
    runner_io_component: RunnerIoComponent,
    mut stdin_rx: Receiver<ContainerMessage>,
    stdout_tx: Sender<ContainerResponse>,
) -> Result<()> {
    let project_dir = project_dir.as_ref();
    let mut stdin_done = false;
    let mut stdout_tx = Some(stdout_tx);
    let stdout_handle = runner_io_component.stdout_handle;

    let handle_stdin_rx = async {
        loop {
            if stdin_done {
                break;
            }
            let next_stdin_msg = stdin_rx.recv().await;
            let stdout_tx_inner = stdout_tx.take();
            if let Some(stdout_tx_inner) = stdout_tx_inner {
                match next_stdin_msg {
                    Some(msg) => {
                        log::debug!("Received message: {:?}", msg);
                        match msg {
                            ContainerMessage::Execute(execute_command) => {
                                handle_execute_cmd(
                                    &project_dir,
                                    execute_command,
                                    stdout_tx_inner.clone(),
                                )
                                .await?;
                            }
                        }
                        // The runner can execute one command, then it can receive no more
                        stdin_rx.close();
                        log::debug!("Runner done processing command");
                        stdin_done = true;
                    }
                    None => {
                        // End of stream
                        log::debug!("End of stream");
                        stdin_done = true;
                    }
                }
                if !stdin_done {
                    stdout_tx.replace(stdout_tx_inner);
                } else {
                    // Otherwise, drop the last clone of `stdout_tx` which closes `stdout_rx`
                }
            }
        }
        log::info!("Finished handling stdin_rx");
        Ok::<(), RunnerError>(())
    };

    // // Waits for writing to stdout to finish and stdin rx to close
    let (stdout_res, stdin_res) = join!(stdout_handle, handle_stdin_rx);
    stdout_res.context(JoinTaskSnafu)??;
    stdin_res?;
    // No longer receive messages from stdin because stdin_rx is closed
    runner_io_component.stdin_handle.abort();
    assert!(runner_io_component
        .stdin_handle
        .await
        .unwrap_err()
        .is_cancelled());
    Ok(())
}

// Execute command in `command` in a Rust project at `project_dir`
async fn handle_execute_cmd<P: AsRef<Path>>(
    project_dir: P,
    command: ExecuteCommand,
    stdout_tx: Sender<ContainerResponse>,
) -> Result<()> {
    let project_dir = project_dir.as_ref();
    let ExecuteCommand {
        code,
        target_type,
        cargo_command,
    } = command;

    // Conditionally write the `code` to a file based on the `target_type`
    // Conditionally run `cargo_command` based on the `target_type`
    let output_path = match target_type {
        TargetType::Binary => project_dir.join(BINARY_OUTPUT),
        TargetType::Library => project_dir.join(LIB_OUTPUT),
    };

    log::debug!("Writing code to {:?}", output_path);
    // Write code to file
    fs::write(&output_path, code).context(WriteCodeSnafu {
        output: output_path.to_path_buf(),
    })?;

    // Runs `cargo [command]` in the project directory inside a sandboxed Docker container
    let mut cmd: Command = cargo_command.into();
    cmd.current_dir(project_dir);
    log::debug!(
        "Executing command: {:?} in project dir {:?}",
        cmd,
        project_dir
    );

    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(SpawnChildSnafu)?;

    let child_stdout = child.stdout.take().context(StdoutCaptureSnafu)?;
    let child_stderr = child.stderr.take().context(StderrCaptureSnafu)?;

    let container_response = Arc::new(Mutex::new(ExecuteResponse {
        stdout: vec![],
        stderr: vec![],
        exit_code: None,
    }));

    let stdout_handle: JoinHandle<Result<(), RunnerError>> = tokio::spawn({
        let container_response = container_response.clone();
        let stdout_tx = stdout_tx.clone();
        async move {
            let mut reader = BufReader::new(child_stdout);
            let mut buffer = [0u8; CHILD_PIPE_BUFFER_SIZE];
            let container_response = container_response.clone();
            loop {
                match reader.read(&mut buffer).await.context(ReadStdoutSnafu)? {
                    0 => {
                        log::info!("Child stdout has closed");
                        break;
                    }
                    n => {
                        log::debug!("Read {} bytes from child stdout", n);
                        let mut response = container_response.lock().await;
                        response.stdout.extend(&buffer[..n]);
                        stdout_tx
                            .send(ContainerResponse::Execute(response.clone()))
                            .await
                            .context(SendResponseSnafu)?;
                    }
                }
            }
            Ok(())
        }
    });

    let stderr_handle: JoinHandle<Result<(), RunnerError>> = tokio::spawn({
        let container_response = container_response.clone();
        let stdout_tx = stdout_tx.clone();
        async move {
            let mut reader = BufReader::new(child_stderr);
            let mut buffer = [0u8; CHILD_PIPE_BUFFER_SIZE];
            loop {
                match reader.read(&mut buffer).await.context(ReadStdoutSnafu)? {
                    0 => {
                        log::info!("Child stderr has closed");
                        break;
                    }
                    n => {
                        log::debug!("Read {} bytes from child stderr", n);
                        log::debug!("Read string: {}", String::from_utf8_lossy(&buffer[..n]));
                        let mut response = container_response.lock().await;
                        response.stderr.extend(&buffer[..n]);
                        // Both cargo stderr and stdout are serialized to runner stdout
                        stdout_tx
                            .send(ContainerResponse::Execute(response.clone()))
                            .await
                            .context(SendResponseSnafu)?;
                    }
                }
            }
            Ok(())
        }
    });

    let exit_status = child.wait().await.context(WaitChildSnafu)?;
    log::debug!("Child process exited with: {:?}", exit_status);
    let mut response = container_response.lock().await;
    response.exit_code = exit_status.code();
    stdout_tx
        .send(ContainerResponse::Execute(response.clone()))
        .await
        .context(SendResponseSnafu)?;

    let res = tokio::try_join!(stdout_handle, stderr_handle);
    match res {
        Ok((stdout_res, stderr_res)) => {
            stdout_res?;
            stderr_res?;
        }
        Err(e) => {
            log::error!("Error reading stdout/stderr: {:?}", e);
            return Err(RunnerError::JoinTask { source: e });
        }
    }
    log::debug!("Execute command finished handling");
    Ok(())
}

#[snafu::report]
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize env logger, writes to stderr
    init_logger(Target::Stderr);
    let project_dir = env::args_os()
        .nth(1)
        .expect("Please specify Rust project directory as the first argument");

    // Channel which roduces output from stdin to a channel and sends to stdout
    // Read from stdin to get serialized [`ContainerMessage`]s, write serialized [`ContainerResponse`] to stdout
    let (stdout_tx, stdout_rx) = mpsc::channel(IO_COMPONENT_CHANNEL_SIZE);
    let (stdin_tx, stdin_rx) = mpsc::channel(IO_COMPONENT_CHANNEL_SIZE);
    let runner_io_component = create_runner_io_component(stdin_tx, stdout_rx)?;

    let res = listen(project_dir, runner_io_component, stdin_rx, stdout_tx).await;
    match res {
        Ok(()) => {
            log::debug!("Finished listening for messages");
            std::process::exit(0);
        }
        Err(e) => {
            log::error!("Error listening for messages: {:?}", e);
            std::process::exit(1);
        }
    }
}
