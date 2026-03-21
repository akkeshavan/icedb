use crate::encoding::{write_bool, write_i16, write_i32, write_str, write_u32, Reader};
use crate::error::CatalogError;

/// One row per column in the catalog.
#[derive(Debug, Clone)]
pub struct PgAttributeRow {
    pub attrelid: u32,    // OID of the table
    pub attname: String,  // column name (max 64 chars)
    pub atttypid: u32,    // data type OID
    pub atttypmod: i32,   // type modifier (e.g., varchar length)
    pub attnum: i16,      // column number (1-based)
    pub attnotnull: bool, // NOT NULL constraint
    pub atthasdef: bool,  // has default value
    pub attisdropped: bool,
}

impl PgAttributeRow {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        write_u32(&mut buf, self.attrelid);
        write_str(&mut buf, &self.attname);
        write_u32(&mut buf, self.atttypid);
        write_i32(&mut buf, self.atttypmod);
        write_i16(&mut buf, self.attnum);
        write_bool(&mut buf, self.attnotnull);
        write_bool(&mut buf, self.atthasdef);
        write_bool(&mut buf, self.attisdropped);
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<PgAttributeRow, CatalogError> {
        let mut r = Reader::new(bytes);
        Ok(PgAttributeRow {
            attrelid: r.read_u32()?,
            attname: r.read_str()?,
            atttypid: r.read_u32()?,
            atttypmod: r.read_i32()?,
            attnum: r.read_i16()?,
            attnotnull: r.read_bool()?,
            atthasdef: r.read_bool()?,
            attisdropped: r.read_bool()?,
        })
    }
}
