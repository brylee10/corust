use warp::Filter;

pub mod execute;
pub mod messages;

pub mod sessions;
pub mod users;
pub mod websocket;

/// Returns a "Server connected" HTML message at the root path
pub fn default_page() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path::end().map(|| warp::reply::html("Corust server connected"))
}
