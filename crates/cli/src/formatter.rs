use sql::row::Row;
use sql::value::Value;
use tabled::settings::Style;

/// Format a result set as an ASCII table.
pub fn format_table(rows: &[Row]) -> String {
    if rows.is_empty() {
        return "(0 rows)\n".to_string();
    }

    let schema = &rows[0].schema;
    let headers: Vec<String> = schema.iter().map(|(name, _)| name.clone()).collect();

    // Build a dynamic table using tabled
    use tabled::builder::Builder;

    let mut builder = Builder::default();
    builder.push_record(headers.iter().map(|s| s.as_str()).collect::<Vec<_>>());

    for row in rows {
        let cells: Vec<String> = row.values.iter().map(format_value).collect();
        builder.push_record(cells.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    }

    let mut table = builder.build();
    table.with(Style::psql());

    let row_count = rows.len();
    let plural = if row_count == 1 { "row" } else { "rows" };
    format!("{}\n({} {})\n", table, row_count, plural)
}

pub fn format_value(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => {
            if *b {
                "t".to_string()
            } else {
                "f".to_string()
            }
        }
        Value::Int4(i) => i.to_string(),
        Value::Int8(i) => i.to_string(),
        Value::Float8(f) => format!("{}", f),
        Value::Text(s) => s.clone(),
        Value::Bytes(b) => format!("\\x{}", hex_encode(b)),
        Value::Date(_)
        | Value::Timestamp(_)
        | Value::Numeric(_)
        | Value::Uuid(_)
        | Value::Array(_)
        | Value::Json(_) => {
            v.to_string()
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Format command result (non-SELECT output)
pub fn format_command_result(command: &str, _rows_affected: u64) -> String {
    format!("{}\n", command)
}
