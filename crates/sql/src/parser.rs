use sqlparser::ast::Statement;
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser as SqlParserLib;

use crate::error::SqlError;

pub struct Parser;

impl Parser {
    pub fn parse(sql: &str) -> Result<Vec<Statement>, SqlError> {
        let dialect = PostgreSqlDialect {};
        let rewritten = Self::rewrite_trim_syntax(sql);
        SqlParserLib::parse_sql(&dialect, &rewritten).map_err(|e| SqlError::Parse(e.to_string()))
    }

    /// Rewrite TRIM(BOTH FROM expr) and TRIM(BOTH 'chars' FROM expr) into
    /// btrim(expr) or btrim(expr, 'chars') which sqlparser 0.53 can parse.
    /// Also handles LEADING/TRAILING variants.
    fn rewrite_trim_syntax(sql: &str) -> String {
        // Fast path: if no TRIM keyword, skip
        let upper = sql.to_uppercase();
        if !upper.contains("TRIM(") && !upper.contains("TRIM (") {
            return sql.to_string();
        }

        let mut result = String::with_capacity(sql.len());
        let bytes = sql.as_bytes();
        let len = bytes.len();
        let mut i = 0;

        while i < len {
            // Look for TRIM( (case-insensitive)
            if i + 5 <= len
                && bytes[i..i + 4].eq_ignore_ascii_case(b"TRIM")
            {
                // Check for optional whitespace then '('
                let mut j = i + 4;
                while j < len && bytes[j] == b' ' { j += 1; }
                if j < len && bytes[j] == b'(' {
                    // Try to parse TRIM(BOTH/LEADING/TRAILING ...)
                    let inner_start = j + 1;
                    // Skip whitespace
                    let mut k = inner_start;
                    while k < len && bytes[k] == b' ' { k += 1; }

                    // Check for BOTH/LEADING/TRAILING keyword
                    let func_name = if sql[k..].to_uppercase().starts_with("BOTH") && (k + 4 >= len || !bytes[k+4].is_ascii_alphabetic()) {
                        k += 4;
                        "btrim"
                    } else if sql[k..].to_uppercase().starts_with("LEADING") && (k + 7 >= len || !bytes[k+7].is_ascii_alphabetic()) {
                        k += 7;
                        "ltrim"
                    } else if sql[k..].to_uppercase().starts_with("TRAILING") && (k + 8 >= len || !bytes[k+8].is_ascii_alphabetic()) {
                        k += 8;
                        "rtrim"
                    } else {
                        // Not a BOTH/LEADING/TRAILING pattern, pass through
                        result.push_str(&sql[i..j + 1]);
                        i = j + 1;
                        continue;
                    };

                    // Skip whitespace after BOTH/LEADING/TRAILING
                    while k < len && bytes[k] == b' ' { k += 1; }

                    // Optionally a quoted characters argument before FROM
                    let chars_arg: Option<String>;
                    if k < len && (bytes[k] == b'\'' || bytes[k] == b'"') {
                        // Collect the quoted string
                        let quote = bytes[k] as char;
                        let mut end = k + 1;
                        while end < len && bytes[end] as char != quote { end += 1; }
                        end += 1; // include closing quote
                        chars_arg = Some(sql[k..end].to_string());
                        k = end;
                        // Skip whitespace
                        while k < len && bytes[k] == b' ' { k += 1; }
                    } else {
                        chars_arg = None;
                    }

                    // Expect FROM keyword
                    if k + 4 <= len && sql[k..k+4].to_uppercase() == "FROM" && (k + 4 >= len || !bytes[k+4].is_ascii_alphabetic()) {
                        k += 4;
                        // Skip whitespace
                        while k < len && bytes[k] == b' ' { k += 1; }
                        // Collect everything up to matching ')'
                        let expr_start = k;
                        let mut depth = 1i32;
                        let mut m = k;
                        let mut in_quote = false;
                        let mut quote_char = b' ';
                        while m < len {
                            let b = bytes[m];
                            if in_quote {
                                if b == quote_char { in_quote = false; }
                            } else if b == b'\'' || b == b'"' {
                                in_quote = true;
                                quote_char = b;
                            } else if b == b'(' {
                                depth += 1;
                            } else if b == b')' {
                                depth -= 1;
                                if depth == 0 { break; }
                            }
                            m += 1;
                        }
                        let inner_expr = &sql[expr_start..m];
                        // Rewrite as func_name(inner_expr) or func_name(inner_expr, chars)
                        result.push_str(func_name);
                        result.push('(');
                        result.push_str(inner_expr);
                        if let Some(chars) = chars_arg {
                            result.push_str(", ");
                            result.push_str(&chars);
                        }
                        result.push(')');
                        i = m + 1; // skip the closing ')'
                        continue;
                    }
                    // FROM not found; fall through to normal output
                    result.push_str(&sql[i..j + 1]);
                    i = j + 1;
                    continue;
                }
            }
            result.push(bytes[i] as char);
            i += 1;
        }
        result
    }

