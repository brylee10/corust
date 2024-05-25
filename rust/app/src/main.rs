mod messages;
pub mod runner;
mod sessions;
mod users;
mod websocket;

use std::sync::Arc;

use ansi_term::Color;
use env_logger::Builder;
use log::Level;
use messages::AppState;
use tokio::sync::Mutex;

use std::io::Write;
use warp::Filter;

use crate::runner::compile::*;
use crate::sessions::SessionMap;
use crate::users::user_join_route;
use crate::websocket::*;
use corust_sandbox::container::ContainerFactory;

const MAX_CONCURRENT_CONTAINERS: usize = 100;

#[tokio::main]
async fn main() {
    let mut builder = Builder::from_default_env();
    builder
        .format(|buf, record| {
            let level = match record.level() {
                Level::Error => Color::Red.paint("ERROR"),
                Level::Warn => Color::Yellow.paint("WARN"),
                Level::Info => Color::Green.paint("INFO"),
                Level::Debug => Color::Blue.paint("DEBUG"),
                Level::Trace => Color::Purple.paint("TRACE"),
            };

            writeln!(
                buf,
                "[{} {}:{}] {}",
                level,
                record.file().unwrap_or("unknown"),
                record.line().unwrap_or(0),
                record.args()
            )
        })
        .init();

    log::info!("Starting Rust server! 🚀");
    let app_state = AppState {
        last_code: Default::default(),
        last_run: None,
    };
    let session_map = SessionMap::new();
    let session_map = Arc::new(Mutex::new(session_map));
    let state = Arc::new(Mutex::new(app_state));

    let _container_factory = Arc::new(ContainerFactory::new(MAX_CONCURRENT_CONTAINERS));

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
