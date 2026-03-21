use std::sync::atomic::{AtomicU32, Ordering};

pub type Xid = u32;
pub const INVALID_XID: Xid = 0;
pub const FROZEN_XID: Xid = 2; // always visible

static NEXT_XID: AtomicU32 = AtomicU32::new(3); // start from 3

pub fn allocate_xid() -> Xid {
    NEXT_XID.fetch_add(1, Ordering::SeqCst)
}

pub fn reset_xid_for_testing(next: Xid) {
    NEXT_XID.store(next, Ordering::SeqCst);
}
