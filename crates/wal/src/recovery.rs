use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::WalError;
use crate::lsn::{Lsn, INVALID_LSN};
use crate::reader::WalReader;
use crate::record::WalRecordType;
use storage::heap::HeapFile;
use storage::page::{Page, PAGE_SIZE};

const CHECKPOINT_CTL: &str = "checkpoint.ctl";

/// Performs REDO recovery by replaying WAL records from the last checkpoint.
pub struct RecoveryManager {
    log_dir: PathBuf,
}

impl RecoveryManager {
    pub fn new(log_dir: &Path) -> Self {
        Self {
            log_dir: log_dir.to_path_buf(),
        }
    }

    /// Replay the WAL into `heap`, starting from the last checkpoint.
    ///
    /// Returns the LSN of the last replayed record (or `INVALID_LSN` if no
    /// records were replayed).
    pub fn recover(&self, heap: &mut HeapFile) -> Result<Lsn, WalError> {
        let start_lsn = self.read_checkpoint_lsn()?;
        log::info!("Starting WAL recovery from LSN {}", start_lsn);

        let mut reader = WalReader::open(&self.log_dir, start_lsn)?;
        let mut last_lsn = INVALID_LSN;

        while let Some(record) = reader.next_record()? {
            last_lsn = record.lsn;

            match record.record_type {
                WalRecordType::PageImage => {
                    // Overwrite the heap page with the full image from the WAL.
                    if record.data.len() != PAGE_SIZE {
                        return Err(WalError::CorruptRecord {
                            lsn: record.lsn,
                            reason: format!(
                                "PageImage data length {} != PAGE_SIZE {}",
                                record.data.len(),
                                PAGE_SIZE
                            ),
                        });
                    }
                    let mut buf = [0u8; PAGE_SIZE];
                    buf.copy_from_slice(&record.data);
                    let page = Page::from_bytes(buf);

                    // Ensure the heap file is large enough.
                    self.ensure_pages(heap, record.page_no + 1)?;
                    heap.write_page(record.page_no, &page)
                        .map_err(|e| WalError::Storage(storage::error::StorageError::Heap(e)))?;

                    log::debug!("Recovered PageImage for page {}", record.page_no);
                }

                // Insert, Delete, Update logical records are superseded by PageImage records.
                // Recovery uses only PageImage for correctness and idempotency.
                WalRecordType::Insert => {
                    log::debug!(
                        "Skipping Insert record at LSN {} (superseded by PageImage)",
                        record.lsn
                    );
                }

                WalRecordType::Delete => {
                    log::debug!(
                        "Skipping Delete record at LSN {} (superseded by PageImage)",
                        record.lsn
                    );
                }

                // Commit, Abort, Checkpoint, Update — no physical redo action needed.
                WalRecordType::Commit
                | WalRecordType::Abort
                | WalRecordType::Checkpoint
                | WalRecordType::Update => {
                    log::debug!(
                        "Skipping {:?} record at LSN {}",
                        record.record_type,
                        record.lsn
                    );
                }
            }
        }

        log::info!("WAL recovery finished; last replayed LSN = {}", last_lsn);
        Ok(last_lsn)
    }

    fn read_checkpoint_lsn(&self) -> Result<Lsn, WalError> {
        let path = self.log_dir.join(CHECKPOINT_CTL);
        if !path.exists() {
            return Ok(INVALID_LSN);
        }
        let mut f = File::open(&path)?;
        let mut buf = [0u8; 8];
        f.read_exact(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    /// Extend the heap with empty pages until `heap.num_pages() >= required`.
    fn ensure_pages(&self, heap: &mut HeapFile, required: u32) -> Result<(), WalError> {
        while heap.num_pages() < required {
            heap.allocate_page()
                .map_err(|e| WalError::Storage(storage::error::StorageError::Heap(e)))?;
        }
        Ok(())
    }
}
