use thiserror::Error;

/// Errors originating from page-level operations.
#[derive(Debug, Error)]
pub enum PageError {
    #[error("not enough free space on page (need {needed}, have {available})")]
    InsufficientSpace { needed: usize, available: u16 },

    #[error("slot index {0} is out of range")]
    SlotOutOfRange(u16),

    #[error("slot {0} has been deleted (dead item pointer)")]
    SlotDead(u16),

    #[error("page checksum mismatch")]
    ChecksumMismatch,
}

/// Errors originating from tuple encode/decode operations.
#[derive(Debug, Error)]
pub enum TupleError {
    #[error("byte slice too short to decode tuple header (got {got}, need {need})")]
    TooShort { got: usize, need: usize },
}

/// Errors originating from heap file operations.
#[derive(Debug, Error)]
pub enum HeapError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("page error: {0}")]
    Page(#[from] PageError),

    #[error("tuple error: {0}")]
    Tuple(#[from] TupleError),

    #[error("page {0} does not exist in heap file")]
    PageNotFound(u32),
}

/// Errors originating from buffer pool operations.
#[derive(Debug, Error)]
pub enum BufferError {
    #[error("heap file error: {0}")]
    Heap(#[from] HeapError),

    #[error("all buffer frames are pinned; cannot evict")]
    AllFramesPinned,

    #[error("frame index {0} is out of range")]
    FrameOutOfRange(usize),
}

/// Top-level unified storage error.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("page error: {0}")]
    Page(#[from] PageError),

    #[error("tuple error: {0}")]
    Tuple(#[from] TupleError),

    #[error("heap error: {0}")]
    Heap(#[from] HeapError),

    #[error("buffer error: {0}")]
    Buffer(#[from] BufferError),
}
