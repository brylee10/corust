use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::http::Method;
use axum::routing::{get, post};
use axum::{Router, http};
use corust_app::db::{CompilationTable, DocumentTable, Table, UserTable};
use corust_app::execute::metadata::metadata;
use corust_app::sandbox_metadata::SandboxMetadata;

use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

use corust_app::background::spawn_background_session_managers;
use corust_app::sessions::{SessionMap, SharedSessionMap};
use corust_app::users::{join_session_no_user, join_session_with_user};
use corust_app::{AppState, favicon, handler_404, root_page, websocket::*};
use corust_sandbox::container::{ContainerFactory, DockerBackend};

/// The maximum number of concurrent containers that can be running at once. Used to avoid overloading the CPU
/// with many concurrent CPU intensive tasks which significantly outnumber the number of cores.
/// The playground defaults to 10: https://github.com/rust-lang/rust-playground/blob/main/ui/src/main.rs#L23
const MAX_CONCURRENT_CONTAINERS: usize = 8;

#[tokio::main]
async fn main() {
    // Will resolve to `None` in prod where `.env` is not present
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .init();

    tracing::info!("Starting Rust server! 🚀");
    let sandbox_metadata = Arc::new(SandboxMetadata::default());
    let session_map: SharedSessionMap = Arc::new(SessionMap::new());
    let container_factory = Arc::new(ContainerFactory::new(
        MAX_CONCURRENT_CONTAINERS,
        DockerBackend::new(),
    ));

    // Initialize database tables
    let db_path: PathBuf = std::env::var("DB_PATH")
        .unwrap_or_else(|e| panic!("DB_PATH must be set, {}", e))
        .into();
    let document_table = DocumentTable::new(db_path.clone());
    let user_table = UserTable::new(db_path.clone());
    let compilation_table = CompilationTable::new(db_path.clone());
    // unwrap: server start up should fail if the tables cannot be created
    document_table.create().unwrap();
    user_table.create().unwrap();
    compilation_table.create().unwrap();

    // Start a background tasks to archive empty sessions and remove inactive users
    spawn_background_session_managers(Arc::clone(&session_map), db_path.clone());

    let app_state = AppState {
        session_map: Arc::clone(&session_map),
        container_factory: Arc::clone(&container_factory),
        sandbox_metadata: Arc::clone(&sandbox_metadata),
        db_path: db_path.clone(),
    };

    let cors_origin = std::env::var("FRONT_END_URI")
        .unwrap_or_else(|e| panic!("FRONT_END_URI must be set, {}", e));
    tracing::info!("CORS origin set to: {}", cors_origin);

    let cors = CorsLayer::new()
        .allow_origin(cors_origin.parse::<axum::http::HeaderValue>().unwrap())
        .allow_methods([Method::GET, Method::POST])
        .allow_headers(vec![
            http::header::AUTHORIZATION,
            http::header::CONTENT_TYPE,
        ]);

    let app = Router::new()
        .route("/websocket/{session_id}/{user_id}", get(websocket))
        .route("/join/{session_id}/{user_id}", post(join_session_with_user))
        .route("/join/{session_id}", post(join_session_no_user))
        .route("/", get(root_page))
        .route("/metadata/versions", get(metadata))
        .route("/favicon.ico", get(favicon))
        .with_state(app_state)
        .layer(cors)
        .fallback(handler_404);

    let addr = std::env::var("WS_SERVER_URI")
        .unwrap_or_else(|e| panic!("WS_SERVER_URI must be set, {}", e));
    // Heroku sets `PORT` per web process and routes all requests to this port
    // https://devcenter.heroku.com/articles/runtime-principles#web-servers
    let port = std::env::var("PORT").unwrap_or_else(|e| panic!("PORT must be set, {}", e));
    tracing::info!("Listening on {}:{}", addr, port);
    let socket_addr = format!("{}:{}", addr, port);
    let socket_addr: SocketAddr = socket_addr.parse().expect("Invalid socket address");
    let listener = tokio::net::TcpListener::bind(socket_addr).await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}
