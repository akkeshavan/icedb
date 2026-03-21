use crate::xid::{Xid, INVALID_XID};
use std::collections::HashSet;

/// A consistent snapshot of the transaction state at a point in time.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Lowest active XID at snapshot time. All XIDs < xmin are committed.
    pub xmin: Xid,
    /// First XID that was NOT yet started at snapshot time. All XIDs >= xmax are invisible.
    pub xmax: Xid,
    /// Set of XIDs that were active (in-progress) at snapshot time.
    pub in_progress: HashSet<Xid>,
}

impl Snapshot {
    pub fn new(xmin: Xid, xmax: Xid, in_progress: HashSet<Xid>) -> Self {
        Snapshot {
            xmin,
            xmax,
            in_progress,
        }
    }

    /// Returns true if the given XID is visible in this snapshot.
    /// A committed XID is visible if it committed before this snapshot.
    /// An XID is invisible if it was in-progress or started after xmax.
    pub fn xid_is_visible(&self, xid: Xid) -> bool {
        if xid == INVALID_XID {
            return false;
        }
        if xid < self.xmin {
            return true;
        }
        if xid >= self.xmax {
            return false;
        }
        !self.in_progress.contains(&xid)
    }
}
