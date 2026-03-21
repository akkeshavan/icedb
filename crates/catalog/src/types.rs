use crate::oids::*;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DataType {
    Boolean,
    Int4,
    Int8,
    Float8,
    Text,
    VarChar(u32), // max length
    Bytea,
    Date,           // days since Unix epoch (i32)
    Timestamp,      // microseconds since Unix epoch (i64)
    TimestampTz,    // same as Timestamp, stored as UTC i64
    Numeric,        // exact decimal as string
    Uuid,           // UUID string
}

impl DataType {
    pub fn oid(&self) -> u32 {
        match self {
            DataType::Boolean => OID_TYPE_BOOL,
            DataType::Int4 => OID_TYPE_INT4,
            DataType::Int8 => OID_TYPE_INT8,
            DataType::Float8 => OID_TYPE_FLOAT8,
            DataType::Text => OID_TYPE_TEXT,
            DataType::VarChar(_) => OID_TYPE_VARCHAR,
            DataType::Bytea => OID_TYPE_BYTEA,
            DataType::Date => OID_TYPE_DATE,
            DataType::Timestamp => OID_TYPE_TIMESTAMP,
            DataType::TimestampTz => OID_TYPE_TIMESTAMPTZ,
            DataType::Numeric => OID_TYPE_NUMERIC,
            DataType::Uuid => OID_TYPE_UUID,
        }
    }

    pub fn from_oid(oid: u32, typmod: i32) -> Option<DataType> {
        match oid {
            OID_TYPE_BOOL => Some(DataType::Boolean),
            OID_TYPE_INT4 => Some(DataType::Int4),
            OID_TYPE_INT8 => Some(DataType::Int8),
            OID_TYPE_FLOAT8 => Some(DataType::Float8),
            OID_TYPE_TEXT => Some(DataType::Text),
            OID_TYPE_VARCHAR => {
                // typmod encodes max length as (max_len + 4) in Postgres convention
                // If typmod is -1 (no modifier), use a large default
                let max_len = if typmod > 4 {
                    (typmod - 4) as u32
                } else if typmod == -1 {
                    u32::MAX
                } else {
                    255
                };
                Some(DataType::VarChar(max_len))
            }
            OID_TYPE_BYTEA => Some(DataType::Bytea),
            OID_TYPE_DATE => Some(DataType::Date),
            OID_TYPE_TIMESTAMP => Some(DataType::Timestamp),
            OID_TYPE_TIMESTAMPTZ => Some(DataType::TimestampTz),
            OID_TYPE_NUMERIC => Some(DataType::Numeric),
            OID_TYPE_UUID => Some(DataType::Uuid),
            _ => None,
        }
    }

    pub fn is_variable_length(&self) -> bool {
        matches!(
            self,
            DataType::Text | DataType::VarChar(_) | DataType::Bytea
        )
    }

    pub fn fixed_size(&self) -> Option<usize> {
        match self {
            DataType::Boolean => Some(1),
            DataType::Int4 => Some(4),
            DataType::Int8 => Some(8),
            DataType::Float8 => Some(8),
            DataType::Date => Some(4),
            DataType::Timestamp | DataType::TimestampTz => Some(8),
            DataType::Text => None,
            DataType::VarChar(_) => None,
            DataType::Bytea => None,
            DataType::Numeric => None,
            DataType::Uuid => None,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            DataType::Boolean => "boolean",
            DataType::Int4 => "integer",
            DataType::Int8 => "bigint",
            DataType::Float8 => "double precision",
            DataType::Text => "text",
            DataType::VarChar(_) => "character varying",
            DataType::Bytea => "bytea",
            DataType::Date => "date",
            DataType::Timestamp => "timestamp without time zone",
            DataType::TimestampTz => "timestamp with time zone",
            DataType::Numeric => "numeric",
            DataType::Uuid => "uuid",
        }
    }

    /// Return the typmod encoding for this type (for storage in pg_attribute).
    /// Returns -1 if no modifier.
    pub fn typmod(&self) -> i32 {
        match self {
            DataType::VarChar(max_len) => {
                if *max_len == u32::MAX {
                    -1
                } else {
                    (*max_len as i32) + 4
                }
            }
            _ => -1,
        }
    }
}
