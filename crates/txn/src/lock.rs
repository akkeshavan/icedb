use crate::error::TxnError;
use crate::xid::Xid;
use parking_lot::Mutex;
use std::collections::HashMap;
use storage::tid::TID;

pub struct LockManager {
    // Maps (page_no, slot) → XID holding a write lock
    write_locks: Mutex<HashMap<(u32, u16), Xid>>,
}

impl LockManager {
    pub fn new() -> Self {
        LockManager {
            write_locks: Mutex::new(HashMap::new()),
        }
    }

    /// Try to acquire an exclusive write lock on a tuple.
    /// Returns Err(WriteWriteConflict) if another active transaction holds the lock.
    pub fn acquire_write_lock(&self, xid: Xid, tid: TID) -> Result<(), TxnError> {
        let mut locks = self.write_locks.lock();
        let key = (tid.page_no, tid.slot);
        match locks.get(&key) {
            Some(&holder) if holder != xid => Err(TxnError::WriteWriteConflict { holder }),
            _ => {
                locks.insert(key, xid);
                Ok(())
            }
        }
    }

    /// Release all locks held by xid (call on commit/abort).
    pub fn release_all(&self, xid: Xid) {
        let mut locks = self.write_locks.lock();
        locks.retain(|_, v| *v != xid);
    }
}

impl Default for LockManager {
    fn default() -> Self {
        Self::new()
    }
}
