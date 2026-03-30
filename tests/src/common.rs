use std::sync::Arc;
use std::path::Path;
use sql::engine::QueryEngine;
use txn::manager::TransactionManager;
use catalog::manager::CatalogManager;
use wal::writer::WalWriter;
use cli::pg_client::PgClient;
use catalog;

pub fn make_engine(dir: &Path) -> Arc<QueryEngine> {
    let wal = Arc::new(WalWriter::open(dir).unwrap());
    let txn = Arc::new(TransactionManager::new_with_wal_recovery(Arc::clone(&wal), dir));
    let cat = Arc::new(CatalogManager::open(dir, Arc::clone(&wal), Arc::clone(&txn)).unwrap());
    Arc::new(QueryEngine::new(txn, cat, dir.to_path_buf()))
}

// ── Backend abstraction ──────────────────────────────────────────────────────

pub enum Backend {
    Embedded(Arc<QueryEngine>),
    Network(std::sync::Mutex<PgClient>),
}

impl Backend {
    pub fn embedded(dir: &Path) -> Self {
        Backend::Embedded(make_engine(dir))
    }

    pub fn execute(&self, sql: &str) -> sql::ExecutionResult {
        match self {
            Backend::Embedded(engine) => {
                engine.execute(sql)
                    .unwrap_or_else(|e| panic!("SQL failed: {}\nSQL: {}", e, sql))
            }
            Backend::Network(client) => {
                let mut c = client.lock().unwrap();
                match c.query(sql) {
                    Ok(r) => pg_result_to_execution_result(r),
                    Err(e) => panic!("Network SQL failed: {}\nSQL: {}", e, sql),
                }
            }
        }
    }

    pub fn execute_err(&self, sql: &str) -> sql::SqlError {
        match self {
            Backend::Embedded(engine) => {
                engine.execute(sql)
                    .expect_err("Expected SQL error but got success")
            }
            Backend::Network(client) => {
                let mut c = client.lock().unwrap();
                match c.query(sql) {
                    Err(e) => pg_error_to_sql_error(cli_error_inner_msg(e)),
                    Ok(_) => panic!("Expected SQL error but got success for: {}", sql),
                }
            }
        }
    }

    pub fn execute_session(&self, session_id: &str, sql: &str) -> sql::ExecutionResult {
        match self {
            Backend::Embedded(engine) => {
                engine.execute_session(session_id, sql)
                    .unwrap_or_else(|e| panic!("SQL failed in session '{}': {}\nSQL: {}", session_id, e, sql))
            }
            Backend::Network(client) => {
                // In network mode, all session calls use the same TCP connection
                let mut c = client.lock().unwrap();
                match c.query(sql) {
                    Ok(r) => pg_result_to_execution_result(r),
                    Err(e) => panic!("Network SQL failed in session '{}': {}\nSQL: {}", session_id, e, sql),
                }
            }
        }
    }

    pub fn execute_session_err(&self, session_id: &str, sql: &str) -> sql::SqlError {
        match self {
            Backend::Embedded(engine) => {
                engine.execute_session(session_id, sql)
                    .expect_err("Expected SQL error but got success")
            }
            Backend::Network(client) => {
                let mut c = client.lock().unwrap();
                match c.query(sql) {
                    Err(e) => pg_error_to_sql_error(e.to_string()),
                    Ok(_) => panic!("Expected SQL error in session '{}' but got success for: {}", session_id, sql),
                }
            }
        }
    }

    /// Execute SQL and return Result — allows tests to check .is_err()/.is_ok().
    pub fn try_execute(&self, sql: &str) -> Result<sql::ExecutionResult, sql::SqlError> {
        match self {
            Backend::Embedded(engine) => engine.execute(sql),
            Backend::Network(client) => {
                let mut c = client.lock().unwrap();
                match c.query(sql) {
                    Ok(r) => Ok(pg_result_to_execution_result(r)),
                    Err(e) => Err(pg_error_to_sql_error(cli_error_inner_msg(e))),
                }
            }
        }
    }

    /// Unwrap the embedded engine (panics in network mode — embedded-only tests).
    pub fn as_engine(&self) -> &Arc<QueryEngine> {
        match self {
            Backend::Embedded(e) => e,
            Backend::Network(_) => panic!("as_engine() called on Network backend"),
        }
    }

    /// Returns true for network backends (plain TCP or TLS).
    pub fn is_network(&self) -> bool {
        matches!(self, Backend::Network(_))
    }
}

