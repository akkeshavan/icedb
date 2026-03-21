use txn::error::TxnError;

#[derive(thiserror::Error, Debug)]
pub enum DriverError {
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Query error: {0}")]
    Query(String),
    #[error("SQL error: {0}")]
    Sql(#[from] sql::SqlError),
    #[error("Transaction error: {0}")]
    Txn(#[from] TxnError),
    #[error("Pool exhausted")]
    PoolExhausted,
    #[error("No connection available")]
    NoConnection,
    #[error("Type conversion error: {0}")]
    TypeConversion(String),
}
