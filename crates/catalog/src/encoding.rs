//! Simple binary encoding helpers (all little-endian).
//! Used by catalog row encode/decode implementations.

use crate::error::CatalogError;

// ── Writers ──────────────────────────────────────────────────────────────────

pub fn write_u8(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}

pub fn write_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub fn write_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub fn write_i16(buf: &mut Vec<u8>, v: i16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub fn write_i32(buf: &mut Vec<u8>, v: i32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub fn write_f64(buf: &mut Vec<u8>, v: f64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub fn write_bool(buf: &mut Vec<u8>, v: bool) {
    buf.push(if v { 1 } else { 0 });
}

/// Write a length-prefixed string: [len: u16][utf8 bytes]
pub fn write_str(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len() as u16;
    write_u16(buf, len);
    buf.extend_from_slice(bytes);
}

/// Write an optional string: [present: u8][len: u16][utf8 bytes]
pub fn write_opt_str(buf: &mut Vec<u8>, s: Option<&str>) {
    match s {
        Some(s) => {
            write_u8(buf, 1);
            write_str(buf, s);
        }
        None => {
            write_u8(buf, 0);
        }
    }
}

// ── Readers ───────────────────────────────────────────────────────────────────

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn need(&self, n: usize) -> Result<(), CatalogError> {
        if self.pos + n > self.buf.len() {
            Err(CatalogError::CorruptCatalog(format!(
                "unexpected end of data: need {} bytes at offset {}, have {}",
                n,
                self.pos,
                self.buf.len()
            )))
        } else {
            Ok(())
        }
    }

    pub fn read_u8(&mut self) -> Result<u8, CatalogError> {
        self.need(1)?;
        let v = self.buf[self.pos];
        self.pos += 1;
        Ok(v)
    }

    pub fn read_u16(&mut self) -> Result<u16, CatalogError> {
        self.need(2)?;
        let v = u16::from_le_bytes(self.buf[self.pos..self.pos + 2].try_into().unwrap());
        self.pos += 2;
        Ok(v)
    }

    pub fn read_u32(&mut self) -> Result<u32, CatalogError> {
        self.need(4)?;
        let v = u32::from_le_bytes(self.buf[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }

    pub fn read_i16(&mut self) -> Result<i16, CatalogError> {
        self.need(2)?;
        let v = i16::from_le_bytes(self.buf[self.pos..self.pos + 2].try_into().unwrap());
        self.pos += 2;
        Ok(v)
    }

    pub fn read_i32(&mut self) -> Result<i32, CatalogError> {
        self.need(4)?;
        let v = i32::from_le_bytes(self.buf[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }

    pub fn read_f64(&mut self) -> Result<f64, CatalogError> {
        self.need(8)?;
        let v = f64::from_le_bytes(self.buf[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;
        Ok(v)
    }

    pub fn read_bool(&mut self) -> Result<bool, CatalogError> {
        Ok(self.read_u8()? != 0)
    }

    pub fn read_str(&mut self) -> Result<String, CatalogError> {
        let len = self.read_u16()? as usize;
        self.need(len)?;
        let s = std::str::from_utf8(&self.buf[self.pos..self.pos + len])
            .map_err(|e| CatalogError::CorruptCatalog(format!("invalid UTF-8: {e}")))?
            .to_string();
        self.pos += len;
        Ok(s)
    }

    pub fn read_opt_str(&mut self) -> Result<Option<String>, CatalogError> {
        let present = self.read_u8()?;
        if present != 0 {
            Ok(Some(self.read_str()?))
        } else {
            Ok(None)
        }
    }
}
