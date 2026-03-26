use crate::error::SqlError;
use crate::row::Row;
use crate::value::Value;

/// Encode a Row into raw bytes suitable for storage in a heap tuple.
/// Format: null bitmap (4-byte LE u32) prepended, then concatenated value bytes.
/// Bit i (0-indexed from LSB) is set if column i is NULL.
pub fn encode_row(row: &Row) -> (Vec<u8>, u32) {
    let mut null_bitmap: u32 = 0;
    let mut data = Vec::new();

    for (i, value) in row.values.iter().enumerate() {
        if matches!(value, Value::Null) {
            if i < 32 {
                null_bitmap |= 1u32 << i;
            }
        } else {
            data.extend_from_slice(&value.to_bytes());
        }
    }

    (data, null_bitmap)
}

/// Decode raw heap tuple bytes into a Row given the table schema.
pub fn decode_row(
    data: &[u8],
    null_bitmap: u32,
    schema: &catalog::schema::TableSchema,
) -> Result<Row, SqlError> {
    let mut values = Vec::with_capacity(schema.columns.len());
    let row_schema: Vec<(String, catalog::DataType)> = schema
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.data_type.clone()))
        .collect();

    let mut offset = 0usize;

    for (i, col) in schema.columns.iter().enumerate() {
        // Check if this column is null
        if i < 32 && (null_bitmap >> i) & 1 == 1 {
            values.push(Value::Null);
            continue;
        }

        let dtype = &col.data_type;
        match dtype {
            catalog::DataType::Boolean => {
                if offset + 1 > data.len() {
                    // Column was added after this tuple was written (ALTER TABLE ADD COLUMN) — return NULL
                    values.push(Value::Null);
                    continue;
                }
                let v = Value::from_bytes(&data[offset..offset + 1], dtype)?;
                values.push(v);
                offset += 1;
            }
            catalog::DataType::Int4 => {
                if offset + 4 > data.len() {
                    values.push(Value::Null);
                    continue;
                }
                let v = Value::from_bytes(&data[offset..offset + 4], dtype)?;
                values.push(v);
                offset += 4;
            }
            catalog::DataType::Int8 => {
                if offset + 8 > data.len() {
                    values.push(Value::Null);
                    continue;
                }
                let v = Value::from_bytes(&data[offset..offset + 8], dtype)?;
                values.push(v);
                offset += 8;
            }
            catalog::DataType::Float8 => {
                if offset + 8 > data.len() {
                    values.push(Value::Null);
                    continue;
                }
                let v = Value::from_bytes(&data[offset..offset + 8], dtype)?;
                values.push(v);
                offset += 8;
            }
            catalog::DataType::Text | catalog::DataType::VarChar(_) => {
                if offset + 4 > data.len() {
                    // Column was added after this tuple was written — return NULL
                    values.push(Value::Null);
                    continue;
                }
                let len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
                if offset + 4 + len > data.len() {
                    return Err(SqlError::Execution(format!(
                        "data too short for column {} (text data)",
                        col.name
                    )));
                }
                let v = Value::from_bytes(&data[offset..offset + 4 + len], dtype)?;
                values.push(v);
                offset += 4 + len;
            }
            catalog::DataType::Bytea => {
                if offset + 4 > data.len() {
                    values.push(Value::Null);
                    continue;
                }
                let len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
                if offset + 4 + len > data.len() {
                    return Err(SqlError::Execution(format!(
                        "data too short for column {} (bytea data)",
                        col.name
                    )));
                }
                let v = Value::from_bytes(&data[offset..offset + 4 + len], dtype)?;
                values.push(v);
                offset += 4 + len;
            }
            catalog::DataType::Date => {
                if offset + 4 > data.len() {
                    values.push(Value::Null);
                    continue;
                }
                let v = Value::from_bytes(&data[offset..offset + 4], dtype)?;
                values.push(v);
                offset += 4;
            }
            catalog::DataType::Timestamp | catalog::DataType::TimestampTz => {
                if offset + 8 > data.len() {
                    values.push(Value::Null);
                    continue;
                }
                let v = Value::from_bytes(&data[offset..offset + 8], dtype)?;
                values.push(v);
                offset += 8;
            }
            catalog::DataType::Numeric | catalog::DataType::Uuid => {
                if offset + 4 > data.len() {
                    values.push(Value::Null);
                    continue;
                }
                let len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
                if offset + 4 + len > data.len() {
                    return Err(SqlError::Execution(format!(
                        "data too short for column {} (variable data)",
                        col.name
                    )));
                }
                let v = Value::from_bytes(&data[offset..offset + 4 + len], dtype)?;
                values.push(v);
                offset += 4 + len;
            }
            catalog::DataType::Array(_) | catalog::DataType::Json | catalog::DataType::Jsonb => {
                if offset + 4 > data.len() {
                    values.push(Value::Null);
                    continue;
                }
                let len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
                if offset + 4 + len > data.len() {
                    return Err(SqlError::Execution(format!(
                        "data too short for column {} (array/json data)",
                        col.name
                    )));
                }
                let v = Value::from_bytes(&data[offset..offset + 4 + len], dtype)?;
                values.push(v);
                offset += 4 + len;
            }
        }
    }

    Ok(Row::new(values, row_schema))
}

