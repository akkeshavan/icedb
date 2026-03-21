#[derive(thiserror::Error, Debug)]
pub enum BTreeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Storage error: {0}")]
    Storage(#[from] storage::error::StorageError),
    #[error("WAL error: {0}")]
    Wal(#[from] wal::error::WalError),
    #[error("Page {0} is not a valid B-tree page")]
    InvalidPage(u32),
    #[error("Key not found")]
    KeyNotFound,
    #[error("Duplicate key")]
    DuplicateKey,
    #[error("Tree is empty")]
    EmptyTree,
    #[error("Corrupt metadata")]
    CorruptMeta,
}
