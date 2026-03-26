pub mod error;
pub mod connection;
pub mod pool;
pub mod async_connection;
pub mod arrow_output;

#[cfg(test)]
mod tests;

pub use error::DriverError;
pub use connection::Connection;
pub use pool::{ConnectionPool, PooledConnection};
pub use async_connection::{AsyncConnection, open_async};
pub use arrow_output::rows_to_record_batch;

use std::sync::Arc;
use std::path::Path;
use sql::engine::QueryEngine;

/// Open an embedded icedb database from a data directory.
pub fn open(data_dir: &Path) -> Result<Arc<QueryEngine>, DriverError> {
    std::fs::create_dir_all(data_dir)
        .map_err(|e| DriverError::Connection(e.to_string()))?;
    let wal_writer = Arc::new(wal::WalWriter::open(data_dir)
        .map_err(|e| DriverError::Connection(e.to_string()))?);
    let txn_manager = Arc::new(txn::TransactionManager::new(Arc::clone(&wal_writer)));
    let catalog = Arc::new(catalog::CatalogManager::open(data_dir, Arc::clone(&wal_writer), Arc::clone(&txn_manager))
        .map_err(|e| DriverError::Connection(e.to_string()))?);
    Ok(Arc::new(QueryEngine::new(txn_manager, catalog, data_dir.to_path_buf())))
}

/// Open a connection pool to an embedded icedb database.
pub fn open_pool(data_dir: &Path, max_connections: usize) -> Result<ConnectionPool, DriverError> {
    let engine = open(data_dir)?;
    Ok(ConnectionPool::new(engine, max_connections))
}