/// Encode a single Value into sort key bytes (for B+ tree and ORDER BY).
pub fn encode_sort_key(value: &Value) -> Vec<u8> {
    match value {
        Value::Null => vec![0u8], // nulls sort first
        Value::Bool(b) => vec![1u8, if *b { 1 } else { 0 }],
        Value::Int4(v) => {
            // Flip sign bit for unsigned comparison
            let u = (*v as u32) ^ 0x8000_0000u32;
            let mut out = vec![2u8];
            out.extend_from_slice(&u.to_be_bytes());
            out
        }
        Value::Int8(v) => {
            let u = (*v as u64) ^ 0x8000_0000_0000_0000u64;
            let mut out = vec![3u8];
            out.extend_from_slice(&u.to_be_bytes());
            out
        }
        Value::Float8(v) => {
            let bits = v.to_bits();
            // IEEE 754: flip sign bit; if negative, flip all bits
            let u = if bits >> 63 == 0 {
                bits ^ 0x8000_0000_0000_0000u64
            } else {
                !bits
            };
            let mut out = vec![4u8];
            out.extend_from_slice(&u.to_be_bytes());
            out
        }
        Value::Text(s) => {
            let mut out = vec![5u8];
            out.extend_from_slice(s.as_bytes());
            out
        }
        Value::Bytes(b) => {
            let mut out = vec![6u8];
            out.extend_from_slice(b);
            out
        }
        Value::Date(d) => {
            // Flip sign bit for unsigned comparison of i32
            let u = (*d as u32) ^ 0x8000_0000u32;
            let mut out = vec![7u8];
            out.extend_from_slice(&u.to_be_bytes());
            out
        }
        Value::Timestamp(t) => {
            // Flip sign bit for unsigned comparison of i64
            let u = (*t as u64) ^ 0x8000_0000_0000_0000u64;
            let mut out = vec![8u8];
            out.extend_from_slice(&u.to_be_bytes());
            out
        }
        Value::Numeric(s) => {
            let mut out = vec![9u8];
            out.extend_from_slice(s.as_bytes());
            out
        }
        Value::Uuid(s) => {
            let mut out = vec![10u8];
            out.extend_from_slice(s.as_bytes());
            out
        }
        Value::Array(v) => {
            // Sort by string representation
            let s = format!("{}", Value::Array(v.clone()));
            let mut out = vec![11u8];
            out.extend_from_slice(s.as_bytes());
            out
        }
        Value::Json(s) => {
            let mut out = vec![12u8];
            out.extend_from_slice(s.as_bytes());
            out
        }
    }
}
