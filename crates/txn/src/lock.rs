use crate::error::TxnError;
use crate::xid::Xid;
use parking_lot::Mutex;
use std::collections::HashMap;
use storage::tid::TID;

struct LockState {
    write_locks: HashMap<(u32, u16), Xid>,
    wait_for: HashMap<Xid, Xid>,
}

pub struct LockManager {
    state: Mutex<LockState>,
}

impl LockManager {
    pub fn new() -> Self {
        LockManager {
            state: Mutex::new(LockState {
                write_locks: HashMap::new(),
                wait_for: HashMap::new(),
            }),
        }
    }

    /// Try to acquire an exclusive write lock on a tuple.
    /// Returns Err(WriteWriteConflict) if another active transaction holds the lock.
    /// Returns Err(Deadlock) if acquiring the lock would form a deadlock cycle.
    pub fn acquire_write_lock(&self, xid: Xid, tid: TID) -> Result<(), TxnError> {
        let mut state = self.state.lock();
        let key = (tid.page_no, tid.slot);
        match state.write_locks.get(&key) {
            Some(&holder) if holder != xid => {
                // Record that xid is waiting for holder
                state.wait_for.insert(xid, holder);

                // Walk the wait_for chain to detect a cycle
                let mut current = holder;
                let cycle = loop {
                    match state.wait_for.get(&current) {
                        Some(&next) if next == xid => break true,
                        Some(&next) => current = next,
                        None => break false,
                    }
                };

                // Clean up the wait_for entry before returning
                state.wait_for.remove(&xid);

                if cycle {
                    Err(TxnError::Deadlock { xid, holder })
                } else {
                    Err(TxnError::WriteWriteConflict { holder })
                }
            }
            _ => {
                state.write_locks.insert(key, xid);
                Ok(())
            }
        }
    }

    /// Release all locks held by xid (call on commit/abort).
    pub fn release_all(&self, xid: Xid) {
        let mut state = self.state.lock();
        state.write_locks.retain(|_, v| *v != xid);
        state.wait_for.remove(&xid);
    }
}

impl Default for LockManager {
    fn default() -> Self {
        Self::new()
    }
}
