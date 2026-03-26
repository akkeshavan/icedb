#[derive(thiserror::Error, Debug)]
pub enum TxnError {
    #[error("Transaction {0} not found")]
    NotFound(u32),
    #[error("Transaction {0} is not active")]
    NotActive(u32),
    #[error("Write-write conflict: tuple locked by transaction {holder}")]
    WriteWriteConflict { holder: u32 },
    #[error("deadlock detected between xid {xid} and xid {holder}")]
    Deadlock { xid: u32, holder: u32 },
    #[error("Serialization failure")]
    SerializationFailure,
    #[error("Storage error: {0}")]
    Storage(#[from] storage::error::StorageError),
    #[error("WAL error: {0}")]
    Wal(#[from] wal::error::WalError),
}
