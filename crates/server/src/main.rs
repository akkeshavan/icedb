use env_logger::Env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // Parse args: --port PORT --data-dir DIR [--shared-buffers N] [--tls-cert PATH --tls-key PATH]
    let args: Vec<String> = std::env::args().collect();
    let port = parse_arg(&args, "--port").unwrap_or("5432".to_string());
    let data_dir = parse_arg(&args, "--data-dir").unwrap_or("./data".to_string());
    let data_dir = PathBuf::from(&data_dir);
    let tls_cert = parse_arg(&args, "--tls-cert");
    let tls_key = parse_arg(&args, "--tls-key");
    let shared_buffers: usize = parse_arg(&args, "--shared-buffers")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1024);

    std::fs::create_dir_all(&data_dir)?;

    let addr: SocketAddr = format!("0.0.0.0:{}", port).parse()?;

    log::info!("Starting icedb server at {}", addr);
    log::info!("Data directory: {}", data_dir.display());
    log::info!("shared_buffers = {} frames", shared_buffers);

    // Initialize the default "icedb" database engine
    let wal_writer = Arc::new(wal::WalWriter::open(&data_dir)?);
    let txn_manager = Arc::new(txn::TransactionManager::new_with_wal_recovery(Arc::clone(&wal_writer), &data_dir));
    let catalog = Arc::new(catalog::CatalogManager::open(
        &data_dir,
        Arc::clone(&wal_writer),
        Arc::clone(&txn_manager),
    )?);
    let engine = Arc::new(sql::QueryEngine::new(
        Arc::clone(&txn_manager),
        Arc::clone(&catalog),
        data_dir.clone(),
    ));

    // DatabaseManager: all databases share one manager; default engine is pre-registered
    let db_manager = Arc::new(sql::DatabaseManager::new(data_dir.clone()));
    db_manager.register_engine("icedb", Arc::clone(&engine));

    let authenticator = Arc::new(auth::Authenticator::new(Arc::clone(&catalog)));

    let server = network::Server::new(db_manager, authenticator, addr);
    let server = match (tls_cert, tls_key) {
        (Some(cert), Some(key)) => {
            let acceptor = network::build_tls_acceptor(&cert, &key)?;
            log::info!("TLS enabled (cert: {})", cert);
            server.with_tls(acceptor)
        }
        _ => {
            log::info!("TLS disabled (no --tls-cert/--tls-key)");
            server
        }
    };
    server.run().await?;

    Ok(())
}

fn parse_arg(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}
