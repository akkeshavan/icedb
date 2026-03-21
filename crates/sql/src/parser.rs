use sqlparser::ast::Statement;
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser as SqlParserLib;

use crate::error::SqlError;

pub struct Parser;

impl Parser {
    pub fn parse(sql: &str) -> Result<Vec<Statement>, SqlError> {
        let dialect = PostgreSqlDialect {};
        SqlParserLib::parse_sql(&dialect, sql).map_err(|e| SqlError::Parse(e.to_string()))
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
