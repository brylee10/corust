use std::net::SocketAddr;
use std::sync::Arc;

use ansi_term::Color;
use dotenv;
use env_logger::Builder;
use log::Level;
use tokio::sync::Mutex;

use std::io::Write;
use warp::Filter;

use corust_app::sessions::SessionMap;
use corust_app::users::user_join_route;
use corust_app::websocket::*;
use corust_sandbox::container::ContainerFactory;

const MAX_CONCURRENT_CONTAINERS: usize = 100;

#[tokio::main]
async fn main() {
    // Will resolve to `None` in prod where `.env` is not present
    dotenv::dotenv().ok();

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
    let session_map = SessionMap::new();
    let session_map = Arc::new(Mutex::new(session_map));
    let container_factory = Arc::new(Mutex::new(ContainerFactory::new(MAX_CONCURRENT_CONTAINERS)));

    // warp::ws() is composed of many filters to handle HTTP -> websocket upgrade
    let websocket_route = websocket_route(Arc::clone(&session_map), Arc::clone(&container_factory));
    let user_join_route = user_join_route(Arc::clone(&session_map));

    let cors_origin = std::env::var("FRONT_END_URI")
        .unwrap_or_else(|e| panic!("FRONT_END_URI must be set, {}", e));
    log::info!("CORS origin set to: {}", cors_origin);
    let cors = warp::cors()
        .allow_origin(cors_origin.as_str())
        // `PUT` and `DELETE` not valid endpoints for this server
        .allow_methods(vec!["POST", "GET"])
        .allow_headers(vec!["Authorization", "Content-Type"]);

    let routes = websocket_route.or(user_join_route).with(cors);

    let addr = std::env::var("WS_SERVER_URI")
        .unwrap_or_else(|e| panic!("WS_SERVER_URI must be set, {}", e));
    let port = std::env::var("PORT").unwrap_or_else(|e| panic!("PORT must be set, {}", e));
    log::info!("Listening on {}:{}", addr, port);
    let socket_addr = format!("{}:{}", addr, port);
    let socket_addr: SocketAddr = socket_addr.parse().expect("Invalid socket address");
    warp::serve(routes).run(socket_addr).await;
}
