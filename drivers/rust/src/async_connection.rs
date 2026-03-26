use std::sync::Arc;
use tokio::task;
use sql::engine::QueryEngine;
use sql::row::Row;
use crate::error::DriverError;

/// An async wrapper around the embedded icedb engine.
///
/// All SQL execution is delegated to a blocking thread pool via
/// `tokio::task::spawn_blocking` so the async runtime is never blocked.
pub struct AsyncConnection {
    engine: Arc<QueryEngine>,
}

impl AsyncConnection {
    pub fn new(engine: Arc<QueryEngine>) -> Self {
        Self { engine }
    }

    /// Execute a SQL query and return the result rows.
    pub async fn query(&self, sql: impl Into<String> + Send + 'static) -> Result<Vec<Row>, DriverError> {
        let engine = Arc::clone(&self.engine);
        let sql = sql.into();
        task::spawn_blocking(move || {
            engine.execute(&sql)
                .map(|r| r.rows)
                .map_err(DriverError::from)
        })
        .await
        .map_err(|e| DriverError::Connection(e.to_string()))?
    }

    /// Execute a DML statement and return the number of affected rows.
    pub async fn execute(&self, sql: impl Into<String> + Send + 'static) -> Result<u64, DriverError> {
        let engine = Arc::clone(&self.engine);
        let sql = sql.into();
        task::spawn_blocking(move || {
            engine.execute(&sql)
                .map(|r| r.rows_affected)
                .map_err(DriverError::from)
        })
        .await
        .map_err(|e| DriverError::Connection(e.to_string()))?
    }
}

/// Open an async connection to an embedded icedb database.
pub async fn open_async(data_dir: impl AsRef<std::path::Path> + Send + 'static) -> Result<AsyncConnection, DriverError> {
    let engine = task::spawn_blocking(move || crate::open(data_dir.as_ref()))
        .await
        .map_err(|e| DriverError::Connection(e.to_string()))??;
    Ok(AsyncConnection::new(engine))
}
