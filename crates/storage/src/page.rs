use crate::error::PageError;

/// Size of a single database page in bytes (8 kB, matching PostgreSQL default).
pub const PAGE_SIZE: usize = 8192;

// ── Byte offsets for PageHeader fields ──────────────────────────────────────
const OFF_LSN: usize = 0; // u64, 8 bytes
const OFF_CHECKSUM: usize = 8; // u16, 2 bytes
const OFF_FLAGS: usize = 10; // u16, 2 bytes
const OFF_LOWER: usize = 12; // u16, 2 bytes
const OFF_UPPER: usize = 14; // u16, 2 bytes
const OFF_SPECIAL: usize = 16; // u16, 2 bytes
const OFF_VERSION: usize = 18; // u16, 2 bytes
const OFF_PRUNE_XID: usize = 20; // u32, 4 bytes

/// Size of the fixed page header in bytes (24 bytes, matching PostgreSQL's PageHeaderData).
pub const PAGE_HEADER_SIZE: usize = 24;

/// Size of a single item pointer (slot) entry in bytes.
pub const ITEM_ID_SIZE: usize = 4; // lp_off: u16 + lp_len: u16

// Sentinel value for a dead (deleted) item pointer.
const ITEM_DEAD_OFF: u16 = 0;
const ITEM_DEAD_LEN: u16 = 0;

// ── Infomask constants ───────────────────────────────────────────────────────
/// Page flag: has checksum enabled.
pub const PD_HAS_CHECKSUM: u16 = 0x0001;

// ── Helper: raw little-endian read/write ─────────────────────────────────────
#[inline]
fn read_u16(buf: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(buf[off..off + 2].try_into().unwrap())
}

#[inline]
fn write_u16(buf: &mut [u8], off: usize, val: u16) {
    buf[off..off + 2].copy_from_slice(&val.to_le_bytes());
}

#[inline]
fn write_u32(buf: &mut [u8], off: usize, val: u32) {
    buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
}

#[inline]
fn read_u64(buf: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(buf[off..off + 8].try_into().unwrap())
}

#[inline]
fn write_u64(buf: &mut [u8], off: usize, val: u64) {
    buf[off..off + 8].copy_from_slice(&val.to_le_bytes());
}

// ── ItemId helpers ────────────────────────────────────────────────────────────

/// Item pointer stored in the page's line pointer array.
///
/// Lives at `PAGE_HEADER_SIZE + slot * ITEM_ID_SIZE` within the raw page bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemId {
    /// Byte offset of the tuple within the page (0 = dead).
    pub lp_off: u16,
    /// Byte length of the tuple (0 = dead).
    pub lp_len: u16,
}

impl ItemId {
    fn read(buf: &[u8], slot: u16) -> Self {
        let base = PAGE_HEADER_SIZE + slot as usize * ITEM_ID_SIZE;
        Self {
            lp_off: read_u16(buf, base),
            lp_len: read_u16(buf, base + 2),
        }
    }

    fn write(buf: &mut [u8], slot: u16, item: &ItemId) {
        let base = PAGE_HEADER_SIZE + slot as usize * ITEM_ID_SIZE;
        write_u16(buf, base, item.lp_off);
        write_u16(buf, base + 2, item.lp_len);
    }

    fn is_dead(&self) -> bool {
        self.lp_off == ITEM_DEAD_OFF && self.lp_len == ITEM_DEAD_LEN
    }
}

// ── Page ──────────────────────────────────────────────────────────────────────

/// An 8 kB database page.
///
/// The byte layout mirrors PostgreSQL's `PageHeaderData`:
///
/// ```text
/// Offset  Size  Field
///   0       8   pd_lsn        — LSN of last WAL record touching this page
///   8       2   pd_checksum   — CRC / XOR checksum
///  10       2   pd_flags
///  12       2   pd_lower      — end of item pointer array (start of free space)
///  14       2   pd_upper      — start of tuple data (end of free space)
///  16       2   pd_special    — start of special space
///  18       2   pd_version
///  20       4   pd_prune_xid
///  24  …       item pointers (ItemId array, grows downward from pd_lower)
///  …   …       free space
///  …   …       tuple data (grows upward toward pd_upper)
/// ```
pub struct Page {
    data: Box<[u8; PAGE_SIZE]>,
}

impl Page {
    /// Allocate a new zeroed page and initialise the header.
    pub fn new() -> Self {
        let mut p = Self {
            data: Box::new([0u8; PAGE_SIZE]),
        };
        p.init_header();
        p
    }

    /// Initialise/reset the page header fields to their default values.
    pub fn init_header(&mut self) {
        let buf = self.data.as_mut();
        write_u64(buf, OFF_LSN, 0);
        write_u16(buf, OFF_CHECKSUM, 0);
        write_u16(buf, OFF_FLAGS, 0);
        write_u16(buf, OFF_LOWER, PAGE_HEADER_SIZE as u16);
        write_u16(buf, OFF_UPPER, PAGE_SIZE as u16);
        write_u16(buf, OFF_SPECIAL, PAGE_SIZE as u16);
        write_u16(buf, OFF_VERSION, 1);
        write_u32(buf, OFF_PRUNE_XID, 0);
    }

