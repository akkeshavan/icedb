use crate::error::TupleError;

// ── Infomask bit constants ────────────────────────────────────────────────────

/// Tuple has at least one NULL attribute.
pub const HEAP_HASNULL: u16 = 0x0001;
/// `t_xmin` has been committed.
pub const HEAP_XMIN_COMMITTED: u16 = 0x0100;
/// `t_xmin` has been rolled back / is invalid.
pub const HEAP_XMIN_INVALID: u16 = 0x0200;
/// `t_xmax` has been committed.
pub const HEAP_XMAX_COMMITTED: u16 = 0x0400;
/// `t_xmax` has been rolled back / is invalid.
pub const HEAP_XMAX_INVALID: u16 = 0x0800;
/// Tuple has been updated; ctid points to newer version.
pub const HEAP_UPDATED: u16 = 0x2000;

/// Size of `TupleHeader` on the wire / in the page.
///
/// Layout (all little-endian):
/// ```text
/// Offset  Size  Field
///   0       4   t_xmin
///   4       4   t_xmax
///   8       4   t_cid
///  12       4   t_ctid_page
///  16       2   t_ctid_slot
///  18       2   t_infomask
///  20       1   t_hoff
///  21       1   t_bits
/// ```
/// Total = 22 bytes.  We keep `t_hoff` pointing at offset 24 (next 8-byte
/// aligned boundary) so the user data always starts at a naturally aligned
/// address relative to the page start.
pub const TUPLE_HEADER_SIZE: usize = 22;
/// The value stored in `t_hoff`: user data starts 24 bytes after the tuple
/// start (header rounded up to the next 8-byte boundary).
pub const TUPLE_HOFF: u8 = 24;

// ── TupleHeader ───────────────────────────────────────────────────────────────

/// Fixed-size header prepended to every heap tuple.
///
/// We serialise / deserialise manually (little-endian) rather than relying on
/// `repr(C)` to avoid platform-dependent struct padding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TupleHeader {
    /// Transaction ID that inserted this tuple.
    pub t_xmin: u32,
    /// Transaction ID that deleted / updated this tuple (0 = live).
    pub t_xmax: u32,
    /// Command identifier within the inserting transaction.
    pub t_cid: u32,
    /// Page number of the newer version of this tuple (MVCC chain).
    pub t_ctid_page: u32,
    /// Slot number of the newer version of this tuple.
    pub t_ctid_slot: u16,
    /// Visibility / status flags (see `HEAP_*` constants).
    pub t_infomask: u16,
    /// Byte offset from tuple start to user data (always [`TUPLE_HOFF`]).
    pub t_hoff: u8,
    /// Null bitmap (1 byte — supports up to 8 attributes for Phase 1).
    pub t_bits: u8,
}

impl TupleHeader {
    /// Create a new header for a freshly inserted tuple.
    pub fn new() -> Self {
        Self {
            t_xmin: 0,
            t_xmax: 0,
            t_cid: 0,
            t_ctid_page: 0,
            t_ctid_slot: 0,
            t_infomask: HEAP_XMAX_INVALID, // no deleter yet
            t_hoff: TUPLE_HOFF,
            t_bits: 0,
        }
    }

    /// Serialise the header into 22 bytes (little-endian).
    pub fn encode_into(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.t_xmin.to_le_bytes());
        buf.extend_from_slice(&self.t_xmax.to_le_bytes());
        buf.extend_from_slice(&self.t_cid.to_le_bytes());
        buf.extend_from_slice(&self.t_ctid_page.to_le_bytes());
        buf.extend_from_slice(&self.t_ctid_slot.to_le_bytes());
        buf.extend_from_slice(&self.t_infomask.to_le_bytes());
        buf.push(self.t_hoff);
        buf.push(self.t_bits);
    }

    /// Deserialise a header from `bytes[0..TUPLE_HEADER_SIZE]`.
    pub fn decode_from(bytes: &[u8]) -> Result<Self, TupleError> {
        if bytes.len() < TUPLE_HEADER_SIZE {
            return Err(TupleError::TooShort {
                got: bytes.len(),
                need: TUPLE_HEADER_SIZE,
            });
        }
        Ok(Self {
            t_xmin: u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            t_xmax: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            t_cid: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            t_ctid_page: u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
            t_ctid_slot: u16::from_le_bytes(bytes[16..18].try_into().unwrap()),
            t_infomask: u16::from_le_bytes(bytes[18..20].try_into().unwrap()),
            t_hoff: bytes[20],
            t_bits: bytes[21],
        })
    }
}

impl Default for TupleHeader {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tuple ─────────────────────────────────────────────────────────────────────

/// A heap tuple: a fixed-size header followed by arbitrary user data bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tuple {
    /// Visibility / transaction metadata.
    pub header: TupleHeader,
    /// Raw user-data bytes (column values are stored here by higher layers).
    pub data: Vec<u8>,
}

impl Tuple {
    /// Create a new tuple wrapping `data` with a default header.
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            header: TupleHeader::new(),
            data,
        }
    }

    /// Serialise to a contiguous byte buffer.
    ///
    /// Layout:
    /// ```text
    /// [TupleHeader (22 bytes)] [2 bytes padding to reach t_hoff=24] [data]
    /// ```
    pub fn encode(&self) -> Vec<u8> {
        let total = self.header.t_hoff as usize + self.data.len();
        let mut buf = Vec::with_capacity(total);
        self.header.encode_into(&mut buf);
        // Pad to t_hoff boundary (2 bytes of zeroes for alignment).
        while buf.len() < self.header.t_hoff as usize {
            buf.push(0u8);
        }
        buf.extend_from_slice(&self.data);
        buf
    }

    /// Deserialise a tuple from `bytes`.
    pub fn decode(bytes: &[u8]) -> Result<Self, TupleError> {
        let header = TupleHeader::decode_from(bytes)?;
        let hoff = header.t_hoff as usize;
        if bytes.len() < hoff {
            return Err(TupleError::TooShort {
                got: bytes.len(),
                need: hoff,
            });
        }
        Ok(Self {
            header,
            data: bytes[hoff..].to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tuple_encode_decode() {
        let original = Tuple::new(b"hello, icedb!".to_vec());
        let encoded = original.encode();
        let decoded = Tuple::decode(&encoded).unwrap();
        assert_eq!(decoded.data, original.data);
        assert_eq!(decoded.header, original.header);
    }

    #[test]
    fn test_tuple_empty_data() {
        let original = Tuple::new(vec![]);
        let encoded = original.encode();
        assert_eq!(encoded.len(), TUPLE_HOFF as usize);
        let decoded = Tuple::decode(&encoded).unwrap();
        assert_eq!(decoded.data, Vec::<u8>::new());
    }

    #[test]
    fn test_tuple_decode_too_short() {
        let result = Tuple::decode(&[0u8; 5]);
        assert!(matches!(result, Err(TupleError::TooShort { .. })));
    }
}
