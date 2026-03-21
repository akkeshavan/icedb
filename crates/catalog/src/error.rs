#[derive(thiserror::Error, Debug)]
pub enum CatalogError {
    #[error("Table '{0}' not found")]
    TableNotFound(String),
    #[error("Table '{0}' already exists")]
    DuplicateTable(String),
    #[error("Column '{0}' not found")]
    ColumnNotFound(String),
    #[error("Schema '{0}' not found")]
    SchemaNotFound(String),
    #[error("Role '{0}' not found")]
    RoleNotFound(String),
    #[error("Role '{0}' already exists")]
    DuplicateRole(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Storage error: {0}")]
    Storage(#[from] storage::error::StorageError),
    #[error("Heap error: {0}")]
    Heap(#[from] storage::error::HeapError),
    #[error("WAL error: {0}")]
    Wal(#[from] wal::error::WalError),
    #[error("Transaction error: {0}")]
    Txn(#[from] txn::error::TxnError),
    #[error("Serialization error: {0}")]
    Serde(String),
    #[error("Corrupt catalog: {0}")]
    CorruptCatalog(String),
    #[error("Already exists: {0}")]
    AlreadyExists(String),
}
