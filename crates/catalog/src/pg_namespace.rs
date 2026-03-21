use crate::encoding::{write_str, write_u32, Reader};
use crate::error::CatalogError;

/// One row per namespace (schema) in the catalog.
#[derive(Debug, Clone)]
pub struct PgNamespaceRow {
    pub oid: u32,
    pub nspname: String,
    pub nspowner: u32,
}

impl PgNamespaceRow {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        write_u32(&mut buf, self.oid);
        write_str(&mut buf, &self.nspname);
        write_u32(&mut buf, self.nspowner);
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<PgNamespaceRow, CatalogError> {
        let mut r = Reader::new(bytes);
        Ok(PgNamespaceRow {
            oid: r.read_u32()?,
            nspname: r.read_str()?,
            nspowner: r.read_u32()?,
        })
    }
}
