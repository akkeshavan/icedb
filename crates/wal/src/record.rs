use crate::error::WalError;
use crate::lsn::{Lsn, INVALID_LSN};

// ── CRC32 ─────────────────────────────────────────────────────────────────────

/// Compute a standard CRC-32/ISO-HDLC (IEEE 802.3) checksum.
/// Uses a simple table-free bit-by-bit implementation for portability.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

// ── WalRecordType ─────────────────────────────────────────────────────────────

/// Discriminant stored as a single byte in each WAL record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WalRecordType {
    /// Tuple inserted into a heap page.
    Insert = 1,
    /// Tuple deleted from a heap page (slot marked dead).
    Delete = 2,
    /// Tuple updated (old TID → new TID).
    Update = 3,
    /// Transaction committed.
    Commit = 4,
    /// Transaction aborted / rolled back.
    Abort = 5,
    /// Checkpoint record (all dirty pages were flushed up to this LSN).
    Checkpoint = 6,
    /// Full page image — used for crash recovery.
    PageImage = 7,
}

impl TryFrom<u8> for WalRecordType {
    type Error = WalError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Insert),
            2 => Ok(Self::Delete),
            3 => Ok(Self::Update),
            4 => Ok(Self::Commit),
            5 => Ok(Self::Abort),
            6 => Ok(Self::Checkpoint),
            7 => Ok(Self::PageImage),
            other => Err(WalError::InvalidRecordType(other)),
        }
    }
}

// ── WalRecord ─────────────────────────────────────────────────────────────────

/// Wire format (all little-endian):
///
/// ```text
/// [total_len: u32]   — length of everything INCLUDING this field and the trailing CRC32
/// [lsn: u64]
/// [prev_lsn: u64]
/// [xid: u32]
/// [type: u8]
/// [page_no: u32]
/// [data_len: u32]
/// [data: ...]
/// [crc32: u32]       — CRC32 over all preceding bytes
/// ```
///
/// Payload layouts by record type:
/// - `Insert`     : `[slot: u16][tuple_bytes: ...]`
/// - `Delete`     : `[slot: u16]`
/// - `Update`     : `[old_slot: u16][new_page_no: u32][new_slot: u16]`
/// - `Commit`     : empty
/// - `Abort`      : empty
/// - `Checkpoint` : empty
/// - `PageImage`  : raw 8192-byte page image
#[derive(Debug)]
pub struct WalRecord {
    /// Assigned by `WalWriter`; 0 (`INVALID_LSN`) on a freshly constructed record.
    pub lsn: Lsn,
    /// LSN of the previous record (enables backwards scans / undo chains).
    pub prev_lsn: Lsn,
    /// Transaction ID that produced this change.
    pub xid: u32,
    pub record_type: WalRecordType,
    /// Affected page (0 for `Commit` / `Abort` / `Checkpoint`).
    pub page_no: u32,
    /// Record-type-specific payload bytes.
    pub data: Vec<u8>,
}

// Fixed-size portion of the header that precedes the data:
//   total_len(4) + lsn(8) + prev_lsn(8) + xid(4) + type(1) + page_no(4) + data_len(4) = 33 bytes
const FIXED_HEADER_LEN: usize = 33;
// Trailing CRC field
const CRC_LEN: usize = 4;

impl WalRecord {
    /// Construct a new record that has not yet been assigned an LSN.
    pub fn new(xid: u32, record_type: WalRecordType, page_no: u32, data: Vec<u8>) -> Self {
        Self {
            lsn: INVALID_LSN,
            prev_lsn: INVALID_LSN,
            xid,
            record_type,
            page_no,
            data,
        }
    }

    /// Serialise the record to bytes, computing and appending the CRC32.
    pub fn encode(&self) -> Vec<u8> {
        let data_len = self.data.len() as u32;
        let total_len = (FIXED_HEADER_LEN + self.data.len() + CRC_LEN) as u32;

        let mut buf = Vec::with_capacity(total_len as usize);

        buf.extend_from_slice(&total_len.to_le_bytes());
        buf.extend_from_slice(&self.lsn.to_le_bytes());
        buf.extend_from_slice(&self.prev_lsn.to_le_bytes());
        buf.extend_from_slice(&self.xid.to_le_bytes());
        buf.push(self.record_type as u8);
        buf.extend_from_slice(&self.page_no.to_le_bytes());
        buf.extend_from_slice(&data_len.to_le_bytes());
        buf.extend_from_slice(&self.data);

        let checksum = crc32(&buf);
        buf.extend_from_slice(&checksum.to_le_bytes());

        buf
    }

    /// Deserialise a record from a byte slice, validating the CRC32.
    ///
    /// Returns `Err(WalError::CorruptRecord)` if the CRC does not match or the
    /// slice is too short.
    pub fn decode(bytes: &[u8]) -> Result<WalRecord, WalError> {
        // We need at least the fixed header.
        if bytes.len() < FIXED_HEADER_LEN + CRC_LEN {
            return Err(WalError::CorruptRecord {
                lsn: INVALID_LSN,
                reason: format!(
                    "slice too short: {} bytes (need at least {})",
                    bytes.len(),
                    FIXED_HEADER_LEN + CRC_LEN
                ),
            });
        }

        let total_len = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
        // total_len must be at least FIXED_HEADER_LEN + CRC_LEN to be valid.
        // Checking this before the subtraction below prevents a usize underflow.
        if total_len < FIXED_HEADER_LEN + CRC_LEN {
            return Err(WalError::CorruptRecord {
                lsn: INVALID_LSN,
                reason: format!(
                    "total_len field ({total_len}) smaller than minimum record size"
                ),
            });
        }
        if bytes.len() < total_len {
            return Err(WalError::CorruptRecord {
                lsn: INVALID_LSN,
                reason: format!(
                    "slice shorter than total_len field: {} < {}",
                    bytes.len(),
                    total_len
                ),
            });
        }

        // CRC covers everything except the trailing CRC field itself.
        let crc_offset = total_len - CRC_LEN;
        let stored_crc = u32::from_le_bytes(bytes[crc_offset..crc_offset + 4].try_into().unwrap());
        let computed_crc = crc32(&bytes[..crc_offset]);
        if stored_crc != computed_crc {
            let lsn = u64::from_le_bytes(bytes[4..12].try_into().unwrap());
            return Err(WalError::CorruptRecord {
                lsn,
                reason: format!(
                    "CRC mismatch (stored 0x{:08x}, computed 0x{:08x})",
                    stored_crc, computed_crc
                ),
            });
        }

        let lsn = u64::from_le_bytes(bytes[4..12].try_into().unwrap());
        let prev_lsn = u64::from_le_bytes(bytes[12..20].try_into().unwrap());
        let xid = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
        let record_type = WalRecordType::try_from(bytes[24])?;
        let page_no = u32::from_le_bytes(bytes[25..29].try_into().unwrap());
        let data_len = u32::from_le_bytes(bytes[29..33].try_into().unwrap()) as usize;

        if crc_offset < FIXED_HEADER_LEN + data_len {
            return Err(WalError::CorruptRecord {
                lsn,
                reason: format!("data_len field ({}) overflows available space", data_len),
            });
        }

        let data = bytes[FIXED_HEADER_LEN..FIXED_HEADER_LEN + data_len].to_vec();

        Ok(WalRecord {
            lsn,
            prev_lsn,
            xid,
            record_type,
            page_no,
            data,
        })
    }
}
