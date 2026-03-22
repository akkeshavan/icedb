use cli::{Config, Repl};
use env_logger::Env;
use std::path::PathBuf;
use std::sync::Arc;

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("warn")).init();

    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    let config = Config::from_args(&args);
    let data_dir = PathBuf::from(&config.data_dir);

    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        eprintln!("Error creating data directory: {}", e);
        std::process::exit(1);
    }

    // Initialize the engine stack
    let wal_writer = match wal::WalWriter::open(&data_dir) {
        Ok(w) => Arc::new(w),
        Err(e) => {
            eprintln!("Failed to open WAL: {}", e);
            std::process::exit(1);
        }
    };
    let txn_manager = Arc::new(txn::TransactionManager::new_with_wal_recovery(Arc::clone(&wal_writer), &data_dir));
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
    let engine = Arc::new(sql::QueryEngine::new(
        Arc::clone(&txn_manager),
        Arc::clone(&catalog),
        data_dir.clone(),
    ));

    let db_manager = Arc::new(sql::DatabaseManager::new(data_dir));
    db_manager.register_engine("icedb", Arc::clone(&engine));

    let mut repl = Repl::new(db_manager, engine, catalog, config);
    if let Err(e) = repl.run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn print_usage() {
    println!("nkv-psql - icedb interactive terminal");
    println!();
    println!("Usage:");
    println!("  nkv-psql [OPTIONS]");
    println!();
    println!("Options:");
    println!("  --data-dir DIR    Data directory (default: ./data)");
    println!("  --user, -U USER   Username (default: postgres, or $PGUSER)");
    println!("  --dbname, -d DB   Database name (default: username, or $PGDATABASE)");
    println!("  --help, -h        Show this help");
}
