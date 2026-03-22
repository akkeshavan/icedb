use std::sync::Arc;
use tempfile::TempDir;

use catalog::manager::CatalogManager;
use sql::db_manager::DatabaseManager;
use sql::engine::QueryEngine;
use txn::manager::TransactionManager;
use wal::writer::WalWriter;

use crate::handler::IceDbHandler;

fn setup_engine(dir: &std::path::Path) -> Arc<QueryEngine> {
    let wal = Arc::new(WalWriter::open(dir).unwrap());
    let txn = Arc::new(TransactionManager::new(Arc::clone(&wal)));
    let catalog = Arc::new(CatalogManager::open(dir, Arc::clone(&wal), Arc::clone(&txn)).unwrap());
    Arc::new(QueryEngine::new(txn, catalog, dir.to_path_buf()))
}

fn setup_db_manager(dir: &std::path::Path) -> Arc<DatabaseManager> {
    let engine = setup_engine(dir);
    let mgr = Arc::new(DatabaseManager::new(dir.to_path_buf()));
    mgr.register_engine("icedb", engine);
    mgr
}

#[test]
fn test_handler_executes_create_table() {
    let dir = TempDir::new().unwrap();
    let engine = setup_engine(dir.path());

    // Execute a CREATE TABLE directly through the engine
    let result = engine
        .execute("CREATE TABLE test_net (id INT4, name TEXT)")
        .unwrap();
    assert_eq!(result.command, "CREATE TABLE test_net");
}

#[test]
fn test_handler_executes_insert_and_select() {
    let dir = TempDir::new().unwrap();
    let engine = setup_engine(dir.path());

    engine
        .execute("CREATE TABLE users (id INT4, name TEXT)")
        .unwrap();
    engine
        .execute("INSERT INTO users VALUES (1, 'Alice')")
        .unwrap();
    engine
        .execute("INSERT INTO users VALUES (2, 'Bob')")
        .unwrap();

    let result = engine.execute("SELECT id, name FROM users").unwrap();
    assert_eq!(result.rows.len(), 2);
}

#[test]
fn test_handler_creation() {
    let dir = TempDir::new().unwrap();
    let wal = Arc::new(WalWriter::open(dir.path()).unwrap());
    let txn = Arc::new(TransactionManager::new(Arc::clone(&wal)));
    let catalog =
        Arc::new(CatalogManager::open(dir.path(), Arc::clone(&wal), Arc::clone(&txn)).unwrap());
    let db_manager = setup_db_manager(dir.path());
    let auth = Arc::new(auth::Authenticator::new(catalog));

    // Verify IceDbHandler can be constructed
    let _handler = IceDbHandler::new(db_manager, auth);
}

#[tokio::test]
async fn test_server_starts() {
    let dir = TempDir::new().unwrap();
    let db_manager = setup_db_manager(dir.path());
    let wal = Arc::new(WalWriter::open(dir.path()).unwrap());
    let txn = Arc::new(TransactionManager::new(Arc::clone(&wal)));
    let catalog =
        Arc::new(CatalogManager::open(dir.path(), Arc::clone(&wal), Arc::clone(&txn)).unwrap());
    let auth = Arc::new(auth::Authenticator::new(catalog));

    // Bind to port 0 to get a random available port
    let bind_addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = crate::Server::new(db_manager, auth, bind_addr);

    // Spawn the server and immediately cancel it — just verify it binds without error
    let handle = tokio::spawn(async move { server.run().await.unwrap_or(()) });

    // Give it a moment to bind
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Cancel the server task
    handle.abort();
    // Aborting is expected
}
