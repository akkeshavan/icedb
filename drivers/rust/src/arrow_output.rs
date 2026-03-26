use std::sync::Arc;
use arrow::array::{
    Array, BooleanArray, Float64Array, Int32Array, Int64Array, StringArray,
};
use arrow::datatypes::{DataType as ArrowType, Field, Schema};
use arrow::record_batch::RecordBatch;
use catalog::types::DataType as IceType;
use sql::row::Row;
use sql::value::Value;
use crate::error::DriverError;

/// Convert a slice of icedb rows into an Arrow [`RecordBatch`].
///
/// Column types are derived from the schema attached to the first row.
/// An empty slice returns an empty `RecordBatch` with an empty schema.
pub fn rows_to_record_batch(rows: &[Row]) -> Result<RecordBatch, DriverError> {
    if rows.is_empty() {
        return RecordBatch::try_new(Arc::new(Schema::empty()), vec![])
            .map_err(|e| DriverError::TypeConversion(e.to_string()));
    }

    let schema_cols = &rows[0].schema;
    let fields: Vec<Field> = schema_cols
        .iter()
        .map(|(name, dt)| Field::new(name.as_str(), ice_to_arrow(dt), true))
        .collect();
    let schema = Arc::new(Schema::new(fields));

    let arrays: Vec<Arc<dyn Array>> = (0..schema_cols.len())
        .map(|col_idx| {
            let vals: Vec<&Value> = rows.iter().map(|r| &r.values[col_idx]).collect();
            values_to_array(&vals, &schema_cols[col_idx].1)
        })
        .collect();

    RecordBatch::try_new(schema, arrays)
        .map_err(|e| DriverError::TypeConversion(e.to_string()))
}

fn ice_to_arrow(dt: &IceType) -> ArrowType {
    match dt {
        IceType::Boolean => ArrowType::Boolean,
        IceType::Int4 => ArrowType::Int32,
        IceType::Int8 => ArrowType::Int64,
        IceType::Float8 => ArrowType::Float64,
        IceType::Bytea => ArrowType::Binary,
        _ => ArrowType::Utf8,
    }
}

fn values_to_array(values: &[&Value], dt: &IceType) -> Arc<dyn Array> {
    match dt {
        IceType::Boolean => Arc::new(
            values.iter().map(|v| match v {
                Value::Bool(b) => Some(*b),
                _ => None,
            })
            .collect::<BooleanArray>(),
        ),
        IceType::Int4 => Arc::new(
            values.iter().map(|v| match v {
                Value::Int4(i) => Some(*i),
                _ => None,
            })
            .collect::<Int32Array>(),
        ),
        IceType::Int8 => Arc::new(
            values.iter().map(|v| match v {
                Value::Int8(i) => Some(*i),
                Value::Int4(i) => Some(*i as i64),
                _ => None,
            })
            .collect::<Int64Array>(),
        ),
        IceType::Float8 => Arc::new(
            values.iter().map(|v| match v {
                Value::Float8(f) => Some(*f),
                _ => None,
            })
            .collect::<Float64Array>(),
        ),
        _ => Arc::new(
            values.iter().map(|v| match v {
                Value::Null => None,
                other => Some(other.to_string()),
            })
            .collect::<StringArray>(),
        ),
    }
}
