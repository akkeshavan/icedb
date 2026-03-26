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
    Array(Box<DataType>), // e.g. INT[], TEXT[]
    Json,                 // JSON text
    Jsonb,                // Binary JSON (stored same as JSON internally)
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
            DataType::Array(_) => 2277, // anyarray
            DataType::Json => 114,
            DataType::Jsonb => 3802,
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
            DataType::Text
                | DataType::VarChar(_)
                | DataType::Bytea
                | DataType::Array(_)
                | DataType::Json
                | DataType::Jsonb
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
            DataType::Array(_) => None,
            DataType::Json => None,
            DataType::Jsonb => None,
        }
    }

    pub fn type_name(&self) -> String {
        match self {
            DataType::Boolean => "boolean".to_string(),
            DataType::Int4 => "integer".to_string(),
            DataType::Int8 => "bigint".to_string(),
            DataType::Float8 => "double precision".to_string(),
            DataType::Text => "text".to_string(),
            DataType::VarChar(_) => "character varying".to_string(),
            DataType::Bytea => "bytea".to_string(),
            DataType::Date => "date".to_string(),
            DataType::Timestamp => "timestamp without time zone".to_string(),
            DataType::TimestampTz => "timestamp with time zone".to_string(),
            DataType::Numeric => "numeric".to_string(),
            DataType::Uuid => "uuid".to_string(),
            DataType::Array(inner) => format!("{}[]", inner.type_name()),
            DataType::Json => "json".to_string(),
            DataType::Jsonb => "jsonb".to_string(),
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
