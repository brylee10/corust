//! Error handling for the server

use serde::Serialize;
use warp::http::StatusCode;
use warp::reply::{Response, json, with_status};
use warp::{Filter, Rejection, Reply};

use crate::execute;

#[derive(Serialize)]
pub struct ErrorMessage {
    code: u16,
    message: String,
}

pub fn favicon_route() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path("favicon.ico")
        .and_then(|| async { Err::<Response, Rejection>(warp::reject::not_found()) })
}

pub async fn handle_rejection(err: Rejection) -> Result<impl Reply, warp::Rejection> {
    let code;
    let message;

    if let Some(sandbox_err) = err.find::<execute::metadata::MetadataError>() {
        // Determine appropriate status code based on error variant
        code = StatusCode::INTERNAL_SERVER_ERROR;
        message = format!(
            "An unexpected error occurred in the sandbox: {}",
            sandbox_err
        );
    } else {
        log::error!("Unhandled rejection: {:?}", err);
        code = StatusCode::INTERNAL_SERVER_ERROR;
        message = "UNHANDLED_REJECTION".to_string();
    }

    let json = json(&ErrorMessage {
        code: code.as_u16(),
        message,
    });

    Ok(with_status(json, code))
}
