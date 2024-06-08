use std::{
    process::ExitStatus,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use corust_components::{RunStateUpdate, RunStatus, RunnerOutput, ServerMessage};
use corust_sandbox::container::{
    ContainerError, ContainerFactory, ContainerMessage, ContainerResponse, ContainerRunRet,
    ExecuteResponse,
};
use fnv::FnvHashMap;
use futures_util::SinkExt;
use serde::{Deserialize, Serialize};
use strum::{Display, IntoEnumIterator};
use strum::{EnumIter, EnumString};
use thiserror::Error;
use tokio::sync::{
    broadcast,
    mpsc::{error::SendError, Sender},
    Mutex,
};
use warp::filters::ws::Message;

use crate::{sessions::SharedSession, websocket::SharedWsSender};

pub type SharedContainerFactory = Arc<Mutex<ContainerFactory>>;
pub type SharedConcurrentRunChecker = Arc<Mutex<ConcurrentRunChecker>>;

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
}

// Server side in memory storage of most recent code and run output for one session
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct CodeOutputState {
    pub container_msg: ContainerMessage,
    pub runner_output: Option<RunnerOutput>,
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
    Execute,
    // Future: Miri, WASM, etc
}

impl From<&ContainerMessage> for RunType {
    fn from(container_msg: &ContainerMessage) -> Self {
        match container_msg {
            ContainerMessage::Execute { .. } => RunType::Execute,
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

// Helper to reset concurrent run checker on drop and send a message to the client.
// Async drop design pattern taken from: https://stackoverflow.com/questions/71541765/rust-async-drop
struct RunProgressNotifier {
    session: SharedSession,
    run_type: RunType,
    bcast_tx: broadcast::Sender<ServerMessage>,
    // Indicates if drop logic should be run. Used to avoid repeated nested drops
    dropped: bool,
}

impl Clone for RunProgressNotifier {
    fn clone(&self) -> Self {
        RunProgressNotifier {
            session: self.session.clone(),
            run_type: self.run_type,
            bcast_tx: self.bcast_tx.clone(),
            dropped: self.dropped,
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
            dropped: false,
        }
    }

    async fn try_acquire_code_lock(&self) -> Result<(), RunCodeError> {
        // Acquire and drop the session lock. Do not hold it across the container run.
        log::debug!("Before acquire session lock in run_code");
        let session = self.session.lock().await;
        log::debug!("Acquired session lock in run_code");
        let concurrent_run_checker = session.concurrent_run_checker();
        let concurrent_run_checker = concurrent_run_checker.lock().await;
        if let Err(e) = concurrent_run_checker.compare_exchange(self.run_type, false, true) {
            assert!(e, "Concurrent compilation check only returns error when a compilation of a RunType already exists");
            return Err(RunCodeError::ConcurrentCompilation(self.run_type));
        }
        bcast_code_run_started(RunType::Execute, &self.bcast_tx);
        Ok(())
    }
}

impl Drop for RunProgressNotifier {
    fn drop(&mut self) {
        log::debug!("RunProgressNotifier drop called");
        // Drop cannot be async. Workaround by spawning an async task to reset the concurrent run flag
        if !self.dropped {
            log::debug!(
                "RunProgressNotifier not dropped. Running drop logic on RunProgressNotifier"
            );
            tokio::spawn({
                let mut self_clone = self.clone();
                // Avoids rerunning drop logic on the cloned `RunProgressNotifier`
                self_clone.dropped = true;
                async move {
                    log::debug!("Dropping RunProgressNotifier");
                    // Reset the concurrent run flag
                    let session = self_clone.session.lock().await;
                    let concurrent_run_checker = session.concurrent_run_checker();
                    let concurrent_run_checker = concurrent_run_checker.lock().await;
                    assert!(
                        concurrent_run_checker
                            .compare_exchange(self_clone.run_type, true, false)
                            .unwrap(),
                        "Previous compilation should have been running"
                    );
                    bcast_code_run_finished(RunType::Execute, &self_clone.bcast_tx);
                }
            });
        }
    }
}

// Only allows one concurrent execution of a given `RunType` for a session.
pub(crate) async fn run_code(
    container_msg: ContainerMessage,
    session: SharedSession,
    container_factory: SharedContainerFactory,
    container_response_tx: Sender<ContainerResponse>,
    bcast_tx: broadcast::Sender<ServerMessage>,
) -> Result<(), RunCodeError> {
    // Check and disallow concurrent compilations in the same session
    let run_type = RunType::from(&container_msg);
    let run_progress_notifier =
        RunProgressNotifier::new(session.clone(), run_type, bcast_tx.clone());
    run_progress_notifier.try_acquire_code_lock().await?;

    let container_factory = container_factory.lock().await;
    let container = container_factory.create_container_docker_backend().await?;
    // Factory no longer needed
    std::mem::drop(container_factory);

    let ContainerRunRet {
        mut child,
        mut child_io,
    } = container.run().await?;

    // `child_stdin_tx` is always `Some()` after construction via `container.run()`
    child_io
        .child_stdin_tx
        .as_ref()
        .unwrap()
        .send(container_msg)
        .await?;

    // Implicit starting state of all executions, an empty stdout/stdin. Useful to reset all users previous output
    // if existing from previous runs.
    // Note, the run starting and output clear is not in the same message so not atomic.
    let clear_output = ContainerResponse::Execute(ExecuteResponse::default());
    container_response_tx.send(clear_output).await?;

    // Read from child stdout until it closes. Do not wait the child before this otherwise
    // the task will wait until execution is complete so the intermediate stdout will not be streamed
    while let Some(container_response) = child_io.child_stdout_rx.recv().await {
        log::debug!("App runner received ContainerResponse");
        log::trace!("Container response: {:?}", container_response);
        container_response_tx.send(container_response).await?;
    }

    // Waiting for child should be fast since the stdout closing indicates the child is finished running
    let exit_code = child.wait().await.unwrap();
    log::debug!("Runner exited with code {:?}", exit_code);

    // Join tasks listening to child stdout and stderr
    while let Some(e) = child_io.tasks.join_next().await {
        // Tasks only join after the `child.wait()`, which indicates the child
        // has finished running. Drop stdin to allow `child_stdin_rx` to terminate and for all child io tasks to
        // join, otherwise only stderr/stdout will finish.
        std::mem::swap(&mut child_io.child_stdin_tx, &mut None);
        match e? {
            Ok(()) => {}
            Err(e) => {
                log::error!("Received container error: {:?}", e);
                return Err(e)?;
            }
        }
    }

    // Only return error on non zero exit code after the concurrent run flag is reset
    if !exit_code.success() {
        log::error!(
            "Runner exited code execution with non zero code: {:?}",
            exit_code
        );
        return Err(RunCodeError::RunnerNonZeroExit(exit_code));
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
    let mut shared_ws_tx = shared_ws_tx.lock().await;
    let msg = serde_json::to_string(&server_message).unwrap();
    let msg = Message::text(msg);
    if let Err(e) = shared_ws_tx.send(msg).await {
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
