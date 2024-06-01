use std::sync::Arc;

use corust_components::RunnerOutput;
use corust_sandbox::container::{
    ContainerFactory, ContainerMessage, ContainerResponse, ContainerRunRet,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use warp::{http::StatusCode, reply};

use crate::{
    messages::{CompileRejections, Rejections},
    sessions::SharedSession,
};

pub type SharedContainerFactory = Arc<Mutex<ContainerFactory>>;

// Server side in memory storage of most recent code and run output for one session
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct CodeOutputState {
    pub container_message: ContainerMessage,
    pub runner_output: Option<RunnerOutput>,
}

pub(crate) async fn compile_code(
    container_message: ContainerMessage,
    _session: SharedSession,
    container_factory: SharedContainerFactory,
) -> Result<impl warp::Reply, warp::Rejection> {
    let container_factory = container_factory.lock().await;
    let container = match container_factory.create_container_docker_backend().await {
        Ok(container) => container,
        Err(e) => {
            log::error!("Error creating container: {}", e);
            return Err(warp::reject::custom(Rejections::Compile(
                CompileRejections::UnableToCreateBackend,
            )));
        }
    };
    // Factory no longer needed
    std::mem::drop(container_factory);

    let ContainerRunRet {
        mut child,
        mut child_io,
    } = match container.run().await {
        Ok(container_run_ret) => container_run_ret,
        Err(e) => {
            log::error!("Error running container: {}", e);
            return Err(warp::reject::custom(Rejections::Compile(
                CompileRejections::RunContainerFailed,
            )));
        }
    };

    match child_io.child_stdin_tx.send(container_message).await {
        Ok(_) => {}
        Err(e) => {
            log::error!("Error sending message to container: {}", e);
            return Err(warp::reject::custom(Rejections::Compile(
                CompileRejections::SendContainerMessageFailed,
            )));
        }
    }

    let exit_code = child.wait().await.unwrap();
    if !exit_code.success() {
        return Err(warp::reject::custom(Rejections::Compile(
            CompileRejections::RunNonZeroExit,
        )));
    }

    // Get the last value
    let mut response = None;
    while let Some(value) = child_io.child_stdout_rx.recv().await {
        response = Some(value);
    }
    let response = response.unwrap();
    match response {
        ContainerResponse::Execute(response) => {
            // Simulate running the code and getting output and status code
            let code = StatusCode::OK;
            let reply = reply::json(&response);
            let reply = reply::with_header(reply, "Access-Control-Allow-Origin", "*");

            Ok(reply::with_status(reply, code))
        }
    }
}

// pub(crate) fn compile_code_route(
//     session: SharedSession,
//     container_factory: SharedContainerFactory,
// ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
//     warp::path("compile")
//         .and(warp::post())
//         .and(warp::body::json())
//         // Implicitly uses `serde_json` to convert bytes to the mapped type (`ContainerMessage`)
//         // `compile_code` is falliable, so prefer `and_then`
//         .and_then(move |container_message: ContainerMessage| {
//             log::debug!("Received code: {compile_request:?}");
//             // Compile the code
//             compile_code(compile_request, session.clone(), container_factory.clone())
//         })
// }
