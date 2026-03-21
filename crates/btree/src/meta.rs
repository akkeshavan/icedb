use crate::error::BTreeError;

pub const META_PAGE_NO: u32 = 0;

/// Magic number to identify a valid BTreeMeta page.
const META_MAGIC: u32 = 0xBEE7_1CEE;

#[derive(Debug, Clone)]
pub struct BTreeMeta {
    pub root_page: u32,
    pub height: u32,
    pub num_entries: u64,
    pub next_page: u32, // next free page number
}

impl BTreeMeta {
    /// Encode the meta into a fixed 32-byte buffer.
    /// Layout: [magic: u32][root_page: u32][height: u32][num_entries: u64][next_page: u32][padding: u8 * 8]
    pub fn encode(&self) -> [u8; 32] {
        let mut buf = [0u8; 32];
        buf[0..4].copy_from_slice(&META_MAGIC.to_le_bytes());
        buf[4..8].copy_from_slice(&self.root_page.to_le_bytes());
        buf[8..12].copy_from_slice(&self.height.to_le_bytes());
        buf[12..20].copy_from_slice(&self.num_entries.to_le_bytes());
        buf[20..24].copy_from_slice(&self.next_page.to_le_bytes());
        // bytes 24..32 are padding/reserved
        buf
    }

    /// Decode from a byte slice (must be at least 32 bytes).
    pub fn decode(bytes: &[u8]) -> Result<BTreeMeta, BTreeError> {
        if bytes.len() < 32 {
            return Err(BTreeError::CorruptMeta);
        }
        let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        if magic != META_MAGIC {
            return Err(BTreeError::CorruptMeta);
        }
        let root_page = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let height = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let num_entries = u64::from_le_bytes(bytes[12..20].try_into().unwrap());
        let next_page = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
        Ok(BTreeMeta {
            root_page,
            height,
            num_entries,
            next_page,
        })
    }
}
