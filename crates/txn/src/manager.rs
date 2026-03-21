use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use crate::error::TxnError;
use crate::lock::LockManager;
use crate::snapshot::Snapshot;
use crate::transaction::{IsolationLevel, Transaction, TransactionState};
use crate::visibility::is_tuple_visible;
use crate::xid::{allocate_xid, Xid, INVALID_XID};
use storage::heap::HeapFile;
use storage::page::Page;
use storage::tid::TID;
use storage::tuple::{Tuple, TupleHeader, TUPLE_HEADER_SIZE};
use wal::reader::WalReader;
use wal::record::WalRecordType;
use wal::writer::WalWriter;

pub struct TransactionManager {
    inner: Mutex<TxnManagerInner>,
    wal_writer: Arc<WalWriter>,
    lock_manager: LockManager,
}

struct TxnManagerInner {
    /// Currently active transactions
    active: HashMap<Xid, Transaction>,
    /// Set of committed XIDs (for visibility checks)
    committed: HashSet<Xid>,
    /// Set of aborted XIDs
    aborted: HashSet<Xid>,
    /// Next XID counter (tracks what the next allocation will be)
    next_xid: Xid,
}

impl TransactionManager {
    pub fn new(wal_writer: Arc<WalWriter>) -> Self {
        TransactionManager {
            inner: Mutex::new(TxnManagerInner {
                active: HashMap::new(),
                committed: HashSet::new(),
                aborted: HashSet::new(),
                next_xid: 3, // matches NEXT_XID atomic start
            }),
            wal_writer,
            lock_manager: LockManager::new(),
        }
    }

    /// Open an existing or new WAL at `log_dir` and create a TransactionManager.
    pub fn open(log_dir: &Path) -> Result<Self, TxnError> {
        let wal = WalWriter::open(log_dir)?;
        Ok(Self::new(Arc::new(wal)))
    }