    /// Parse SQL but also handle VACUUM, LISTEN, UNLISTEN, NOTIFY statements which
    /// sqlparser doesn't support natively.
    /// Returns either parsed statements or a special sentinel variant.
    pub fn parse_with_vacuum(sql: &str) -> Result<ParseResult, SqlError> {
        let trimmed = sql.trim();
        let upper = trimmed.to_uppercase();

        // Check for VACUUM statement
        if upper.starts_with("VACUUM") {
            let rest = trimmed[6..].trim(); // after "VACUUM"
            let (analyze, rest2) = if rest.to_uppercase().starts_with("ANALYZE") {
                (true, rest[7..].trim())
            } else {
                (false, rest)
            };
            let table_name = if rest2.is_empty() {
                None
            } else {
                // strip trailing semicolon
                let t = rest2.trim_end_matches(';').trim();
                if t.is_empty() { None } else { Some(t.to_string()) }
            };
            return Ok(ParseResult::Vacuum { table_name, analyze });
        }

        // Check for ANALYZE statement (standalone) — map to VACUUM ANALYZE
        if upper == "ANALYZE" || upper.starts_with("ANALYZE ") {
            let rest = trimmed[7..].trim(); // after "ANALYZE"
            let table_name = if rest.is_empty() {
                None
            } else {
                let t = rest.trim_end_matches(';').trim();
                if t.is_empty() { None } else { Some(t.to_string()) }
            };
            return Ok(ParseResult::Vacuum { table_name, analyze: true });
        }

        // Check for LISTEN statement
        if upper.starts_with("LISTEN ") || upper == "LISTEN" {
            let channel = trimmed[6..].trim().trim_end_matches(';').trim().to_string();
            return Ok(ParseResult::Listen { channel });
        }

        // Check for UNLISTEN statement
        if upper.starts_with("UNLISTEN ") || upper == "UNLISTEN" {
            let rest = trimmed[8..].trim().trim_end_matches(';').trim();
            let channel = if rest == "*" || rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            };
            return Ok(ParseResult::Unlisten { channel });
        }

        // Check for NOTIFY statement: NOTIFY channel [, 'payload']
        if upper.starts_with("NOTIFY ") {
            let rest = trimmed[7..].trim().trim_end_matches(';');
            let parts: Vec<&str> = rest.splitn(2, ',').collect();
            let channel = parts[0].trim().to_string();
            let payload = parts
                .get(1)
                .map(|s| s.trim().trim_matches('\'').to_string());
            return Ok(ParseResult::Notify { channel, payload });
        }

        // Check for DROP FUNCTION [IF EXISTS] name — sqlparser 0.53 doesn't have ObjectType::Function
        if upper.starts_with("DROP FUNCTION") {
            let rest = trimmed[13..].trim();
            let (if_exists, name_part) = if rest.to_uppercase().starts_with("IF EXISTS") {
                (true, rest[9..].trim().trim_end_matches(';').trim())
            } else {
                (false, rest.trim_end_matches(';').trim())
            };
            // Strip any parentheses from the function name (e.g. "name()" or "name(INT)")
            let name = if let Some(pos) = name_part.find('(') {
                name_part[..pos].trim().to_string()
            } else {
                name_part.to_string()
            };
            return Ok(ParseResult::DropFunction { name, if_exists });
        }

        let stmts = Self::parse(sql)?;
        Ok(ParseResult::Statements(stmts))
    }
}

#[cfg(test)]
mod tests {
    use super::Parser;

    #[test]
    fn test_trim_rewrite_both_no_chars() {
        let sql = "SELECT TRIM(BOTH FROM '  hello  ')";
        let rewritten = Parser::rewrite_trim_syntax(sql);
        assert_eq!(rewritten, "SELECT btrim('  hello  ')");
    }

    #[test]
    fn test_trim_rewrite_both_with_chars() {
        let sql = "SELECT TRIM(BOTH ' ' FROM '  hello  ')";
        let rewritten = Parser::rewrite_trim_syntax(sql);
        assert_eq!(rewritten, "SELECT btrim('  hello  ', ' ')");
    }

    #[test]
    fn test_trim_rewrite_leading() {
        let sql = "SELECT TRIM(LEADING FROM '  hello  ')";
        let rewritten = Parser::rewrite_trim_syntax(sql);
        assert_eq!(rewritten, "SELECT ltrim('  hello  ')");
    }

    #[test]
    fn test_trim_rewrite_trailing() {
        let sql = "SELECT TRIM(TRAILING FROM '  hello  ')";
        let rewritten = Parser::rewrite_trim_syntax(sql);
        assert_eq!(rewritten, "SELECT rtrim('  hello  ')");
    }

    #[test]
    fn test_trim_both_parses_after_rewrite() {
        let sql = "SELECT TRIM(BOTH FROM '  hello  ')";
        let result = Parser::parse(sql);
        assert!(result.is_ok(), "TRIM(BOTH FROM ...) should parse OK after rewrite, got: {:?}", result);
    }
}

pub enum ParseResult {
    Statements(Vec<Statement>),
    Vacuum {
        table_name: Option<String>,
        analyze: bool,
    },
    Listen {
        channel: String,
    },
    Unlisten {
        channel: Option<String>,
    },
    Notify {
        channel: String,
        payload: Option<String>,
    },
    DropFunction {
        name: String,
        if_exists: bool,
    },
}
