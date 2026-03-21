/// Physical location of a tuple within the heap.
///
/// A TID (Tuple Identifier) uniquely addresses a tuple by combining a
/// page number and a slot index within that page's item pointer array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TID {
    /// Page number (0-based) within the heap file.
    pub page_no: u32,
    /// Slot index (0-based) within the page's item pointer array.
    pub slot: u16,
}

impl TID {
    /// Create a new TID from a page number and slot index.
    pub fn new(page_no: u32, slot: u16) -> Self {
        Self { page_no, slot }
    }
}

impl std::fmt::Display for TID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {})", self.page_no, self.slot)
    }
}
