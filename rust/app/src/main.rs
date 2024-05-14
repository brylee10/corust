mod compile;
mod messages;
mod sessions;
mod users;
mod websocket;

use std::sync::Arc;

use env_logger::Builder;
use messages::AppState;
use tokio::sync::Mutex;

use std::io::Write;
use warp::Filter;

use crate::compile::*;
use crate::sessions::SessionMap;
use crate::users::user_join_route;
use crate::websocket::*;

// // Map of SessionID (hash) to SharedAppState. Each request updates a particular session?
// // Users will send the session ID with each request
// // On join, the server assigns a session?
// // Maintains a connection count per session, and if all connections are closed, the session is removed from the map
// struct Session {
//     // Unique hash respresenting the session
//     session_id: String,
//     // Shared state for the session
//     state: SharedAppState,
//     // Number of connections to the session
//     connection_count: u32,
// }

#[tokio::main]
async fn main() {
    let mut builder = Builder::from_default_env();
    builder
        .format(|buf, record| writeln!(buf, "{} - {}", record.level(), record.args()))
        .init();

    log::info!("Starting Rust server! 🚀");
    let app_state = AppState {
        last_code: Default::default(),
        last_run: None,
    };
    let session_map = SessionMap::new();
    let session_map = Arc::new(Mutex::new(session_map));
    let state = Arc::new(Mutex::new(app_state));

    // warp::ws() is composed of many filters to handle HTTP -> websocket upgrade
    let websocket_route = websocket_route(Arc::clone(&session_map));
    let compile_code_route = compile_code_route(Arc::clone(&state));
    let user_join_route = user_join_route(Arc::clone(&session_map));

    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["POST", "GET", "PUT", "DELETE"])
        .allow_headers(vec!["Authorization", "Content-Type"]);

    let routes = websocket_route
        .or(compile_code_route)
        .or(user_join_route)
        .with(cors);

    warp::serve(routes).run(([127, 0, 0, 1], 8000)).await;
}
