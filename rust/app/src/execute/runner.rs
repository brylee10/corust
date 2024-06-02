use std::{
    process::ExitStatus,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use corust_components::RunnerOutput;
use corust_sandbox::container::{
    ContainerError, ContainerFactory, ContainerMessage, ContainerResponse, ContainerRunRet,
    ExecuteResponse,
};
use fnv::FnvHashMap;
use serde::{Deserialize, Serialize};
use strum::{Display, IntoEnumIterator};
use strum::{EnumIter, EnumString};
use thiserror::Error;
use tokio::sync::{
    mpsc::{error::SendError, Sender},
    Mutex,
};

use crate::sessions::SharedSession;

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

// Only allows one concurrent execution of a given `RunType` for a session.
pub(crate) async fn run_code(
    container_msg: ContainerMessage,
    session: SharedSession,
    container_factory: SharedContainerFactory,
    container_response_tx: Sender<ContainerResponse>,
) -> Result<(), RunCodeError> {
    // Check and disallow concurrent compilations in the same session
    let run_type = RunType::from(&container_msg);
    let session = session.lock().await;
    let concurrent_run_checker = session.concurrent_run_checker();
    let concurrent_run_checker = concurrent_run_checker.lock().await;
    if let Err(e) = concurrent_run_checker.compare_exchange(run_type, false, true) {
        assert!(e, "Concurrent compilation check only returns error when a compilation of a RunType already exists");
        log::error!("Concurrent compilation type: {:?}", run_type);
        return Err(RunCodeError::ConcurrentCompilation(run_type));
    }

    let container_factory = container_factory.lock().await;
    let container = container_factory.create_container_docker_backend().await?;
    // Factory no longer needed
    std::mem::drop(container_factory);

    let ContainerRunRet {
        mut child,
        mut child_io,
    } = container.run().await?;

    child_io.child_stdin_tx.send(container_msg).await?;

    let exit_code = child.wait().await.unwrap();
    if !exit_code.success() {
        return Err(RunCodeError::RunnerNonZeroExit(exit_code));
    }

    // Get the last value
    while let Some(value) = child_io.child_stdout_rx.recv().await {
        container_response_tx.send(value).await?;
    }

    // Compilation finished
    assert!(
        concurrent_run_checker
            .compare_exchange(run_type, true, false)
            .unwrap(),
        "Previous compilation should have been running"
    );
    Ok(())
}
