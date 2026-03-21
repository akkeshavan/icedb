use crate::snapshot::Snapshot;
use crate::xid::Xid;
use wal::lsn::Lsn;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionState {
    Active,
    Committed,
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolationLevel {
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

pub struct Transaction {
    pub xid: Xid,
    pub state: TransactionState,
    pub isolation: IsolationLevel,
    /// For RepeatableRead and Serializable: snapshot taken at BEGIN
    pub snapshot: Option<Snapshot>,
    /// Sequence of WAL LSNs written by this transaction
    pub wal_lsns: Vec<Lsn>,
    /// Command counter (incremented per DML statement within transaction)
    pub command_id: u32,
    /// SSI: set of (page_no, slot) this transaction has read (rw-antidependency tracking)
    pub read_set: std::collections::HashSet<(u32, u16)>,
    /// SSI: set of (page_no, slot) this transaction has written
    pub write_set: std::collections::HashSet<(u32, u16)>,
}

impl Transaction {
    pub fn new(xid: Xid, isolation: IsolationLevel) -> Self {
        Transaction {
            xid,
            state: TransactionState::Active,
            isolation,
            snapshot: None,
            wal_lsns: Vec::new(),
            command_id: 0,
            read_set: std::collections::HashSet::new(),
            write_set: std::collections::HashSet::new(),
        }
    }
}
