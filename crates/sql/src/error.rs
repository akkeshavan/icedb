#[derive(thiserror::Error, Debug)]
pub enum SqlError {
    /// 42601 — syntax error
    #[error("Parse error: {0}")]
    Parse(String),
    /// 42P01 — undefined table
    #[error("Table '{0}' not found")]
    TableNotFound(String),
    /// 42703 — undefined column
    #[error("Column '{0}' not found")]
    ColumnNotFound(String),
    /// 42804 — data type mismatch
    #[error("Type error: {0}")]
    TypeError(String),
    #[error("Ambiguous column name: '{0}'")]
    AmbiguousColumn(String),
    /// 23000 — constraint violation
    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),
    /// 23505 — unique violation
    #[error("Unique violation: {0}")]
    UniqueViolation(String),
    #[error("Storage error: {0}")]
    Storage(#[from] storage::error::StorageError),
    #[error("Transaction error: {0}")]
    Txn(#[from] txn::error::TxnError),
    /// 42P07 — duplicate table
    #[error("Catalog error: {0}")]
    Catalog(#[from] catalog::error::CatalogError),
    #[error("WAL error: {0}")]
    Wal(#[from] wal::error::WalError),
    /// XX000 — not yet implemented
    #[error("Not implemented: {0}")]
    NotImplemented(String),
    /// XX000 — internal / execution error
    #[error("Execution error: {0}")]
    Execution(String),
    /// 22012 — division by zero
    #[error("Division by zero")]
    DivisionByZero,
    /// 22003 — numeric overflow
    #[error("Numeric overflow: {0}")]
    NumericOverflow(String),
}

impl SqlError {
    /// Return the PostgreSQL SQLSTATE code for this error.
    pub fn sqlstate(&self) -> &'static str {
        match self {
            SqlError::Parse(_) => "42601",
            SqlError::TableNotFound(_) => "42P01",
            SqlError::ColumnNotFound(_) => "42703",
            SqlError::AmbiguousColumn(_) => "42702",
            SqlError::TypeError(_) => "42804",
            SqlError::ConstraintViolation(_) => "23000",
            SqlError::UniqueViolation(_) => "23505",
            SqlError::DivisionByZero => "22012",
            SqlError::NumericOverflow(_) => "22003",
            SqlError::Catalog(catalog::error::CatalogError::DuplicateTable(_)) => "42P07",
            SqlError::Catalog(_) => "XX000",
            SqlError::Storage(_) => "XX000",
            SqlError::Txn(txn::error::TxnError::SerializationFailure) => "40001",
            SqlError::Txn(_) => "XX000",
            SqlError::Wal(_) => "XX000",
            SqlError::NotImplemented(_) => "0A000",
            SqlError::Execution(_) => "XX000",
        }
    }
}
