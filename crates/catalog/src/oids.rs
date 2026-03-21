// System OIDs (below 1000 are reserved)
pub const OID_PG_CLASS: u32 = 1; // table registry
pub const OID_PG_ATTRIBUTE: u32 = 2; // column definitions
pub const OID_PG_AUTHID: u32 = 3; // roles
pub const OID_PG_NAMESPACE: u32 = 4; // schemas
pub const OID_PG_TYPE: u32 = 5; // data types
pub const OID_PG_STATISTIC: u32 = 6; // column statistics

// Well-known namespace OIDs
pub const OID_NS_PUBLIC: u32 = 100; // public schema
pub const OID_NS_PG_CATALOG: u32 = 101; // pg_catalog schema

// Well-known type OIDs (subset of Postgres type OIDs)
pub const OID_TYPE_BOOL: u32 = 16;
pub const OID_TYPE_INT4: u32 = 23;
pub const OID_TYPE_INT8: u32 = 20;
pub const OID_TYPE_TEXT: u32 = 25;
pub const OID_TYPE_FLOAT8: u32 = 701;
pub const OID_TYPE_BYTEA: u32 = 17;
pub const OID_TYPE_VARCHAR: u32 = 1043;
pub const OID_TYPE_DATE: u32 = 1082;
pub const OID_TYPE_TIMESTAMP: u32 = 1114;
pub const OID_TYPE_TIMESTAMPTZ: u32 = 1184;
pub const OID_TYPE_NUMERIC: u32 = 1700;
pub const OID_TYPE_UUID: u32 = 2950;

// Superuser role OID
pub const OID_ROLE_SUPERUSER: u32 = 10;

// User OID counter starts here
pub const USER_OID_START: u32 = 1000;
