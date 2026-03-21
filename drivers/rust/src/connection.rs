use std::sync::Arc;
use sql::engine::QueryEngine;
use sql::executor::ExecutionResult;
use txn::transaction::IsolationLevel;
use txn::xid::Xid;
use crate::error::DriverError;

/// A connection to icedb in embedded mode.
/// Wraps the QueryEngine and manages a transaction lifecycle.
pub struct Connection {
    pub(crate) engine: Arc<QueryEngine>,
    current_xid: Option<Xid>,
    isolation: IsolationLevel,
}

impl Connection {
    pub fn new(engine: Arc<QueryEngine>) -> Self {
        Connection {
            engine,
            current_xid: None,
            isolation: IsolationLevel::ReadCommitted,
        }
    }

    /// Execute a SQL statement, auto-committing.
    pub fn execute(&self, sql: &str) -> Result<ExecutionResult, DriverError> {
        Ok(self.engine.execute(sql)?)
    }

    /// Execute a SQL statement within an explicit transaction.
    pub fn execute_in_txn(&self, sql: &str, xid: Xid) -> Result<ExecutionResult, DriverError> {
        Ok(self.engine.execute_in_txn(xid, sql)?)
    }

    /// Begin a transaction, returning the XID.
    pub fn begin(&mut self) -> Result<Xid, DriverError> {
        if self.current_xid.is_some() {
            return Err(DriverError::Connection("Transaction already active".to_string()));
        }
        let xid = self.engine.txn_manager.begin(self.isolation);
        self.current_xid = Some(xid);
        Ok(xid)
    }

    /// Commit the current transaction.
    pub fn commit(&mut self) -> Result<(), DriverError> {
        let xid = self.current_xid.take()
            .ok_or_else(|| DriverError::Connection("No active transaction".to_string()))?;
        self.engine.txn_manager.commit(xid)?;
        Ok(())
    }

    /// Rollback the current transaction.
    pub fn rollback(&mut self) -> Result<(), DriverError> {
        let xid = self.current_xid.take()
            .ok_or_else(|| DriverError::Connection("No active transaction".to_string()))?;
        self.engine.txn_manager.abort(xid)?;
        Ok(())
    }

    /// Set isolation level (must be called before begin()).
    pub fn set_isolation_level(&mut self, level: IsolationLevel) {
        self.isolation = level;
    }

    /// Returns true if there is an active (uncommitted) transaction.
    pub fn has_active_txn(&self) -> bool {
        self.current_xid.is_some()
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // Auto-rollback if transaction was not committed
        if let Some(xid) = self.current_xid.take() {
            let _ = self.engine.txn_manager.abort(xid);
        }
    }
}
