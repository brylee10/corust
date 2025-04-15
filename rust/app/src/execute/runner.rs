use std::{
    process::ExitStatus,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use corust_components::{
    RunConfig, RunConfigAction, RunConfigExec, RunStateUpdate, RunStatus, ServerMessage,
};
use corust_sandbox::container::{CommanderError, ContainerError, ContainerFactory, DockerBackend};
use corust_types::{
    ContainerMessage, ContainerResponse, ExecuteCommand, ExecuteResponse, RunnerOutput,
};
use fnv::FnvHashMap;
use futures_util::SinkExt;
use strum::{Display, IntoEnumIterator};
use strum::{EnumIter, EnumString};
use thiserror::Error;
use tokio::sync::{
    broadcast,
    mpsc::{Sender, error::SendError},
};
use warp::filters::ws::Message;

use crate::{sessions::SharedSession, websocket::SharedWsSender};

pub type SharedContainerFactory = Arc<ContainerFactory<DockerBackend>>;
pub type SharedConcurrentRunChecker = Arc<ConcurrentRunChecker>;

#[derive(Debug, Error)]
pub enum RunCodeError {
    #[error(transparent)]
    SendContainerMessageFailed(#[from] SendError<ContainerMessage>),
    #[error(transparent)]
    SendContainerResponseFailed(#[from] SendError<ContainerResponse>),
    #[error(transparent)]
    ContainerError(#[from] ContainerError),
    #[error("Runner exited code execution with non zero code: {0}")]
    RunnerNonZeroExit(ExitStatus),
    #[error("Container already running code of type {0:?}")]
    ConcurrentCompilation(RunType),
    #[error(transparent)]
    JoinError(#[from] tokio::task::JoinError),
    #[error(transparent)]
    CommanderError(#[from] CommanderError),
}

/// Checks if concurrent complations of the same type are occurring. One per session.
pub struct ConcurrentRunChecker {
    is_running: FnvHashMap<RunType, AtomicBool>,
}

impl ConcurrentRunChecker {
    pub(crate) fn new() -> Self {
        // Enumerates over all `CompilationTypes` and initializes them to false
        let is_running = RunType::iter()
            .map(|run_type| (run_type, AtomicBool::new(false)))
            .collect();
        ConcurrentRunChecker { is_running }
    }

    // Compares and exchanges the value of the flag for the given `RunType`
    pub(crate) fn compare_exchange(
        &self,
        run_type: RunType,
        current: bool,
        new: bool,
    ) -> Result<bool, bool> {
        // unwrap: all `RunType` variants are initialized in `new()
        self.is_running.get(&run_type).unwrap().compare_exchange(
            current,
            new,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
    }
}

#[derive(Debug, PartialEq, Eq, Hash, EnumIter, EnumString, Display, Clone, Copy)]
pub enum RunType {
    /// Runs a command line program, currently rustc or cargo
    Execute,
    // Future: Miri, WASM, etc
}

impl From<&ContainerMessage> for RunType {
    fn from(container_msg: &ContainerMessage) -> Self {
        match container_msg {
            ContainerMessage::Execute { .. } => RunType::Execute,
            ContainerMessage::Standalone { .. } => RunType::Execute,
        }
    }
}

impl From<&ContainerResponse> for RunType {
    fn from(container_response: &ContainerResponse) -> Self {
        match container_response {
            ContainerResponse::Execute { .. } => RunType::Execute,
        }
    }
}

pub(crate) fn container_response_to_runner_output(
    container_response: &ContainerResponse,
) -> RunnerOutput {
    match container_response {
        ContainerResponse::Execute(ExecuteResponse {
            stdout,
            stderr,
            exit_code,
            ..
        }) => {
            let stdout = String::from_utf8_lossy(stdout).to_string();
            let stderr = String::from_utf8_lossy(stderr).to_string();

            RunnerOutput {
                run_type: RunType::Execute.to_string(),
                stdout,
                stderr,
                exit_code: *exit_code,
            }
        }
    }
}

// Broadcasts run progress and completion to all clients
struct RunProgressNotifier {
    session: SharedSession,
    run_type: RunType,
    bcast_tx: broadcast::Sender<ServerMessage>,
}

impl Clone for RunProgressNotifier {
    fn clone(&self) -> Self {
        RunProgressNotifier {
            session: self.session.clone(),
            run_type: self.run_type,
            bcast_tx: self.bcast_tx.clone(),
        }
    }
}

impl RunProgressNotifier {
    fn new(
        session: SharedSession,
        run_type: RunType,
        bcast_tx: broadcast::Sender<ServerMessage>,
    ) -> Self {
        RunProgressNotifier {
            session,
            run_type,
            bcast_tx,
        }
    }

    // Acquires the code lock for the session. Returns an error if a concurrent compilation.
    async fn try_acquire_code_lock(&self) -> Result<(), RunCodeError> {
        // Acquire and drop the session lock. Do not hold it across the container run.
        log::debug!("Before acquire session lock in run_code");
        log::debug!("Acquired session lock in run_code");
        let concurrent_run_checker = self.session.concurrent_run_checker();
        if let Err(e) = concurrent_run_checker.compare_exchange(self.run_type, false, true) {
            assert!(
                e,
                "Concurrent compilation check only returns error when a compilation of a RunType already exists"
            );
            return Err(RunCodeError::ConcurrentCompilation(self.run_type));
        }
        bcast_code_run_started(RunType::Execute, &self.bcast_tx);
        Ok(())
    }
}

impl Drop for RunProgressNotifier {
    fn drop(&mut self) {
        log::debug!("RunProgressNotifier drop called");
        // Reset the concurrent run flag
        let concurrent_run_checker = self.session.concurrent_run_checker();
        assert!(
            concurrent_run_checker
                .compare_exchange(self.run_type, true, false)
                .unwrap(),
            "Previous compilation should have been running"
        );
        bcast_code_run_finished(RunType::Execute, &self.bcast_tx);
    }
}

/// Constructs messages to compile or execute user provided code using `cargo`
/// Only allows one concurrent execution of a given `RunType` for a session.
pub(crate) async fn run_code(
    execute_command: ExecuteCommand,
    session: SharedSession,
    container_factory: SharedContainerFactory,
    container_response_tx: Sender<ContainerResponse>,
    bcast_tx: broadcast::Sender<ServerMessage>,
    username: String,
) -> Result<(), RunCodeError> {
    // Check and disallow concurrent compilations in the same session
    let container_msg = ContainerMessage::Execute(execute_command.clone());
    let run_type = RunType::from(&container_msg);
    let run_progress_notifier =
        RunProgressNotifier::new(session.clone(), run_type, bcast_tx.clone());
    run_progress_notifier.try_acquire_code_lock().await?;
    let channel = execute_command.channel;

    let mut container = container_factory.create_container(channel).await?;
    // Shared factory no longer needed
    std::mem::drop(container_factory);

    // Inform other clients about run configuration change
    let run_config = RunConfig {
        opt_level: execute_command.opt_level,
        channel,
        cargo_command: execute_command.cargo_command,
    };
    session.set_run_config(run_config);
    let run_config_msg =
        ServerMessage::RunConfigAction(RunConfigAction::RecentExecution(RunConfigExec {
            run_config,
            code: execute_command.code.clone(),
            username,
        }));
    if let Err(e) = bcast_tx.send(run_config_msg) {
        // Not an error, just means all receiver handles have been closed
        log::info!("All receiver handles have been closed. {e:?}");
    }

    // Implicit starting state of all executions, an empty stdout/stdin. Useful to reset all users previous output
    // if existing from previous runs.
    // Note, the run starting and output clear is not in the same message so not atomic.
    let clear_output = ContainerResponse::Execute(ExecuteResponse {
        stdout: Vec::new(),
        stderr: Vec::new(),
        exit_code: None,
    });

    container_response_tx.send(clear_output).await?;

    let commander = container.execute_request(execute_command).await?;
    let exit_status = commander
        .stream_responses(container_response_tx.clone())
        .await?;

    // Only return error on non zero exit code after the concurrent run flag is reset
    if !exit_status.success() {
        log::error!(
            "Runner exited code execution with non zero code: {:?}",
            exit_status
        );
        return Err(RunCodeError::RunnerNonZeroExit(exit_status));
    }
    Ok(())
}

// Helpers for sending ws notifications
pub(crate) async fn ws_notify_concurrent_code_error(
    shared_ws_tx: SharedWsSender,
    run_type: RunType,
) {
    // Send error back to client (not a broadcast)
    let server_message = ServerMessage::RunStatus(RunStatus {
        run_type: run_type.to_string(),
        run_state_update: RunStateUpdate::ConcurrentCompilation,
    });
    let msg = serde_json::to_string(&server_message).unwrap();
    let msg = Message::text(msg);
    if let Err(e) = shared_ws_tx.write().await.send(msg).await {
        log::info!("All receiver handles have been closed. {e:?}");
    }
}

pub(crate) async fn bcast_notify_output_size_error(
    bcast_tx: broadcast::Sender<ServerMessage>,
    run_type: RunType,
) {
    // Broadcast error to all clients, not 1-1 because output size affects all client runs
    let server_message = ServerMessage::RunStatus(RunStatus {
        run_type: run_type.to_string(),
        run_state_update: RunStateUpdate::StdoutErrTooLarge,
    });
    if let Err(e) = bcast_tx.send(server_message) {
        log::info!("All receiver handles have been closed. {e:?}");
    }
}

pub(crate) fn bcast_code_run_started(
    run_type: RunType,
    bcast_tx: &broadcast::Sender<ServerMessage>,
) {
    let run_started_msg = ServerMessage::RunStatus(RunStatus {
        run_type: run_type.to_string(),
        run_state_update: RunStateUpdate::RunStarted,
    });
    if let Err(e) = bcast_tx.send(run_started_msg) {
        // Not an error, just means all receiver handles have been closed
        log::info!("All receiver handles have been closed. {e:?}");
        // Handle error (e.g., all receiver handles have been closed)
    }
}

pub(crate) fn bcast_code_run_finished(
    run_type: RunType,
    bcast_tx: &broadcast::Sender<ServerMessage>,
) {
    // Broadcast error back to client
    let run_ended_msg = ServerMessage::RunStatus(RunStatus {
        run_type: run_type.to_string(),
        run_state_update: RunStateUpdate::RunEnded,
    });
    if let Err(e) = bcast_tx.send(run_ended_msg) {
        // Not an error, just means all receiver handles have been closed
        log::info!("All receiver handles have been closed. {e:?}");
        // Handle error (e.g., all receiver handles have been closed)
    }
}
