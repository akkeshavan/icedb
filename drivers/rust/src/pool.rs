use std::sync::Arc;
use parking_lot::Mutex;
use sql::engine::QueryEngine;
use sql::executor::ExecutionResult;
use crate::connection::Connection;
use crate::error::DriverError;

/// A simple connection pool for icedb embedded connections.
pub struct ConnectionPool {
    engine: Arc<QueryEngine>,
    max_size: usize,
    /// Pool of idle connections
    idle: Mutex<Vec<Connection>>,
    /// Current count of connections ever created (idle + in-use)
    active_count: Mutex<usize>,
}

impl ConnectionPool {
    pub fn new(engine: Arc<QueryEngine>, max_size: usize) -> Self {
        ConnectionPool {
            engine,
            max_size,
            idle: Mutex::new(Vec::new()),
            active_count: Mutex::new(0),
        }
    }

    /// Acquire a connection from the pool.
    /// If idle connections are available, returns one.
    /// If under max_size, creates a new one.
    /// Otherwise returns PoolExhausted.
    pub fn acquire(&self) -> Result<PooledConnection<'_>, DriverError> {
        // Try to get an idle connection
        {
            let mut idle = self.idle.lock();
            if let Some(conn) = idle.pop() {
                return Ok(PooledConnection { conn: Some(conn), pool: self });
            }
        }

        // Create new connection if under limit
        {
            let mut count = self.active_count.lock();
            if *count >= self.max_size {
                return Err(DriverError::PoolExhausted);
            }
            *count += 1;
        }

        Ok(PooledConnection {
            conn: Some(Connection::new(Arc::clone(&self.engine))),
            pool: self,
        })
    }

    fn return_connection(&self, conn: Connection) {
        let mut idle = self.idle.lock();
        idle.push(conn);
    }
}

/// RAII guard that returns the connection to the pool on drop.
pub struct PooledConnection<'a> {
    conn: Option<Connection>,
    pool: &'a ConnectionPool,
}

impl<'a> PooledConnection<'a> {
    pub fn execute(&self, sql: &str) -> Result<ExecutionResult, DriverError> {
        self.conn.as_ref().unwrap().execute(sql)
    }

    pub fn connection(&mut self) -> &mut Connection {
        self.conn.as_mut().unwrap()
    }
}

impl<'a> Drop for PooledConnection<'a> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.return_connection(conn);
        }
    }
}