/// Convert a PgResult (string-based) into an ExecutionResult (typed Values).
/// Uses column type OIDs from the RowDescription message for accurate type mapping.
fn pg_result_to_execution_result(pr: cli::pg_client::PgResult) -> sql::ExecutionResult {
    let oids = &pr.col_type_oids;
    let col_names = pr.columns.clone();
    let rows: Vec<sql::Row> = pr.rows.iter().map(|row| {
        let values: Vec<sql::Value> = row.iter().enumerate().map(|(i, v)| {
            match v {
                None => sql::Value::Null,
                Some(s) => {
                    let oid = oids.get(i).copied().unwrap_or(0);
                    parse_pg_value_typed(s, oid)
                }
            }
        }).collect();
        // Build a schema with (name, DataType) pairs so r.get("col_name") works
        let schema: Vec<(String, catalog::DataType)> = col_names.iter().zip(oids.iter())
            .map(|(name, &oid)| (name.clone(), oid_to_datatype(oid)))
            .collect();
        sql::Row::new(values, schema)
    }).collect();
    sql::ExecutionResult {
        rows,
        rows_affected: pr.rows_affected,
        command: pr.command_tag,
        col_names: pr.columns,
        col_types: vec![],
    }
}

/// Map a PostgreSQL type OID to a catalog DataType (best effort).
fn oid_to_datatype(oid: u32) -> catalog::DataType {
    match oid {
        16 => catalog::DataType::Boolean,
        23 => catalog::DataType::Int4,
        20 => catalog::DataType::Int8,
        700 | 701 => catalog::DataType::Float8,
        25 | 1042 => catalog::DataType::Text,
        1043 => catalog::DataType::VarChar(0),
        17 => catalog::DataType::Bytea,
        1082 => catalog::DataType::Date,
        1114 => catalog::DataType::Timestamp,
        1184 => catalog::DataType::TimestampTz,
        1700 => catalog::DataType::Numeric,
        2950 => catalog::DataType::Uuid,
        114 => catalog::DataType::Json,
        3802 => catalog::DataType::Jsonb,
        _ if is_array_oid(oid) => catalog::DataType::Array(Box::new(catalog::DataType::Text)),
        _ => catalog::DataType::Text, // fallback
    }
}

/// Parse a text-format PostgreSQL value using the column's type OID.
///
/// Common OIDs:
///   16   bool       23   int4       20   int8
///   700  float4     701  float8     25   text / 1043 varchar
///   1082 date       1114 timestamp  1700 numeric
///   2950 uuid       199  json array 114  json
/// Parse a PostgreSQL date string "YYYY-MM-DD" into Value::Date(days_since_epoch).
fn parse_pg_date(s: &str) -> sql::Value {
    let parts: Vec<&str> = s.splitn(3, '-').collect();
    if parts.len() == 3 {
        if let (Ok(y), Ok(m), Ok(d)) = (
            parts[0].parse::<i32>(),
            parts[1].parse::<u32>(),
            parts[2].parse::<u32>(),
        ) {
            let days = civil_to_epoch_days(y, m, d);
            return sql::Value::Date(days);
        }
    }
    sql::Value::Text(s.to_string())
}

/// Parse a PostgreSQL timestamp string "YYYY-MM-DD HH:MM:SS[.ffffff]" into Value::Timestamp(micros).
fn parse_pg_timestamp(s: &str) -> sql::Value {
    let (date_part, time_part) = match s.split_once(' ') {
        Some(p) => p,
        None => return sql::Value::Text(s.to_string()),
    };
    let date_days = match parse_pg_date(date_part) {
        sql::Value::Date(d) => d,
        _ => return sql::Value::Text(s.to_string()),
    };
    let tp: Vec<&str> = time_part.splitn(2, '.').collect();
    let hms_parts: Vec<&str> = tp[0].splitn(3, ':').collect();
    if hms_parts.len() < 3 {
        return sql::Value::Text(s.to_string());
    }
    let (h, m, sec): (i64, i64, i64) = match (
        hms_parts[0].parse(),
        hms_parts[1].parse(),
        hms_parts[2].parse(),
    ) {
        (Ok(h), Ok(m), Ok(s)) => (h, m, s),
        _ => return sql::Value::Text(s.to_string()),
    };
    let frac_micros: i64 = if tp.len() > 1 {
        let frac = tp[1];
        let padded = format!("{:0<6}", &frac[..6.min(frac.len())]);
        padded.parse::<i64>().unwrap_or(0)
    } else {
        0
    };
    let day_micros: i64 = date_days as i64 * 86_400_000_000;
    let time_micros: i64 = (h * 3600 + m * 60 + sec) * 1_000_000 + frac_micros;
    sql::Value::Timestamp(day_micros + time_micros)
}

/// Convert a civil date (year, month 1-12, day 1-31) to days since Unix epoch (1970-01-01).
fn civil_to_epoch_days(year: i32, month: u32, day: u32) -> i32 {
    // Algorithm: Julian Day Number → Unix epoch days
    let a = (14i32 - month as i32) / 12;
    let y = year + 4800 - a;
    let m = month as i32 + 12 * a - 3;
    let jdn = day as i32 + (153 * m + 2) / 5 + 365 * y + y / 4 - y / 100 + y / 400 - 32045;
    jdn - 2_440_588 // subtract Unix epoch JDN
}

