use crate::encoding::{write_bool, write_opt_str, write_str, write_u32, Reader};
use crate::error::CatalogError;

/// One row per role in the catalog.
#[derive(Debug, Clone)]
pub struct PgAuthidRow {
    pub oid: u32,
    pub rolname: String, // max 64 chars
    pub rolsuper: bool,
    pub rolinherit: bool,
    pub rolcreaterole: bool,
    pub rolcreatedb: bool,
    pub rolcanlogin: bool,
    pub rolbypassrls: bool,
    pub rolpassword: Option<String>, // SCRAM verifier or None
}

impl PgAuthidRow {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        write_u32(&mut buf, self.oid);
        write_str(&mut buf, &self.rolname);
        write_bool(&mut buf, self.rolsuper);
        write_bool(&mut buf, self.rolinherit);
        write_bool(&mut buf, self.rolcreaterole);
        write_bool(&mut buf, self.rolcreatedb);
        write_bool(&mut buf, self.rolcanlogin);
        write_bool(&mut buf, self.rolbypassrls);
        write_opt_str(&mut buf, self.rolpassword.as_deref());
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<PgAuthidRow, CatalogError> {
        let mut r = Reader::new(bytes);
        Ok(PgAuthidRow {
            oid: r.read_u32()?,
            rolname: r.read_str()?,
            rolsuper: r.read_bool()?,
            rolinherit: r.read_bool()?,
            rolcreaterole: r.read_bool()?,
            rolcreatedb: r.read_bool()?,
            rolcanlogin: r.read_bool()?,
            rolbypassrls: r.read_bool()?,
            rolpassword: r.read_opt_str()?,
        })
    }
}
