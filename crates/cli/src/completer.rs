use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};

const SQL_KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE",
    "TABLE",
    "DROP",
    "ALTER",
    "INDEX",
    "ON",
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "OUTER",
    "AND",
    "OR",
    "NOT",
    "NULL",
    "IS",
    "IN",
    "LIKE",
    "ORDER",
    "BY",
    "GROUP",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "TRANSACTION",
    "DISTINCT",
    "ALL",
    "AS",
    "BETWEEN",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "INT",
    "INTEGER",
    "TEXT",
    "BOOLEAN",
    "FLOAT",
    "BIGINT",
    "VARCHAR",
    "PRIMARY",
    "KEY",
    "FOREIGN",
    "REFERENCES",
    "UNIQUE",
    "DEFAULT",
    "CONSTRAINT",
    "NOT NULL",
    "EXPLAIN",
    "ANALYZE",
];

pub struct SqlCompleter {
    pub table_names: Vec<String>,
}

impl Completer for SqlCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        // Find the start of the current word
        let start = line[..pos]
            .rfind(|c: char| c.is_whitespace() || c == '(' || c == ',')
            .map(|i| i + 1)
            .unwrap_or(0);
        let word = &line[start..pos];

        if word.is_empty() {
            return Ok((pos, vec![]));
        }

        let word_upper = word.to_uppercase();
        let mut candidates: Vec<Pair> = Vec::new();

        // Match SQL keywords
        for kw in SQL_KEYWORDS {
            if kw.starts_with(word_upper.as_str()) {
                candidates.push(Pair {
                    display: kw.to_string(),
                    replacement: kw.to_string(),
                });
            }
        }

        // Match table names (case-insensitive)
        for table in &self.table_names {
            if table.to_uppercase().starts_with(word_upper.as_str()) {
                candidates.push(Pair {
                    display: table.clone(),
                    replacement: table.clone(),
                });
            }
        }

        Ok((start, candidates))
    }
}

impl Hinter for SqlCompleter {
    type Hint = String;
}

impl Highlighter for SqlCompleter {}
impl Validator for SqlCompleter {}
impl Helper for SqlCompleter {}
