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

/// Advance the global NEXT_XID to at least `min_val`.
///
/// Called during WAL recovery so that newly allocated XIDs are always
/// greater than any XID written in a previous session.  This prevents
/// a restarted process from re-using an XID that already appears in
/// committed tuples, which would make those tuples invisible (the new
/// snapshot's `xmax` would be ≤ the old committed XID).
pub fn advance_next_xid(min_val: Xid) {
    let mut current = NEXT_XID.load(Ordering::Relaxed);
    loop {
        if current >= min_val {
            break;
        }
        match NEXT_XID.compare_exchange_weak(
            current,
            min_val,
            Ordering::SeqCst,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(actual) => current = actual,
        }
    }
}