    // ── Header field accessors ────────────────────────────────────────────────

    /// Return the page's LSN.
    pub fn lsn(&self) -> u64 {
        read_u64(self.data.as_ref(), OFF_LSN)
    }

    /// Set the page's LSN.
    pub fn set_lsn(&mut self, lsn: u64) {
        write_u64(self.data.as_mut(), OFF_LSN, lsn);
    }

    /// Return the stored checksum.
    pub fn checksum(&self) -> u16 {
        read_u16(self.data.as_ref(), OFF_CHECKSUM)
    }

    fn set_checksum(&mut self, val: u16) {
        write_u16(self.data.as_mut(), OFF_CHECKSUM, val);
    }

    /// Return `pd_lower` (end of item pointer array).
    pub fn pd_lower(&self) -> u16 {
        read_u16(self.data.as_ref(), OFF_LOWER)
    }

    fn set_pd_lower(&mut self, val: u16) {
        write_u16(self.data.as_mut(), OFF_LOWER, val);
    }

    /// Return `pd_upper` (start of tuple data region).
    pub fn pd_upper(&self) -> u16 {
        read_u16(self.data.as_ref(), OFF_UPPER)
    }

    fn set_pd_upper(&mut self, val: u16) {
        write_u16(self.data.as_mut(), OFF_UPPER, val);
    }

    /// Number of bytes of usable free space between the item pointer array and
    /// the tuple data region.
    pub fn free_space(&self) -> u16 {
        let lower = self.pd_lower();
        let upper = self.pd_upper();
        upper.saturating_sub(lower)
    }

    /// Number of item pointer slots currently allocated in this page.
    pub fn num_slots(&self) -> u16 {
        let lower = self.pd_lower() as usize;
        if lower < PAGE_HEADER_SIZE {
            return 0;
        }
        ((lower - PAGE_HEADER_SIZE) / ITEM_ID_SIZE) as u16
    }

    // ── Tuple operations ──────────────────────────────────────────────────────

    /// Insert `data` into the page and return the (0-based) slot index.
    ///
    /// The item pointer array grows from `pd_lower` downward; tuple data is
    /// written just below `pd_upper` and `pd_upper` is decremented.
    pub fn insert_tuple(&mut self, data: &[u8]) -> Result<u16, PageError> {
        let tuple_len = data.len();
        // Space needed: one new ItemId entry + the tuple bytes.
        let needed = ITEM_ID_SIZE + tuple_len;
        if needed > self.free_space() as usize {
            return Err(PageError::InsufficientSpace {
                needed,
                available: self.free_space(),
            });
        }

        let slot = self.num_slots();
        let new_lower = self.pd_lower() + ITEM_ID_SIZE as u16;
        let new_upper = self.pd_upper() - tuple_len as u16;

        // Write tuple bytes at new_upper.
        let off = new_upper as usize;
        self.data[off..off + tuple_len].copy_from_slice(data);

        // Write item pointer.
        ItemId::write(
            self.data.as_mut(),
            slot,
            &ItemId {
                lp_off: new_upper,
                lp_len: tuple_len as u16,
            },
        );

        self.set_pd_lower(new_lower);
        self.set_pd_upper(new_upper);

        Ok(slot)
    }

    /// Return a byte slice referencing the tuple stored at `slot`.
    pub fn get_tuple(&self, slot: u16) -> Result<&[u8], PageError> {
        let n = self.num_slots();
        if slot >= n {
            return Err(PageError::SlotOutOfRange(slot));
        }
        let item = ItemId::read(self.data.as_ref(), slot);
        if item.is_dead() {
            return Err(PageError::SlotDead(slot));
        }
        let off = item.lp_off as usize;
        let len = item.lp_len as usize;
        Ok(&self.data[off..off + len])
    }

    /// Mark the item pointer at `slot` as dead (logically delete the tuple).
    ///
    /// The tuple bytes remain on the page until a VACUUM pass reclaims them.
    pub fn delete_tuple(&mut self, slot: u16) -> Result<(), PageError> {
        let n = self.num_slots();
        if slot >= n {
            return Err(PageError::SlotOutOfRange(slot));
        }
        let item = ItemId::read(self.data.as_ref(), slot);
        if item.is_dead() {
            return Err(PageError::SlotDead(slot));
        }
        ItemId::write(
            self.data.as_mut(),
            slot,
            &ItemId {
                lp_off: ITEM_DEAD_OFF,
                lp_len: ITEM_DEAD_LEN,
            },
        );
        Ok(())
    }

    /// Mark the item pointer at `slot` as dead (alias for delete_tuple, used by VACUUM).
    pub fn mark_dead(&mut self, slot: u16) {
        let _ = self.delete_tuple(slot);
    }

    /// Set pd_prune_xid — the XID of the oldest dead tuple pruned from this page.
    pub fn set_prune_xid(&mut self, xid: u32) {
        write_u32(self.data.as_mut(), OFF_PRUNE_XID, xid);
    }

