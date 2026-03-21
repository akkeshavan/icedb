use crate::error::SqlError;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int4(i32),
    Int8(i64),
    Float8(f64),
    Text(String),
    Bytes(Vec<u8>),
    /// Days since Unix epoch (1970-01-01)
    Date(i32),
    /// Microseconds since Unix epoch
    Timestamp(i64),
    /// Exact decimal as string
    Numeric(String),
    /// UUID string
    Uuid(String),
}

impl Value {
    pub fn data_type(&self) -> Option<catalog::DataType> {
        match self {
            Value::Null => None,
            Value::Bool(_) => Some(catalog::DataType::Boolean),
            Value::Int4(_) => Some(catalog::DataType::Int4),
            Value::Int8(_) => Some(catalog::DataType::Int8),
            Value::Float8(_) => Some(catalog::DataType::Float8),
            Value::Text(_) => Some(catalog::DataType::Text),
            Value::Bytes(_) => Some(catalog::DataType::Bytea),
            Value::Date(_) => Some(catalog::DataType::Date),
            Value::Timestamp(_) => Some(catalog::DataType::Timestamp),
            Value::Numeric(_) => Some(catalog::DataType::Numeric),
            Value::Uuid(_) => Some(catalog::DataType::Uuid),
        }
    }

    /// Serialize for storage. Null = empty bytes (tracked by null bitmap).
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Value::Null => vec![],
            Value::Bool(b) => vec![if *b { 1u8 } else { 0u8 }],
            Value::Int4(v) => v.to_le_bytes().to_vec(),
            Value::Int8(v) => v.to_le_bytes().to_vec(),
            Value::Float8(v) => v.to_le_bytes().to_vec(),
            Value::Text(s) | Value::Numeric(s) | Value::Uuid(s) => {
                let bytes = s.as_bytes();
                let len = bytes.len() as u32;
                let mut out = len.to_le_bytes().to_vec();
                out.extend_from_slice(bytes);
                out
            }
            Value::Bytes(b) => {
                let len = b.len() as u32;
                let mut out = len.to_le_bytes().to_vec();
                out.extend_from_slice(b);
                out
            }
            Value::Date(v) => v.to_le_bytes().to_vec(),
            Value::Timestamp(v) => v.to_le_bytes().to_vec(),
        }
    }

    pub fn from_bytes(bytes: &[u8], dtype: &catalog::DataType) -> Result<Value, SqlError> {
        match dtype {
            catalog::DataType::Boolean => {
                if bytes.is_empty() {
                    return Err(SqlError::TypeError("not enough bytes for bool".to_string()));
                }
                Ok(Value::Bool(bytes[0] != 0))
            }
            catalog::DataType::Int4 => {
                if bytes.len() < 4 {
                    return Err(SqlError::TypeError("not enough bytes for int4".to_string()));
                }
                let v = i32::from_le_bytes(bytes[0..4].try_into().unwrap());
                Ok(Value::Int4(v))
            }
            catalog::DataType::Int8 => {
                if bytes.len() < 8 {
                    return Err(SqlError::TypeError("not enough bytes for int8".to_string()));
                }
                let v = i64::from_le_bytes(bytes[0..8].try_into().unwrap());
                Ok(Value::Int8(v))
            }
            catalog::DataType::Float8 => {
                if bytes.len() < 8 {
                    return Err(SqlError::TypeError(
                        "not enough bytes for float8".to_string(),
                    ));
                }
                let v = f64::from_le_bytes(bytes[0..8].try_into().unwrap());
                Ok(Value::Float8(v))
            }
            catalog::DataType::Text | catalog::DataType::VarChar(_) => {
                if bytes.len() < 4 {
                    return Err(SqlError::TypeError("not enough bytes for text".to_string()));
                }
                let len = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
                if bytes.len() < 4 + len {
                    return Err(SqlError::TypeError("text data truncated".to_string()));
                }
                let s = std::str::from_utf8(&bytes[4..4 + len])
                    .map_err(|e| SqlError::TypeError(format!("invalid UTF-8: {e}")))?;
                Ok(Value::Text(s.to_string()))
            }
            catalog::DataType::Bytea => {
                if bytes.len() < 4 {
                    return Err(SqlError::TypeError(
                        "not enough bytes for bytea".to_string(),
                    ));
                }
                let len = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
                if bytes.len() < 4 + len {
                    return Err(SqlError::TypeError("bytea data truncated".to_string()));
                }
                Ok(Value::Bytes(bytes[4..4 + len].to_vec()))
            }
            catalog::DataType::Date => {
                if bytes.len() < 4 {
                    return Err(SqlError::TypeError("not enough bytes for date".to_string()));
                }
                let v = i32::from_le_bytes(bytes[0..4].try_into().unwrap());
                Ok(Value::Date(v))
            }
            catalog::DataType::Timestamp | catalog::DataType::TimestampTz => {
                if bytes.len() < 8 {
                    return Err(SqlError::TypeError("not enough bytes for timestamp".to_string()));
                }
                let v = i64::from_le_bytes(bytes[0..8].try_into().unwrap());
                Ok(Value::Timestamp(v))
            }
            catalog::DataType::Numeric => {
                if bytes.len() < 4 {
                    return Err(SqlError::TypeError("not enough bytes for numeric".to_string()));
                }
                let len = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
                if bytes.len() < 4 + len {
                    return Err(SqlError::TypeError("numeric data truncated".to_string()));
                }
                let s = std::str::from_utf8(&bytes[4..4 + len])
                    .map_err(|e| SqlError::TypeError(format!("invalid UTF-8 in numeric: {e}")))?;
                Ok(Value::Numeric(s.to_string()))
            }
            catalog::DataType::Uuid => {
                if bytes.len() < 4 {
                    return Err(SqlError::TypeError("not enough bytes for uuid".to_string()));
                }
                let len = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
                if bytes.len() < 4 + len {
                    return Err(SqlError::TypeError("uuid data truncated".to_string()));
                }
                let s = std::str::from_utf8(&bytes[4..4 + len])
                    .map_err(|e| SqlError::TypeError(format!("invalid UTF-8 in uuid: {e}")))?;
                Ok(Value::Uuid(s.to_string()))
            }
        }
    }

    pub fn cast_to(&self, dtype: &catalog::DataType) -> Result<Value, SqlError> {
        match (self, dtype) {
            // Same type
            (Value::Bool(_), catalog::DataType::Boolean) => Ok(self.clone()),
            (Value::Int4(_), catalog::DataType::Int4) => Ok(self.clone()),
            (Value::Int8(_), catalog::DataType::Int8) => Ok(self.clone()),
            (Value::Float8(_), catalog::DataType::Float8) => Ok(self.clone()),
            (Value::Text(_), catalog::DataType::Text) => Ok(self.clone()),
            (Value::Text(_), catalog::DataType::VarChar(_)) => Ok(self.clone()),
            (Value::Bytes(_), catalog::DataType::Bytea) => Ok(self.clone()),
            (Value::Date(_), catalog::DataType::Date) => Ok(self.clone()),
            (Value::Timestamp(_), catalog::DataType::Timestamp) => Ok(self.clone()),
            (Value::Timestamp(_), catalog::DataType::TimestampTz) => Ok(self.clone()),
            (Value::Numeric(_), catalog::DataType::Numeric) => Ok(self.clone()),
            (Value::Uuid(_), catalog::DataType::Uuid) => Ok(self.clone()),
            (Value::Null, _) => Ok(Value::Null),

            // Int4 → Int8
            (Value::Int4(v), catalog::DataType::Int8) => Ok(Value::Int8(*v as i64)),
            // Int8 → Int4
            (Value::Int8(v), catalog::DataType::Int4) => Ok(Value::Int4(*v as i32)),
            // Int4 → Float8
            (Value::Int4(v), catalog::DataType::Float8) => Ok(Value::Float8(*v as f64)),
            // Int8 → Float8
            (Value::Int8(v), catalog::DataType::Float8) => Ok(Value::Float8(*v as f64)),
            // Float8 → Int4
            (Value::Float8(v), catalog::DataType::Int4) => Ok(Value::Int4(*v as i32)),
            // Float8 → Int8
            (Value::Float8(v), catalog::DataType::Int8) => Ok(Value::Int8(*v as i64)),
            // Anything → Text
            (v, catalog::DataType::Text) => Ok(Value::Text(v.to_string())),
            (v, catalog::DataType::VarChar(_)) => Ok(Value::Text(v.to_string())),

            // Bool → Int4
            (Value::Bool(b), catalog::DataType::Int4) => Ok(Value::Int4(if *b { 1 } else { 0 })),

            // Text → Date
            (Value::Text(s), catalog::DataType::Date) => {
                parse_date_str(s).map(Value::Date).ok_or_else(|| {
                    SqlError::TypeError(format!("cannot parse date: '{s}'"))
                })
            }
            // Text → Timestamp / TimestampTz
            (Value::Text(s), catalog::DataType::Timestamp)
            | (Value::Text(s), catalog::DataType::TimestampTz) => {
                parse_timestamp_str(s).map(Value::Timestamp).ok_or_else(|| {
                    SqlError::TypeError(format!("cannot parse timestamp: '{s}'"))
                })
            }
            // Text → Numeric
            (Value::Text(s), catalog::DataType::Numeric) => {
                // Validate it's a valid number string
                if s.trim().parse::<f64>().is_ok() || is_valid_numeric(s.trim()) {
                    Ok(Value::Numeric(s.trim().to_string()))
                } else {
                    Err(SqlError::TypeError(format!("invalid numeric literal: '{s}'")))
                }
            }
            // Text → Uuid
            (Value::Text(s), catalog::DataType::Uuid) => {
                Ok(Value::Uuid(s.clone()))
            }
            // Integer/Float → Numeric
            (Value::Int4(v), catalog::DataType::Numeric) => Ok(Value::Numeric(v.to_string())),
            (Value::Int8(v), catalog::DataType::Numeric) => Ok(Value::Numeric(v.to_string())),
            (Value::Float8(v), catalog::DataType::Numeric) => Ok(Value::Numeric(v.to_string())),
            // Numeric → Float8
            (Value::Numeric(s), catalog::DataType::Float8) => {
                s.parse::<f64>().map(Value::Float8).map_err(|_| {
                    SqlError::TypeError(format!("cannot cast numeric '{s}' to float"))
                })
            }
            // Numeric → Int4
            (Value::Numeric(s), catalog::DataType::Int4) => {
                s.parse::<f64>().map(|f| Value::Int4(f as i32)).map_err(|_| {
                    SqlError::TypeError(format!("cannot cast numeric '{s}' to integer"))
                })
            }
            // Numeric → Int8
            (Value::Numeric(s), catalog::DataType::Int8) => {
                s.parse::<f64>().map(|f| Value::Int8(f as i64)).map_err(|_| {
                    SqlError::TypeError(format!("cannot cast numeric '{s}' to bigint"))
                })
            }

            // Text → Int4
            (Value::Text(s), catalog::DataType::Int4) => {
                s.trim().parse::<i32>().map(Value::Int4).map_err(|_| {
                    SqlError::TypeError(format!("cannot cast text '{s}' to integer"))
                })
            }
            // Text → Int8
            (Value::Text(s), catalog::DataType::Int8) => {
                s.trim().parse::<i64>().map(Value::Int8).map_err(|_| {
                    SqlError::TypeError(format!("cannot cast text '{s}' to bigint"))
                })
            }
            // Text → Float8 (including 'Infinity', '-Infinity', 'NaN')
            (Value::Text(s), catalog::DataType::Float8) => {
                let trimmed = s.trim();
                let v = match trimmed.to_ascii_lowercase().as_str() {
                    "infinity" | "+infinity" => f64::INFINITY,
                    "-infinity" => f64::NEG_INFINITY,
                    "nan" => f64::NAN,
                    _ => trimmed.parse::<f64>().map_err(|_| {
                        SqlError::TypeError(format!("cannot cast text '{s}' to float8"))
                    })?,
                };
                Ok(Value::Float8(v))
            }
            // Text → Boolean ('true','false','t','f','yes','no','on','off','1','0')
            (Value::Text(s), catalog::DataType::Boolean) => {
                match s.trim().to_ascii_lowercase().as_str() {
                    "true" | "t" | "yes" | "on" | "1" => Ok(Value::Bool(true)),
                    "false" | "f" | "no" | "off" | "0" => Ok(Value::Bool(false)),
                    _ => Err(SqlError::TypeError(format!("cannot cast text '{s}' to boolean"))),
                }
            }

            _ => Err(SqlError::TypeError(format!(
                "cannot cast {:?} to {:?}",
                self.data_type(),
                dtype
            ))),
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Bool(b) => write!(f, "{}", if *b { "t" } else { "f" }),
            Value::Int4(v) => write!(f, "{v}"),
            Value::Int8(v) => write!(f, "{v}"),
            Value::Float8(v) => write!(f, "{v}"),
            Value::Text(s) => write!(f, "{s}"),
            Value::Bytes(b) => write!(f, "\\x{}", hex_encode(b)),
            Value::Date(d) => write!(f, "{}", format_date(*d)),
            Value::Timestamp(ts) => write!(f, "{}", format_timestamp(*ts)),
            Value::Numeric(s) => write!(f, "{s}"),
            Value::Uuid(s) => write!(f, "{s}"),
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Value::Null, Value::Null) => Some(std::cmp::Ordering::Equal),
            (Value::Null, _) => Some(std::cmp::Ordering::Less),
            (_, Value::Null) => Some(std::cmp::Ordering::Greater),
            (Value::Bool(a), Value::Bool(b)) => a.partial_cmp(b),
            (Value::Int4(a), Value::Int4(b)) => a.partial_cmp(b),
            (Value::Int8(a), Value::Int8(b)) => a.partial_cmp(b),
            (Value::Float8(a), Value::Float8(b)) => a.partial_cmp(b),
            (Value::Int4(a), Value::Int8(b)) => (*a as i64).partial_cmp(b),
            (Value::Int8(a), Value::Int4(b)) => a.partial_cmp(&(*b as i64)),
            (Value::Int4(a), Value::Float8(b)) => (*a as f64).partial_cmp(b),
            (Value::Int8(a), Value::Float8(b)) => (*a as f64).partial_cmp(b),
            (Value::Float8(a), Value::Int4(b)) => a.partial_cmp(&(*b as f64)),
            (Value::Float8(a), Value::Int8(b)) => a.partial_cmp(&(*b as f64)),
            (Value::Text(a), Value::Text(b)) => a.partial_cmp(b),
            (Value::Bytes(a), Value::Bytes(b)) => a.partial_cmp(b),
            (Value::Date(a), Value::Date(b)) => a.partial_cmp(b),
            (Value::Timestamp(a), Value::Timestamp(b)) => a.partial_cmp(b),
            (Value::Numeric(a), Value::Numeric(b)) => {
                // Compare numerically if possible, else lexicographically
                match (a.parse::<f64>(), b.parse::<f64>()) {
                    (Ok(fa), Ok(fb)) => fa.partial_cmp(&fb),
                    _ => a.partial_cmp(b),
                }
            }
            (Value::Uuid(a), Value::Uuid(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

// ── Date/Timestamp helpers ────────────────────────────────────────────────────

/// Parse "YYYY-MM-DD" → days since Unix epoch (1970-01-01).
pub fn parse_date_str(s: &str) -> Option<i32> {
    let s = s.trim();
    let parts: Vec<&str> = s.splitn(3, '-').collect();
    if parts.len() != 3 {
        return None;
    }
    let year: i32 = parts[0].parse().ok()?;
    let month: i32 = parts[1].parse().ok()?;
    let day: i32 = parts[2].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Compute days since epoch using civil calendar algorithm
    Some(days_from_ymd(year, month, day))
}

/// Parse "YYYY-MM-DD HH:MM:SS[.ffffff]" or "YYYY-MM-DDTHH:MM:SS" → microseconds since epoch.
pub fn parse_timestamp_str(s: &str) -> Option<i64> {
    let s = s.trim();
    // Support both space and T as separator
    let (date_part, time_part) = if let Some(i) = s.find(' ').or_else(|| s.find('T')) {
        (&s[..i], &s[i+1..])
    } else {
        // Date-only timestamp
        (s, "00:00:00")
    };
    let days = parse_date_str(date_part)?;
    let micros_in_day = parse_time_str(time_part)?;
    Some(days as i64 * 86_400_000_000 + micros_in_day)
}

fn parse_time_str(s: &str) -> Option<i64> {
    // "HH:MM:SS" or "HH:MM:SS.ffffff"
    let (hms, frac_micros) = if let Some(dot) = s.find('.') {
        let frac_str = &s[dot+1..];
        // Pad/trim to 6 digits
        let padded = format!("{:0<6}", &frac_str[..frac_str.len().min(6)]);
        let frac: i64 = padded.parse().ok()?;
        (&s[..dot], frac)
    } else {
        (s, 0)
    };
    let parts: Vec<&str> = hms.splitn(3, ':').collect();
    if parts.len() != 3 {
        return None;
    }
    let h: i64 = parts[0].parse().ok()?;
    let m: i64 = parts[1].parse().ok()?;
    let s_val: i64 = parts[2].parse().ok()?;
    Some((h * 3600 + m * 60 + s_val) * 1_000_000 + frac_micros)
}

/// Format days-since-epoch as "YYYY-MM-DD".
pub fn format_date(days: i32) -> String {
    let (y, m, d) = ymd_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Format microseconds-since-epoch as "YYYY-MM-DD HH:MM:SS".
pub fn format_timestamp(micros: i64) -> String {
    let total_secs = micros / 1_000_000;
    let rem_micros = micros.rem_euclid(1_000_000);
    let days = (total_secs / 86400) as i32;
    let time_secs = total_secs.rem_euclid(86400);
    let (y, mo, d) = ymd_from_days(days);
    let h = time_secs / 3600;
    let mi = (time_secs % 3600) / 60;
    let s = time_secs % 60;
    if rem_micros == 0 {
        format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
    } else {
        format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}.{rem_micros:06}")
    }
}

/// Simple civil calendar: days since 1970-01-01.
fn days_from_ymd(y: i32, m: i32, d: i32) -> i32 {
    // Using the formula from http://howardhinnant.github.io/date_algorithms.html
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// Convert days since epoch to (year, month, day).
fn ymd_from_days(z: i32) -> (i32, i32, i32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn is_valid_numeric(s: &str) -> bool {
    // Simple check: optional sign, digits, optional decimal point
    let s = s.trim_start_matches(['+', '-']);
    if s.is_empty() { return false; }
    let mut has_dot = false;
    for c in s.chars() {
        if c == '.' {
            if has_dot { return false; }
            has_dot = true;
        } else if !c.is_ascii_digit() {
            return false;
        }
    }
    true
}

/// Truncate a timestamp (microseconds since epoch) to a date/time boundary.
pub fn date_trunc(micros: i64, field: &str) -> i64 {
    let total_secs = micros / 1_000_000;
    let days = total_secs / 86400;
    let (y, mo, _d) = ymd_from_days(days as i32);
    match field {
        "microseconds" => micros,
        "milliseconds" => (micros / 1000) * 1000,
        "second" | "seconds" => (micros / 1_000_000) * 1_000_000,
        "minute" | "minutes" => {
            let secs = (total_secs / 60) * 60;
            secs * 1_000_000
        }
        "hour" | "hours" => {
            let secs = (total_secs / 3600) * 3600;
            secs * 1_000_000
        }
        "day" | "days" => days * 86_400 * 1_000_000,
        "week" | "weeks" => {
            // ISO week: truncate to Monday
            // day of week: 1=Mon, 7=Sun
            let dow = ((days + 3) % 7 + 7) % 7; // 0=Mon, 6=Sun
            (days - dow) * 86_400 * 1_000_000
        }
        "month" | "months" => {
            let month_start = days_from_ymd(y, mo, 1) as i64;
            month_start * 86_400 * 1_000_000
        }
        "quarter" => {
            let q_month = ((mo - 1) / 3) * 3 + 1;
            let quarter_start = days_from_ymd(y, q_month, 1) as i64;
            quarter_start * 86_400 * 1_000_000
        }
        "year" | "years" => {
            let year_start = days_from_ymd(y, 1, 1) as i64;
            year_start * 86_400 * 1_000_000
        }
        "decade" => {
            let decade_year = (y / 10) * 10;
            let decade_start = days_from_ymd(decade_year, 1, 1) as i64;
            decade_start * 86_400 * 1_000_000
        }
        "century" => {
            let century_year = ((y - 1) / 100) * 100 + 1;
            let century_start = days_from_ymd(century_year, 1, 1) as i64;
            century_start * 86_400 * 1_000_000
        }
        _ => micros,
    }
}

/// Extract a numeric field from a Date or Timestamp value.
pub fn extract_field(field: &str, val: &Value) -> Option<f64> {
    let micros = match val {
        Value::Timestamp(t) => *t,
        Value::Date(d) => *d as i64 * 86_400_000_000,
        _ => return None,
    };
    let total_secs = micros / 1_000_000;
    let days = (total_secs / 86400) as i32;
    let (y, mo, d) = ymd_from_days(days);
    let time_secs = total_secs.rem_euclid(86400);
    let h = time_secs / 3600;
    let mi = (time_secs % 3600) / 60;
    let s = time_secs % 60;
    let us = micros.rem_euclid(1_000_000);
    match field {
        "year" | "years" => Some(y as f64),
        "month" | "months" => Some(mo as f64),
        "day" | "days" => Some(d as f64),
        "hour" | "hours" => Some(h as f64),
        "minute" | "minutes" => Some(mi as f64),
        "second" | "seconds" => Some(s as f64 + us as f64 / 1_000_000.0),
        "microseconds" => Some(s as f64 * 1_000_000.0 + us as f64),
        "milliseconds" => Some(s as f64 * 1000.0 + us as f64 / 1000.0),
        "epoch" => Some(micros as f64 / 1_000_000.0),
        "dow" => Some(((days + 4).rem_euclid(7)) as f64), // 0=Sun
        "doy" => {
            let year_start = days_from_ymd(y, 1, 1);
            Some((days - year_start + 1) as f64)
        }
        "quarter" => Some(((mo - 1) / 3 + 1) as f64),
        "week" => {
            // ISO week number (simplified)
            let year_start = days_from_ymd(y, 1, 1);
            Some(((days - year_start) / 7 + 1) as f64)
        }
        "decade" => Some((y / 10) as f64),
        "century" => Some(((y + 99) / 100) as f64),
        "millennium" => Some(((y + 999) / 1000) as f64),
        _ => None,
    }
}
