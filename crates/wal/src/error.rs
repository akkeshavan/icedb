use crate::lsn::Lsn;

#[derive(thiserror::Error, Debug)]
pub enum WalError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Corrupt WAL record at LSN {lsn}: {reason}")]
    CorruptRecord { lsn: Lsn, reason: String },

    #[error("Storage error: {0}")]
    Storage(#[from] storage::error::StorageError),

    #[error("Invalid record type byte: {0}")]
    InvalidRecordType(u8),
}
