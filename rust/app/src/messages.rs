use std::sync::Arc;

use tokio::sync::Mutex;

use serde::Serialize;

use corust_components::server::Server;
use warp::http::StatusCode;

pub type SharedServer = Arc<Mutex<Server>>;

#[derive(Debug, Clone, Copy)]
pub enum Rejections {
    Compile(CompileRejections),
}

#[derive(Debug, Clone, Copy)]
pub enum CompileRejections {
    UnableToCreateBackend,
    RunContainerFailed,
    RunNonZeroExit,
    SendContainerMessageFailed,
}

impl warp::reject::Reject for Rejections {}

impl From<Rejections> for warp::reply::Json {
    fn from(rejection: Rejections) -> Self {
        let (code, message) = match rejection {
            Rejections::Compile(CompileRejections::UnableToCreateBackend) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Unable to create backend".to_string(),
            ),
            Rejections::Compile(CompileRejections::RunContainerFailed) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Running container failed".to_string(),
            ),
            Rejections::Compile(CompileRejections::RunNonZeroExit) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Runner exited code execution with non zero code".to_string(),
            ),
            Rejections::Compile(CompileRejections::SendContainerMessageFailed) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Send container message failed".to_string(),
            ),
        };
        warp::reply::json(&ErrorResponse {
            code: code.as_u16(),
            message,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub code: u16,
    pub message: String,
}
