use cli::{Config, Repl};
use env_logger::Env;
use std::path::PathBuf;
use std::sync::Arc;

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("warn")).init();

    let args: Vec<String> = std::env::args().collect();

    // -h is the short form of --help only (not --host, which avoids the
    // collision).  --host sets network-client mode.
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    let config = Config::from_args(&args);

    if config.is_network_mode() {
        run_network(config);
    } else {
        run_embedded(config);
    }
}

// ── Embedded mode (no TCP, direct engine access) ──────────────────────────────

fn run_embedded(config: Config) {
    let data_dir = PathBuf::from(&config.data_dir);

    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        eprintln!("Error creating data directory: {}", e);
        std::process::exit(1);
    }

    let wal_writer = match wal::WalWriter::open(&data_dir) {
        Ok(w) => Arc::new(w),
        Err(e) => {
            eprintln!("Failed to open WAL: {}", e);
            std::process::exit(1);
        }
    };
    let txn_manager = Arc::new(txn::TransactionManager::new_with_wal_recovery(
        Arc::clone(&wal_writer),
        &data_dir,
    ));
    let catalog = match catalog::CatalogManager::open(
        &data_dir,
        Arc::clone(&wal_writer),
        Arc::clone(&txn_manager),
    ) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("Failed to open catalog: {}", e);
            std::process::exit(1);
        }
    };
    let engine = Arc::new(
        sql::QueryEngine::new(
            Arc::clone(&txn_manager),
            Arc::clone(&catalog),
            data_dir.clone(),
        )
        .with_role(config.username.clone()),
    );

    let db_manager = Arc::new(sql::DatabaseManager::new(data_dir));
    db_manager.register_engine("icedb", Arc::clone(&engine));

    let mut repl = Repl::new_embedded(db_manager, engine, catalog, config);
    if let Err(e) = repl.run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

// ── Network mode (TCP + optional TLS, PostgreSQL wire protocol) ───────────────

fn run_network(config: Config) {
    match cli::pg_client::PgClient::connect(&config) {
        Ok(client) => {
            let mut repl = Repl::new_network(client, config);
            if let Err(e) = repl.run() {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Connection failed: {}", e);
            std::process::exit(1);
        }
    }
}

// ── Usage ─────────────────────────────────────────────────────────────────────

fn print_usage() {
    println!("isql - icedb interactive terminal");
    println!();
    println!("USAGE");
    println!("  isql [OPTIONS]");
    println!();
    println!("EMBEDDED MODE (default — opens data directory directly)");
    println!("  --data-dir DIR        Data directory (default: ./data)");
    println!();
    println!("NETWORK MODE (connects to a running icedb-server over TCP)");
    println!("  --host HOST           Server hostname or IP  [enables network mode]");
    println!("  --port PORT           Server port (default: 5432, or $PGPORT)");
    println!("  --sslmode MODE        TLS mode: disable | require | verify-full (default: disable)");
    println!("  --sslrootcert PATH    CA certificate file (required for verify-full)");
    println!();
    println!("COMMON OPTIONS");
    println!("  --user, -U USER       Username (default: icedb, or $PGUSER)");
    println!("  --dbname, -d DB       Database name (default: username, or $PGDATABASE)");
    println!("  --password, -W PWD    Password (or set $PGPASSWORD / use ~/.pgpass)");
    println!("  --help, -h            Show this help");
    println!();
    println!("EXAMPLES");
    println!("  # Embedded (local dev — no server needed)");
    println!("  isql --data-dir ./data");
    println!();
    println!("  # Network — plaintext (server running without TLS)");
    println!("  isql --host localhost --port 5432 --user icedb");
    println!();
    println!("  # Network — TLS with self-signed cert (dev/test)");
    println!("  isql --host localhost --sslmode require --user icedb");
    println!();
    println!("  # Network — TLS with certificate verification (production)");
    println!("  isql --host db.example.com --sslmode verify-full \\");
    println!("       --sslrootcert /etc/ssl/certs/ca.crt --user icedb");
    println!();
    println!("ENV VARS");
    println!("  PGHOST, PGPORT, PGUSER, PGDATABASE, PGPASSWORD");
}