    /// Create a TransactionManager and restore committed/aborted XID state from WAL.
    ///
    /// This scans WAL Commit and Abort records to repopulate the in-memory
    /// committed and aborted sets after a restart.
    pub fn new_with_wal_recovery(wal_writer: Arc<WalWriter>, log_dir: &Path) -> Self {
        let mgr = Self::new(Arc::clone(&wal_writer));
        // Best-effort: scan WAL records to find committed/aborted XIDs
        if let Ok(mut reader) = WalReader::open(log_dir, wal::lsn::INVALID_LSN) {
            let mut max_xid: Xid = 2; // start after FROZEN_XID
            loop {
                match reader.next_record() {
                    Ok(Some(record)) => {
                        let xid = record.xid;
                        if xid > max_xid {
                            max_xid = xid;
                        }
                        match record.record_type {
                            WalRecordType::Commit => {
                                let mut inner = mgr.inner.lock();
                                inner.committed.insert(xid);
                                inner.aborted.remove(&xid);
                            }
                            WalRecordType::Abort => {
                                let mut inner = mgr.inner.lock();
                                inner.aborted.insert(xid);
                            }
                            _ => {}
                        }
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            // Update next_xid so new transactions get fresh XIDs
            {
                let mut inner = mgr.inner.lock();
                if max_xid + 1 > inner.next_xid {
                    inner.next_xid = max_xid + 1;
                }
            }
        }
        mgr
    }

    /// Begin a new transaction with the given isolation level.
    /// Returns the XID of the new transaction.
    pub fn begin(&self, isolation: IsolationLevel) -> Xid {
        let xid = allocate_xid();
        let mut inner = self.inner.lock();
        inner.next_xid = xid + 1;
        let mut txn = Transaction::new(xid, isolation);

        // For RepeatableRead and Serializable, take a snapshot at BEGIN time
        if isolation == IsolationLevel::RepeatableRead || isolation == IsolationLevel::Serializable
        {
            let snap = Self::take_snapshot_inner(&inner, xid);
            txn.snapshot = Some(snap);
        }

        inner.active.insert(xid, txn);
        xid
    }

    /// Commit a transaction: write WAL COMMIT record, move to committed set.
    pub fn commit(&self, xid: Xid) -> Result<(), TxnError> {
        {
            let inner = self.inner.lock();
            let txn = inner.active.get(&xid).ok_or(TxnError::NotFound(xid))?;
            if txn.state != TransactionState::Active {
                return Err(TxnError::NotActive(xid));
            }
        }

        // For Serializable transactions, check for rw-antidependency conflicts.
        self.check_serializable_conflict(xid)?;

        // Write WAL COMMIT record (durable flush)
        self.wal_writer
            .append_and_flush(xid, WalRecordType::Commit, 0, vec![])?;

        // Move from active to committed
        {
            let mut inner = self.inner.lock();
            inner.active.remove(&xid);
            inner.committed.insert(xid);
        }

        // Release all locks
        self.lock_manager.release_all(xid);

        log::debug!("txn {xid}: committed");
        Ok(())
    }

    /// Check for serializable conflicts using rw-antidependency cycle detection.
    ///
    /// Implements simplified Thomas-Ports SSI:
    /// For committing txn T, find concurrent txns T2 that read data T wrote,
    /// then check if T2 also wrote data T read — forming a cycle T→T2→T.
    fn check_serializable_conflict(&self, xid: Xid) -> Result<(), TxnError> {
        let inner = self.inner.lock();
        let txn = match inner.active.get(&xid) {
            Some(t) => t,
            None => return Ok(()),
        };

        if txn.isolation != IsolationLevel::Serializable {
            return Ok(());
        }

        log::debug!(
            "SSI commit check for xid={}: read_set={} entries, write_set={} entries",
            xid,
            txn.read_set.len(),
            txn.write_set.len()
        );

        if txn.write_set.is_empty() || txn.read_set.is_empty() {
            return Ok(());
        }

        let t_write_set: HashSet<(u32, u16)> = txn.write_set.clone();
        let t_read_set: HashSet<(u32, u16)> = txn.read_set.clone();

        // For each concurrent Serializable transaction T2 (not yet committed, not aborted)
        for (other_xid, other_txn) in &inner.active {
            if *other_xid == xid {
                continue;
            }
            if other_txn.isolation != IsolationLevel::Serializable {
                continue;
            }

            // Check: T2 read something that T wrote (T2 →rw→ T antidependency)
            let t2_read_t_write = other_txn.read_set.intersection(&t_write_set).next().is_some();

            if t2_read_t_write {
                // Check: T wrote something T2 also wrote (T →rw→ T2 antidependency)
                // (T read data that T2 is writing)
                let t_read_t2_write = t_read_set.intersection(&other_txn.write_set).next().is_some();

                if t_read_t2_write {
                    log::warn!(
                        "SSI: cycle detected — xid={} and xid={} form a dangerous structure",
                        xid, other_xid
                    );
                    return Err(TxnError::SerializationFailure);
                }
            }
        }

        Ok(())
    }

    /// Abort a transaction: write WAL ABORT record, move to aborted set.
    pub fn abort(&self, xid: Xid) -> Result<(), TxnError> {
        {
            let inner = self.inner.lock();
            let txn = inner.active.get(&xid).ok_or(TxnError::NotFound(xid))?;
            if txn.state != TransactionState::Active {
                return Err(TxnError::NotActive(xid));
            }
        }

        // Write WAL ABORT record
        self.wal_writer
            .append_and_flush(xid, WalRecordType::Abort, 0, vec![])?;

        // Move from active to aborted
        {
            let mut inner = self.inner.lock();
            inner.active.remove(&xid);
            inner.aborted.insert(xid);
        }

        // Release all locks
        self.lock_manager.release_all(xid);

        log::debug!("txn {xid}: aborted");
        Ok(())
    }

    /// Get the current snapshot for a transaction.
    /// For ReadCommitted: take a fresh snapshot each time.
    /// For RepeatableRead/Serializable: return the stored snapshot from BEGIN.
    pub fn current_snapshot(&self, xid: Xid) -> Snapshot {
        let inner = self.inner.lock();
        let txn = inner.active.get(&xid);
        match txn {
            Some(t)
                if t.isolation == IsolationLevel::RepeatableRead
                    || t.isolation == IsolationLevel::Serializable =>
            {
                t.snapshot
                    .clone()
                    .unwrap_or_else(|| Self::take_snapshot_inner(&inner, xid))
            }
            _ => Self::take_snapshot_inner(&inner, xid),
        }
    }

    /// Compute a snapshot from the current active set.
    fn take_snapshot_inner(inner: &TxnManagerInner, current_xid: Xid) -> Snapshot {
        let in_progress: HashSet<Xid> = inner.active.keys().copied().collect();
        let xmin = in_progress
            .iter()
            .copied()
            .filter(|&x| x != current_xid)
            .min()
            .unwrap_or(inner.next_xid);
        let xmax = inner.next_xid;
        Snapshot::new(xmin, xmax, in_progress)
    }

    /// Insert a tuple into the heap with MVCC metadata, log to WAL.
    pub fn insert_tuple(
        &self,
        xid: Xid,
        heap: &mut HeapFile,
        data: &[u8],
    ) -> Result<TID, TxnError> {
        let mut header = TupleHeader::new();
        header.t_xmin = xid;
        header.t_xmax = INVALID_XID;

        let tuple = Tuple {
            header,
            data: data.to_vec(),
        };

        let encoded = tuple.encode();

        // 1. Prepare the modification in memory (find page, insert into page buffer)
        let (page_no, modified_page, slot) = heap
            .find_and_prepare_insert(&encoded)
            .map_err(|e| TxnError::Storage(storage::error::StorageError::Heap(e)))?;

        let tid = TID::new(page_no, slot);

        // 2. Write WAL PageImage record FIRST (before page hits disk)
        let page_bytes = modified_page.as_bytes().to_vec();
        self.wal_writer
            .append(xid, WalRecordType::PageImage, page_no, page_bytes)?;

        // 3. THEN write the page to disk
        heap.write_page(page_no, &modified_page)
            .map_err(|e| TxnError::Storage(storage::error::StorageError::Heap(e)))?;

        // SSI: track written TIDs for Serializable transactions.
        {
            let mut inner = self.inner.lock();
            if let Some(txn) = inner.active.get_mut(&xid) {
                if txn.isolation == IsolationLevel::Serializable {
                    txn.write_set.insert((tid.page_no, tid.slot));
                }
            }
        }

        log::debug!("txn {xid}: inserted tuple at {tid}");
        Ok(tid)
    }

    /// Delete a tuple by setting t_xmax = xid in the stored header.
    /// This is an MVCC "soft delete" — the slot is NOT physically marked dead.
    pub fn delete_tuple(&self, xid: Xid, heap: &mut HeapFile, tid: TID) -> Result<(), TxnError> {
        // Acquire write lock
        self.lock_manager.acquire_write_lock(xid, tid)?;

        // Read the current tuple
        let mut tuple = heap
            .get_tuple(tid)
            .map_err(|e| TxnError::Storage(storage::error::StorageError::Heap(e)))?;

        // Set xmax
        tuple.header.t_xmax = xid;

        // 1. Read the page and modify it in memory
        let mut page = heap
            .read_page(tid.page_no)
            .map_err(|e| TxnError::Storage(storage::error::StorageError::Heap(e)))?;
        write_tuple_to_page(&mut page, tid.slot, &tuple)
            .map_err(|e| TxnError::Storage(storage::error::StorageError::Page(e)))?;

        // 2. Write WAL PageImage record FIRST (before page hits disk)
        self.wal_writer.append(
            xid,
            WalRecordType::PageImage,
            tid.page_no,
            page.as_bytes().to_vec(),
        )?;

        // 3. Write the page to disk
        heap.write_page(tid.page_no, &page)
            .map_err(|e| TxnError::Storage(storage::error::StorageError::Heap(e)))?;

        // SSI: track written TIDs for Serializable transactions.
        {
            let mut inner = self.inner.lock();
            if let Some(txn) = inner.active.get_mut(&xid) {
                if txn.isolation == IsolationLevel::Serializable {
                    txn.write_set.insert((tid.page_no, tid.slot));
                }
            }
        }

        log::debug!("txn {xid}: soft-deleted tuple at {tid}");
        Ok(())
    }

    /// Update a tuple: soft-delete old, insert new. Returns new TID.
    ///
    /// WAL is handled by the individual delete_tuple and insert_tuple calls
    /// (each writes a PageImage record before the page hits disk).
    pub fn update_tuple(
        &self,
        xid: Xid,
        heap: &mut HeapFile,
        tid: TID,
        new_data: &[u8],
    ) -> Result<TID, TxnError> {
        self.delete_tuple(xid, heap, tid)?;
        let new_tid = self.insert_tuple(xid, heap, new_data)?;

        log::debug!("txn {xid}: updated tuple {tid} → {new_tid}");
        Ok(new_tid)
    }

    /// Scan all tuples in the heap that are visible to the given transaction's snapshot.
    pub fn scan_visible_tuples(
        &self,
        xid: Xid,
        heap: &mut HeapFile,
    ) -> Result<Vec<(TID, Tuple)>, TxnError> {
        let snapshot = self.current_snapshot(xid);
        let (committed, is_serializable) = {
            let inner = self.inner.lock();
            let is_ser = inner
                .active
                .get(&xid)
                .map(|t| t.isolation == IsolationLevel::Serializable)
                .unwrap_or(false);
            (inner.committed.clone(), is_ser)
        };

        let mut results = Vec::new();
        let num_pages = heap.num_pages();

        for page_no in 0..num_pages {
            let page = heap
                .read_page(page_no)
                .map_err(|e| TxnError::Storage(storage::error::StorageError::Heap(e)))?;
            let num_slots = page.num_slots();

            for slot in 0..num_slots {
                // Try to get the raw bytes for this slot; skip dead slots
                let bytes = match page.get_tuple(slot) {
                    Ok(b) => b,
                    Err(_) => continue, // dead or out of range slot
                };

                // Decode the tuple header
                if bytes.len() < TUPLE_HEADER_SIZE {
                    continue;
                }
                let header = match TupleHeader::decode_from(bytes) {
                    Ok(h) => h,
                    Err(_) => continue,
                };

                // Check own-transaction visibility: tuples inserted by current xid are visible
                // to itself (within-transaction reads)
                let visible = if header.t_xmin == xid {
                    // Our own insert; visible unless we've also deleted it in this txn
                    header.t_xmax != xid
                } else if header.t_xmax == xid {
                    // Deleted (or updated-away) by our own transaction — not visible
                    false
                } else {
                    is_tuple_visible(&header, &snapshot, &committed)
                };

                if visible {
                    let tuple = match Tuple::decode(bytes) {
                        Ok(t) => t,
                        Err(_) => continue,
                    };
                    let tid = TID::new(page_no, slot);

                    // SSI: track read TIDs for Serializable transactions.
                    if is_serializable {
                        let mut inner = self.inner.lock();
                        if let Some(txn) = inner.active.get_mut(&xid) {
                            txn.read_set.insert((tid.page_no, tid.slot));
                        }
                    }

                    results.push((tid, tuple));
                }
            }
        }

        Ok(results)
    }

    /// Return a snapshot of the committed XID set (used by VACUUM for dead tuple detection).
    pub fn committed_set(&self) -> HashSet<Xid> {
        self.inner.lock().committed.clone()
    }

    /// Check if an XID is in the committed set (for testing/inspection).
    pub fn is_committed(&self, xid: Xid) -> bool {
        self.inner.lock().committed.contains(&xid)
    }

    /// Check if an XID is in the aborted set (for testing/inspection).
    pub fn is_aborted(&self, xid: Xid) -> bool {
        self.inner.lock().aborted.contains(&xid)
    }

    /// Check if an XID is currently active.
    pub fn is_active(&self, xid: Xid) -> bool {
        self.inner.lock().active.contains_key(&xid)
    }
}

/// Overwrite the header bytes of an existing tuple slot in a page.
/// The slot must already exist and have enough space for the encoded tuple.
fn write_tuple_to_page(
    page: &mut Page,
    slot: u16,
    tuple: &Tuple,
) -> Result<(), storage::error::PageError> {
    use storage::page::{ITEM_ID_SIZE, PAGE_HEADER_SIZE};

    // Read item pointer location from the page header
    let raw = page.as_bytes();
    let base = PAGE_HEADER_SIZE + slot as usize * ITEM_ID_SIZE;
    let lp_off = u16::from_le_bytes(raw[base..base + 2].try_into().unwrap()) as usize;
    let lp_len = u16::from_le_bytes(raw[base + 2..base + 4].try_into().unwrap()) as usize;

    let encoded = tuple.encode();
    if encoded.len() > lp_len {
        // The new encoded tuple must fit within the existing slot allocation.
        // For MVCC soft deletes, we only change the header (same size), so this should hold.
        return Err(storage::error::PageError::InsufficientSpace {
            needed: encoded.len(),
            available: lp_len as u16,
        });
    }

    // Write only the bytes that changed (the encoded slice fits within lp_len)
    let raw_mut = page.as_bytes_mut();
    raw_mut[lp_off..lp_off + encoded.len()].copy_from_slice(&encoded);

    Ok(())
}
