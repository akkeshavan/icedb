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

/// Format a result set in expanded (vertical) mode, like psql `\x`.
///
/// Each row is printed as a block with `column | value` lines, separated by
/// a record header `-- [ RECORD N ] ---`.
pub fn format_expanded(rows: &[Row]) -> String {
    if rows.is_empty() {
        return "(0 rows)\n".to_string();
    }

    let schema = &rows[0].schema;
    let headers: Vec<String> = schema.iter().map(|(name, _)| name.clone()).collect();
    let max_header_len = headers.iter().map(|h| h.len()).max().unwrap_or(0);

    let mut out = String::new();
    for (i, row) in rows.iter().enumerate() {
        out.push_str(&format!("-[ RECORD {} ]", i + 1));
        out.push('\n');
        for (header, value) in headers.iter().zip(row.values.iter()) {
            out.push_str(&format!(
                "{:<width$} | {}\n",
                header,
                format_value(value),
                width = max_header_len
            ));
        }
    }

    let row_count = rows.len();
    let plural = if row_count == 1 { "row" } else { "rows" };
    out.push_str(&format!("({} {})\n", row_count, plural));
    out
}

/// Format command result (non-SELECT output)
pub fn format_command_result(command: &str, _rows_affected: u64) -> String {
    format!("{}\n", command)
}

/// Format a result set received over the network (raw string columns/rows).
///
/// Used by the TCP client mode where values arrive as text rather than typed
/// [`Value`] variants.
pub fn format_pg_table(
    columns: &[String],
    rows: &[Vec<Option<String>>],
    expanded: bool,
) -> String {
    if columns.is_empty() {
        return "(0 rows)\n".to_string();
    }

    if expanded {
        return format_pg_expanded(columns, rows);
    }

    if rows.is_empty() {
        return "(0 rows)\n".to_string();
    }

    use tabled::builder::Builder;

    let mut builder = Builder::default();
    builder.push_record(columns.iter().map(String::as_str).collect::<Vec<_>>());

    for row in rows {
        let cells: Vec<String> = row
            .iter()
            .map(|v| v.as_deref().unwrap_or("NULL").to_string())
            .collect();
        builder.push_record(cells.iter().map(String::as_str).collect::<Vec<_>>());
    }

    let mut table = builder.build();
    table.with(Style::psql());

    let n = rows.len();
    format!("{}\n({} {})\n", table, n, if n == 1 { "row" } else { "rows" })
}

fn format_pg_expanded(columns: &[String], rows: &[Vec<Option<String>>]) -> String {
    if rows.is_empty() {
        return "(0 rows)\n".to_string();
    }

    let max_w = columns.iter().map(|c| c.len()).max().unwrap_or(0);
    let mut out = String::new();

    for (i, row) in rows.iter().enumerate() {
        out.push_str(&format!("-[ RECORD {} ]\n", i + 1));
        for (col, val) in columns.iter().zip(row.iter()) {
            let v = val.as_deref().unwrap_or("NULL");
            out.push_str(&format!("{:<width$} | {}\n", col, v, width = max_w));
        }
    }

    let n = rows.len();
    out.push_str(&format!("({} {})\n", n, if n == 1 { "row" } else { "rows" }));
    out
}
