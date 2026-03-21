// NOTE: Run `npm run build` in admin-ui/ before starting admin-server to serve the frontend.

mod auth;
mod routes;
mod state;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use axum::{
    routing::{delete, get, post, put},
    Router,
};
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use state::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            "admin_server=info,tower_http=info".into()
        }))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Parse CLI args
    let args: Vec<String> = std::env::args().collect();
    let port: u16 = parse_arg(&args, "--port")
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let data_dir = parse_arg(&args, "--data-dir").unwrap_or_else(|| "./data".to_string());
    let admin_token = parse_arg(&args, "--admin-token")
        .or_else(|| std::env::var("ICEDB_ADMIN_TOKEN").ok())
        .unwrap_or_else(|| {
            tracing::warn!(
                "No --admin-token or ICEDB_ADMIN_TOKEN set; using insecure default token 'changeme'"
            );
            "changeme".to_string()
        });

    let data_path = PathBuf::from(&data_dir);
    std::fs::create_dir_all(&data_path)?;

    tracing::info!("Initializing icedb components from data dir: {}", data_dir);

    // Initialize components (same pattern as server/src/main.rs)
    let wal_writer = Arc::new(wal::WalWriter::open(&data_path)?);
    let txn_manager = Arc::new(txn::TransactionManager::new(Arc::clone(&wal_writer)));
    let catalog = Arc::new(catalog::CatalogManager::open(
        &data_path,
        Arc::clone(&wal_writer),
        Arc::clone(&txn_manager),
    )?);
    let engine = Arc::new(sql::QueryEngine::new(
        Arc::clone(&txn_manager),
        Arc::clone(&catalog),
        data_path.clone(),
    ));

    let app_state = AppState {
        engine,
        catalog,
        wal_writer,
        admin_token,
        data_dir: data_dir.clone(),
        port,
        start_time: Instant::now(),
    };

    // CORS: allow localhost origins
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build router
    let api_router = Router::new()
        .route("/status", get(routes::status::get_status))
        // Roles
        .route("/roles", get(routes::roles::list_roles))
        .route("/roles", post(routes::roles::create_role))
        .route("/roles/:name", get(routes::roles::get_role))
        .route("/roles/:name", put(routes::roles::update_role))
        .route("/roles/:name", delete(routes::roles::delete_role))
        // Schemas
        .route("/schemas", get(routes::schemas::list_schemas))
        .route(
            "/schemas/:schema/tables",
            get(routes::schemas::list_tables_in_schema),
        )
        // Tables
        .route(
            "/tables/:schema/:table",
            get(routes::tables::get_table_schema),
        )
        .route(
            "/tables/:schema/:table",
            delete(routes::tables::drop_table),
        )
        .route(
            "/tables/:schema/:table/indexes",
            get(routes::tables::get_table_indexes),
        )
        .route(
            "/tables/:schema/:table/rowcount",
            get(routes::tables::get_table_rowcount),
        )
        .route(
            "/tables/:schema/:table/truncate",
            post(routes::tables::truncate_table),
        )
        // Query console
        .route("/query", post(routes::query::run_query))
        // Permissions (stub)
        .route("/permissions", get(routes::permissions::get_permissions));

    // Determine path to the React SPA dist directory
    // When running `cargo run -p admin-server` from the workspace root,
    // the CWD is the workspace root, so admin-ui/dist is relative to it.
    let spa_dir = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("admin-ui")
        .join("dist");

    let mut app = Router::new()
        .nest("/api", api_router)
        .layer(cors)
        .with_state(app_state);

    // Serve the React SPA if the dist directory exists
    if spa_dir.exists() {
        tracing::info!("Serving React SPA from: {}", spa_dir.display());
        app = app.fallback_service(ServeDir::new(&spa_dir).append_index_html_on_directories(true));
    } else {
        tracing::warn!(
            "admin-ui/dist not found at {}. Run `cd admin-ui && npm run build` to build the frontend.",
            spa_dir.display()
        );
    }

    let addr: SocketAddr = format!("0.0.0.0:{}", port).parse()?;
    tracing::info!("icedb Admin UI running at http://localhost:{}", port);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn parse_arg(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}
