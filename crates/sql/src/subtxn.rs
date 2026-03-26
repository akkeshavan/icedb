/// Undo log entry for a single DML operation.
///
/// Recorded by the executor for every tuple-level write that happens inside
/// a session transaction.  When the engine processes `ROLLBACK TO SAVEPOINT`,
/// it replays these entries in reverse order to restore the pre-savepoint state
/// without aborting the whole transaction.
#[derive(Debug, Clone)]
pub enum UndoEntry {
    /// A tuple was inserted after the savepoint.
    /// Undo by marking it deleted (set t_xmax = current xid).
    Insert { table_oid: u32, tid: storage::tid::TID },
    /// A tuple was soft-deleted after the savepoint.
    /// Undo by clearing t_xmax (restoring visibility).
    Delete { table_oid: u32, tid: storage::tid::TID },
}

/// A named point inside an open transaction.
///
/// `undo_log_start` is the index into the session's flat undo log at the moment
/// the savepoint was issued.  On `ROLLBACK TO SAVEPOINT`, entries from that
/// index onwards are applied in reverse and the log is truncated back to it.
#[derive(Debug)]
pub struct SavepointFrame {
    pub name: String,
    pub undo_log_start: usize,
}
