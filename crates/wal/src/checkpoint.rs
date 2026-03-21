use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::WalError;
use crate::lsn::{Lsn, INVALID_LSN};
use crate::record::WalRecordType;
use crate::writer::WalWriter;
use storage::page::{Page, PAGE_SIZE};

/// Name of the small binary control file that persists the last checkpoint LSN.
const CHECKPOINT_CTL: &str = "checkpoint.ctl";

/// Manages periodic checkpoints: flushes dirty pages as WAL `PageImage` records,
/// then writes a `Checkpoint` record and persists the checkpoint LSN to disk.
pub struct CheckpointManager {
    wal_writer: Arc<WalWriter>,
    checkpoint_lsn: Lsn,
    checkpoint_file: PathBuf,
}

impl CheckpointManager {
    /// Create (or resume) a `CheckpointManager` in `log_dir`.
    pub fn new(log_dir: &Path, wal_writer: Arc<WalWriter>) -> Result<Self, WalError> {
        fs::create_dir_all(log_dir)?;
        let checkpoint_file = log_dir.join(CHECKPOINT_CTL);
        let checkpoint_lsn = Self::read_checkpoint_file(&checkpoint_file)?;
        Ok(Self {
            wal_writer,
            checkpoint_lsn,
            checkpoint_file,
        })
    }

    /// Read the last checkpoint LSN from the control file; returns 0 if absent.
    fn read_checkpoint_file(path: &Path) -> Result<Lsn, WalError> {
        if !path.exists() {
            return Ok(INVALID_LSN);
        }
        let mut f = File::open(path)?;
        let mut buf = [0u8; 8];
        f.read_exact(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    /// Return the LSN of the last successful checkpoint (0 = none yet).
    pub fn last_checkpoint_lsn(&self) -> Lsn {
        self.checkpoint_lsn
    }

    /// Perform a checkpoint:
    ///
    /// 1. For each dirty page, write a `PageImage` WAL record.
    /// 2. Write a `Checkpoint` WAL record and fsync.
    /// 3. Persist the checkpoint LSN to `checkpoint.ctl`.
    ///
    /// Returns the LSN of the `Checkpoint` record.
    pub fn perform_checkpoint(&mut self, dirty_pages: &[(u32, &Page)]) -> Result<Lsn, WalError> {
        // Write a PageImage record for every dirty page.
        for &(page_no, page) in dirty_pages {
            let image = page.as_bytes().to_vec();
            assert_eq!(
                image.len(),
                PAGE_SIZE,
                "page image must be exactly PAGE_SIZE bytes"
            );
            self.wal_writer.append(
                0, // xid = 0 for system operations
                WalRecordType::PageImage,
                page_no,
                image,
            )?;
        }

        // Write the checkpoint record and flush.
        let ckpt_lsn = self
            .wal_writer
            .append_and_flush(0, WalRecordType::Checkpoint, 0, vec![])?;

        // Persist the checkpoint LSN.
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&self.checkpoint_file)?;
        f.write_all(&ckpt_lsn.to_le_bytes())?;
        f.sync_all()?;

        self.checkpoint_lsn = ckpt_lsn;
        log::info!("Checkpoint completed at LSN {}", ckpt_lsn);
        Ok(ckpt_lsn)
    }
}