/// Classify a text value that the server sent as TEXT OID — may actually be
/// a PostgreSQL array `{a,b}`, a JSON value, or plain text.
fn classify_text_value(s: &str) -> sql::Value {
    if s.starts_with('[') {
        // JSON array
        return sql::Value::Json(s.to_string());
    }
    if s.starts_with('{') && s.ends_with('}') {
        // Peek past the opening brace to determine JSON object vs PG array.
        // JSON objects have "key": pattern; PG arrays don't have colons outside quotes.
        let inner = s[1..s.len()-1].trim();
        let is_json_object = inner.starts_with('"')
            && inner.contains("\": ")
            || inner.contains("\":\"")
            || (inner.starts_with('"') && inner.chars().skip(1).take_while(|&c| c != '"').count() > 0
                && inner.contains(':'));
        if is_json_object {
            return sql::Value::Json(s.to_string());
        }
        return parse_pg_array(s);
    }
    sql::Value::Text(s.to_string())
}

fn parse_pg_value_typed(s: &str, oid: u32) -> sql::Value {
    match oid {
        16 => match s { "t" | "true" => sql::Value::Bool(true), _ => sql::Value::Bool(false) },
        23 => s.parse::<i32>().map(sql::Value::Int4).unwrap_or_else(|_| sql::Value::Text(s.to_string())),
        20 => s.parse::<i64>().map(sql::Value::Int8).unwrap_or_else(|_| sql::Value::Text(s.to_string())),
        700 | 701 => s.parse::<f64>().map(sql::Value::Float8).unwrap_or_else(|_| sql::Value::Text(s.to_string())),
        // text / varchar: may be a serialised array ({a,b,c}) or JSON if the
        // server sent Type::TEXT for an Array or JSON column.
        25 | 1042 | 1043 | 19 | 142 => classify_text_value(s),
        1082 => parse_pg_date(s),
        1114 | 1184 => parse_pg_timestamp(s),
        1700 => sql::Value::Numeric(s.to_string()),
        2950 => sql::Value::Text(s.to_string()), // uuid
        114 | 3802 => sql::Value::Json(s.to_string()), // json / jsonb
        // Array OIDs — PostgreSQL text[] = 1009, int4[] = 1007, int8[] = 1016, etc.
        // All array OIDs are even numbers > 199; we recognise them by trying to parse
        // the PostgreSQL text array format "{val1,val2,...}".
        _ if is_array_oid(oid) || (s.starts_with('{') && s.ends_with('}')) => {
            parse_pg_array(s)
        }
        _ => {
            // Unknown OID: heuristic fallback (int4 → int8 → float8 → bool → text)
            if let Ok(n) = s.parse::<i32>() {
                return sql::Value::Int4(n);
            }
            if let Ok(n) = s.parse::<i64>() {
                return sql::Value::Int8(n);
            }
            if let Ok(f) = s.parse::<f64>() {
                return sql::Value::Float8(f);
            }
            match s {
                "t" | "true" => return sql::Value::Bool(true),
                "f" | "false" => return sql::Value::Bool(false),
                _ => {}
            }
            sql::Value::Text(s.to_string())
        }
    }
}

/// Returns true for well-known PostgreSQL array type OIDs.
fn is_array_oid(oid: u32) -> bool {
    matches!(oid,
        1000 | 1001 | 1002 | 1003 | 1005 | 1006 | 1007 | 1008 | 1009 |
        1010 | 1011 | 1012 | 1013 | 1014 | 1015 | 1016 | 1017 | 1018 |
        1019 | 1020 | 1021 | 1022 | 1028 | 1182 | 1183 | 1185 | 1187 |
        1231 | 2951 | 3807
    )
}

/// Parse a PostgreSQL text-format array like `{val1,val2,"val 3"}` into `Value::Array`.
fn parse_pg_array(s: &str) -> sql::Value {
    // Strip outer braces
    let inner = if s.starts_with('{') && s.ends_with('}') {
        &s[1..s.len() - 1]
    } else {
        return sql::Value::Text(s.to_string());
    };

    if inner.is_empty() {
        return sql::Value::Array(vec![]);
    }

    let mut elements = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = inner.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
            }
            '\\' if in_quotes => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ',' if !in_quotes => {
                elements.push(sql::Value::Text(current.clone()));
                current.clear();
            }
            _ => current.push(c),
        }
    }
    elements.push(sql::Value::Text(current));

    sql::Value::Array(elements)
}

