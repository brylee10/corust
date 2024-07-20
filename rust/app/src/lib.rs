#![deny(dead_code)]

use warp::Filter;

pub mod execute;
pub mod messages;

pub mod sessions;
pub mod users;
pub mod websocket;

/// Returns a "connected" HTML message at the root path to sanity check server network connectivity
pub fn root_page() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path::end().map(|| warp::reply::html("Corust server connected"))
}
