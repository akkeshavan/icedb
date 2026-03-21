use crate::snapshot::Snapshot;
use crate::xid::{Xid, INVALID_XID};
use std::collections::HashSet;
use storage::tuple::TupleHeader;

/// Returns true if this tuple version should be visible to the given snapshot.
pub fn is_tuple_visible(
    header: &TupleHeader,
    snapshot: &Snapshot,
    committed: &HashSet<Xid>,
) -> bool {
    // xmin must be a committed transaction visible to the snapshot
    let xmin_committed = committed.contains(&header.t_xmin);
    if !xmin_committed {
        return false;
    }
    if !snapshot.xid_is_visible(header.t_xmin) {
        return false;
    }
    // xmax: if 0, not deleted → visible
    if header.t_xmax == INVALID_XID {
        return true;
    }
    // if xmax is committed and visible, tuple is deleted
    let xmax_committed = committed.contains(&header.t_xmax);
    if xmax_committed && snapshot.xid_is_visible(header.t_xmax) {
        return false; // deleted, not visible
    }
    true
}