    /// Return pd_prune_xid.
    pub fn prune_xid(&self) -> u32 {
        u32::from_le_bytes(
            self.data[OFF_PRUNE_XID..OFF_PRUNE_XID + 4]
                .try_into()
                .unwrap_or([0; 4]),
        )
    }

    // ── Checksum ──────────────────────────────────────────────────────────────

    /// Compute a simple FNV-1a checksum over all page bytes, treating the two
    /// checksum bytes themselves as zero (so the stored checksum does not
    /// influence the computed value).
    pub fn compute_checksum(&self) -> u16 {
        const FNV_OFFSET: u32 = 2_166_136_261;
        const FNV_PRIME: u32 = 16_777_619;
        let buf = self.data.as_ref();
        let mut hash: u32 = FNV_OFFSET;
        for (i, &byte) in buf.iter().enumerate() {
            let b = if i == OFF_CHECKSUM || i == OFF_CHECKSUM + 1 {
                0u8
            } else {
                byte
            };
            hash ^= b as u32;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        (hash ^ (hash >> 16)) as u16
    }

    /// Write the computed checksum into the page header.
    pub fn update_checksum(&mut self) {
        let cs = self.compute_checksum();
        self.set_checksum(cs);
    }

    /// Return `true` iff the page's stored checksum matches the computed one.
    pub fn verify_checksum(&self) -> bool {
        self.checksum() == self.compute_checksum()
    }

    // ── Raw byte access ───────────────────────────────────────────────────────

    /// Return a reference to the raw page bytes (for I/O).
    pub fn as_bytes(&self) -> &[u8; PAGE_SIZE] {
        &self.data
    }

    /// Return a mutable reference to the raw page bytes (for I/O).
    pub fn as_bytes_mut(&mut self) -> &mut [u8; PAGE_SIZE] {
        &mut self.data
    }

    /// Construct a `Page` from a raw byte array read off disk.
    pub fn from_bytes(bytes: [u8; PAGE_SIZE]) -> Self {
        Self {
            data: Box::new(bytes),
        }
    }
}

impl Default for Page {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_header_layout() {
        let mut page = Page::new();
        // After init: lower = PAGE_HEADER_SIZE, upper = PAGE_SIZE.
        assert_eq!(page.pd_lower(), PAGE_HEADER_SIZE as u16);
        assert_eq!(page.pd_upper(), PAGE_SIZE as u16);
        assert_eq!(page.free_space(), (PAGE_SIZE - PAGE_HEADER_SIZE) as u16);

        let data = b"hello world";
        let slot = page.insert_tuple(data).unwrap();
        assert_eq!(slot, 0);

        // pd_lower grew by one ItemId.
        assert_eq!(page.pd_lower(), (PAGE_HEADER_SIZE + ITEM_ID_SIZE) as u16);
        // pd_upper shrank by tuple length.
        assert_eq!(page.pd_upper(), (PAGE_SIZE - data.len()) as u16);
        // free_space shrank by ITEM_ID_SIZE + data.len()
        let expected_free = PAGE_SIZE - PAGE_HEADER_SIZE - ITEM_ID_SIZE - data.len();
        assert_eq!(page.free_space(), expected_free as u16);
    }

    #[test]
    fn test_page_checksum() {
        let mut page = Page::new();
        page.insert_tuple(b"checksum test").unwrap();
        page.update_checksum();
        assert!(page.verify_checksum());

        // Corrupt a byte in the tuple data area.
        let upper = page.pd_upper() as usize;
        page.data[upper] ^= 0xFF;
        assert!(!page.verify_checksum());
    }

    #[test]
    fn test_page_insert_and_read() {
        let mut page = Page::new();
        let tuples: &[&[u8]] = &[b"alpha", b"beta", b"gamma delta epsilon"];
        let mut slots = Vec::new();
        for t in tuples {
            slots.push(page.insert_tuple(t).unwrap());
        }
        for (i, t) in tuples.iter().enumerate() {
            assert_eq!(page.get_tuple(slots[i]).unwrap(), *t);
        }
    }

    #[test]
    fn test_page_full() {
        let mut page = Page::new();
        // Fill the page with 100-byte tuples.
        let tuple = vec![0xABu8; 100];
        let mut count = 0usize;
        loop {
            match page.insert_tuple(&tuple) {
                Ok(_) => count += 1,
                Err(PageError::InsufficientSpace { .. }) => break,
                Err(e) => panic!("unexpected error: {e}"),
            }
        }
        assert!(count > 0, "should have inserted at least one tuple");
    }

    #[test]
    fn test_page_delete() {
        let mut page = Page::new();
        let slot = page.insert_tuple(b"to be deleted").unwrap();
        assert!(page.get_tuple(slot).is_ok());

        page.delete_tuple(slot).unwrap();
        assert!(matches!(page.get_tuple(slot), Err(PageError::SlotDead(_))));

        // Double-delete should fail.
        assert!(matches!(
            page.delete_tuple(slot),
            Err(PageError::SlotDead(_))
        ));
    }
}
