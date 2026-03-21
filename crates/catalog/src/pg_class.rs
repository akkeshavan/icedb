use crate::encoding::{write_f64, write_str, write_u16, write_u32, write_u8, Reader};
use crate::error::CatalogError;

/// One row per table or index in the catalog.
#[derive(Debug, Clone)]
pub struct PgClassRow {
    pub oid: u32,
    pub relname: String,   // table/index name (max 64 chars)
    pub relnamespace: u32, // namespace OID
    pub reltype: u32,      // type OID (0 for indexes)
    pub relkind: u8,       // 'r'=table, 'i'=index, 'v'=view
    pub relnatts: u16,     // number of columns
    pub relpages: u32,     // estimated number of pages
    pub reltuples: f64,    // estimated number of tuples
}

impl PgClassRow {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        write_u32(&mut buf, self.oid);
        write_str(&mut buf, &self.relname);
        write_u32(&mut buf, self.relnamespace);
        write_u32(&mut buf, self.reltype);
        write_u8(&mut buf, self.relkind);
        write_u16(&mut buf, self.relnatts);
        write_u32(&mut buf, self.relpages);
        write_f64(&mut buf, self.reltuples);
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<PgClassRow, CatalogError> {
        let mut r = Reader::new(bytes);
        Ok(PgClassRow {
            oid: r.read_u32()?,
            relname: r.read_str()?,
            relnamespace: r.read_u32()?,
            reltype: r.read_u32()?,
            relkind: r.read_u8()?,
            relnatts: r.read_u16()?,
            relpages: r.read_u32()?,
            reltuples: r.read_f64()?,
        })
    }
}
