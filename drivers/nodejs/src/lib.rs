#![deny(clippy::all)]

use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::sync::Arc;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use sql::value::Value;

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// A row returned from a query
#[napi(object)]
pub struct JsRow {
    pub columns: Vec<String>,
    pub values: Vec<Option<String>>,  // Values as strings for simplicity
}

/// The main connection object
#[napi]
pub struct Connection {
    engine: Arc<sql::engine::QueryEngine>,
    session_id: String,
}

#[napi]
impl Connection {
    /// Execute a SQL query and return the rows
    #[napi]
    pub fn query(&self, sql: String) -> napi::Result<Vec<JsRow>> {
        let result = self.engine.execute_session(&self.session_id, &sql)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        let rows = result.rows.into_iter().map(|row| {
            let columns: Vec<String> = row.schema.iter().map(|(name, _)| name.clone()).collect();
            let values: Vec<Option<String>> = row.values.iter().map(|v| {
                match v {
                    Value::Null => None,
                    other => Some(format!("{}", other)),
                }
            }).collect();
            JsRow { columns, values }
        }).collect();

        Ok(rows)
    }

    /// Execute a DML statement and return the number of affected rows
    #[napi]
    pub fn execute(&self, sql: String) -> napi::Result<u32> {
        let result = self.engine.execute_session(&self.session_id, &sql)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(result.rows_affected as u32)
    }

    /// Begin a transaction
    #[napi]
    pub fn begin(&self) -> napi::Result<()> {
        self.engine.execute_session(&self.session_id, "BEGIN")
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(())
    }

    /// Commit the current transaction
    #[napi]
    pub fn commit(&self) -> napi::Result<()> {
        self.engine.execute_session(&self.session_id, "COMMIT")
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(())
    }

    /// Rollback the current transaction
    #[napi]
    pub fn rollback(&self) -> napi::Result<()> {
        self.engine.execute_session(&self.session_id, "ROLLBACK")
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(())
    }
}

/// Open an embedded icedb connection
#[napi]
pub fn connect(data_dir: String) -> napi::Result<Connection> {
    let data_dir = PathBuf::from(&data_dir);
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;

    let wal_writer = Arc::new(wal::WalWriter::open(&data_dir)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?);
    let txn_manager = Arc::new(txn::TransactionManager::new(Arc::clone(&wal_writer)));
    let catalog = Arc::new(catalog::CatalogManager::open(&data_dir, Arc::clone(&wal_writer), Arc::clone(&txn_manager))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?);
    let engine = Arc::new(sql::QueryEngine::new(txn_manager, catalog, data_dir));

    let session_id = format!("js-session-{}", SESSION_COUNTER.fetch_add(1, Ordering::Relaxed));
    Ok(Connection { engine, session_id })
}
