mod assets;
mod config;
mod db;
mod error;
mod ffprobe;
mod filesystem;
mod formatting;
mod importer;
mod jobs;
mod library;
mod models;
mod naming;
mod parser;
mod providers;
mod routes;
mod state;

use std::net::SocketAddr;

use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::config::load_config;
use crate::db::Database;
use crate::jobs::JobManager;
use crate::providers::MetadataProviders;
use crate::state::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tvsorter=info,tower_http=warn".into()),
        )
        .init();

    let config = load_config();
    let db = Database::open(&config.database_path).expect("failed to open database");
    let providers = MetadataProviders::new(db.clone());
    let state = AppState {
        config: config.clone(),
        db,
        providers,
        jobs: JobManager::new(),
    };

    // Permissive CORS so the Vite dev server (port 5173) can call the API.
    let cors = CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any);

    let app = routes::build_router(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .expect("invalid host/port");
    tracing::info!("TvSorter listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind address");
    axum::serve(listener, app)
        .await
        .expect("server error");
}