/// Map a PG error string (from parse_error_response) to the appropriate SqlError variant
/// using the embedded SQLSTATE code.
fn pg_error_to_sql_error(msg: String) -> sql::SqlError {
    // Error format: "SQLSTATE[CODE]: ..." or just the message
    if let Some(rest) = msg.strip_prefix("SQLSTATE[") {
        if let Some(bracket_end) = rest.find(']') {
            let code = &rest[..bracket_end];
            let body = rest[bracket_end + 2..].to_string(); // skip "]: "
            return match code {
                "42P01" => sql::SqlError::TableNotFound(body),
                "42703" => sql::SqlError::ColumnNotFound(body),
                "42601" => sql::SqlError::Parse(body),
                "42P07" => sql::SqlError::Catalog(
                    catalog::error::CatalogError::DuplicateTable(body)
                ),
                "23505" => sql::SqlError::UniqueViolation(body),
                "23000" => sql::SqlError::ConstraintViolation(body),
                "22012" => sql::SqlError::DivisionByZero,
                "22003" => sql::SqlError::NumericOverflow(body),
                "0A000" => sql::SqlError::NotImplemented(body),
                "42804" => sql::SqlError::TypeError(body),
                _ => sql::SqlError::Execution(body),
            };
        }
    }
    sql::SqlError::Execution(msg)
}

/// Extract the raw inner message from a CliError, bypassing the thiserror wrapper
/// so that "SQLSTATE[42P01]: ..." is not obscured by "Network error: ...".
fn cli_error_inner_msg(e: cli::error::CliError) -> String {
    match e {
        cli::error::CliError::Network(s) => s,
        other => other.to_string(),
    }
}

// ── Backend-agnostic helpers ─────────────────────────────────────────────────

/// Execute SQL and panic on error (for test setup)
pub fn exec(b: &Backend, sql: &str) -> sql::ExecutionResult {
    b.execute(sql)
}

/// Execute SQL and expect an error
pub fn exec_err(b: &Backend, sql: &str) -> sql::SqlError {
    b.execute_err(sql)
}

/// Get a single integer value from the first column of the first row
pub fn query_int(b: &Backend, sql: &str) -> i64 {
    let result = b.execute(sql);
    match result.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(sql::Value::Int4(v)) => *v as i64,
        Some(sql::Value::Int8(v)) => *v,
        Some(sql::Value::Float8(f)) => *f as i64,
        Some(sql::Value::Text(s)) => s.parse().unwrap_or_else(|_| panic!("Expected integer, got text: {}", s)),
        other => panic!("Expected integer, got {:?} for SQL: {}", other, sql),
    }
}

/// Count rows matching a query
pub fn count_rows(b: &Backend, sql: &str) -> usize {
    b.execute(sql).rows.len()
}

/// Execute SQL within a named session (supports BEGIN/COMMIT/ROLLBACK/SAVEPOINT state)
pub fn exec_session(b: &Backend, session_id: &str, sql: &str) -> sql::ExecutionResult {
    b.execute_session(session_id, sql)
}

/// Execute SQL in a session, expecting an error
pub fn exec_session_err(b: &Backend, session_id: &str, sql: &str) -> sql::SqlError {
    b.execute_session_err(session_id, sql)
}

// ── Legacy helpers that accept &Arc<QueryEngine> directly ────────────────────
// These are kept for embedded-only test files (transactions.rs, hermitage.rs, etc.)
// that are NOT being refactored to the Backend abstraction.

pub fn exec_engine(engine: &Arc<QueryEngine>, sql: &str) -> sql::ExecutionResult {
    engine.execute(sql).unwrap_or_else(|e| panic!("SQL failed: {}\nSQL: {}", e, sql))
}

pub fn exec_err_engine(engine: &Arc<QueryEngine>, sql: &str) -> sql::SqlError {
    engine.execute(sql).expect_err("Expected SQL error but got success")
}

pub fn query_int_engine(engine: &Arc<QueryEngine>, sql: &str) -> i64 {
    let result = exec_engine(engine, sql);
    match result.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(sql::Value::Int4(v)) => *v as i64,
        Some(sql::Value::Int8(v)) => *v,
        other => panic!("Expected integer, got {:?} for SQL: {}", other, sql),
    }
}

pub fn count_rows_engine(engine: &Arc<QueryEngine>, sql: &str) -> usize {
    exec_engine(engine, sql).rows.len()
}

pub fn exec_session_engine(engine: &Arc<QueryEngine>, session_id: &str, sql: &str) -> sql::ExecutionResult {
    engine.execute_session(session_id, sql)
        .unwrap_or_else(|e| panic!("SQL failed in session '{}': {}\nSQL: {}", session_id, e, sql))
}

pub fn exec_session_err_engine(engine: &Arc<QueryEngine>, session_id: &str, sql: &str) -> sql::SqlError {
    engine.execute_session(session_id, sql)
        .expect_err("Expected SQL error but got success")
}
