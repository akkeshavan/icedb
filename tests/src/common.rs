use std::sync::Arc;
use std::path::Path;
use sql::engine::QueryEngine;
use txn::manager::TransactionManager;
use catalog::manager::CatalogManager;
use wal::writer::WalWriter;

pub fn make_engine(dir: &Path) -> Arc<QueryEngine> {
    let wal = Arc::new(WalWriter::open(dir).unwrap());
    let txn = Arc::new(TransactionManager::new_with_wal_recovery(Arc::clone(&wal), dir));
    let cat = Arc::new(CatalogManager::open(dir, Arc::clone(&wal), Arc::clone(&txn)).unwrap());
    Arc::new(QueryEngine::new(txn, cat, dir.to_path_buf()))
}

/// Execute SQL and panic on error (for test setup)
pub fn exec(engine: &Arc<QueryEngine>, sql: &str) -> sql::ExecutionResult {
    engine.execute(sql).unwrap_or_else(|e| panic!("SQL failed: {}\nSQL: {}", e, sql))
}

/// Execute SQL and expect an error
pub fn exec_err(engine: &Arc<QueryEngine>, sql: &str) -> sql::SqlError {
    engine.execute(sql).expect_err("Expected SQL error but got success")
}

/// Get a single integer value from the first column of the first row
pub fn query_int(engine: &Arc<QueryEngine>, sql: &str) -> i64 {
    let result = exec(engine, sql);
    match result.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(sql::Value::Int4(v)) => *v as i64,
        Some(sql::Value::Int8(v)) => *v,
        other => panic!("Expected integer, got {:?} for SQL: {}", other, sql),
    }
}

/// Count rows matching a query
pub fn count_rows(engine: &Arc<QueryEngine>, sql: &str) -> usize {
    exec(engine, sql).rows.len()
}

/// Execute SQL within a named session (supports BEGIN/COMMIT/ROLLBACK/SAVEPOINT state)
pub fn exec_session(engine: &Arc<QueryEngine>, session_id: &str, sql: &str) -> sql::ExecutionResult {
    engine.execute_session(session_id, sql)
        .unwrap_or_else(|e| panic!("SQL failed in session '{}': {}\nSQL: {}", session_id, e, sql))
}

/// Execute SQL in a session, expecting an error
pub fn exec_session_err(engine: &Arc<QueryEngine>, session_id: &str, sql: &str) -> sql::SqlError {
    engine.execute_session(session_id, sql)
        .expect_err("Expected SQL error but got success")
}
