use crate::completer::SqlCompleter;
use crate::config::Config;
use crate::error::CliError;
use crate::formatter::{format_command_result, format_table};
use crate::meta::{execute_meta_command, parse_meta_command};
use catalog::manager::CatalogManager;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use sql::db_manager::DatabaseManager;
use sql::engine::QueryEngine;
use std::sync::Arc;

pub struct Repl {
    db_manager: Arc<DatabaseManager>,
    engine: Arc<QueryEngine>,
    catalog: Arc<CatalogManager>,
    config: Config,
    current_db: String,
}

impl Repl {
    pub fn new(db_manager: Arc<DatabaseManager>, engine: Arc<QueryEngine>, catalog: Arc<CatalogManager>, config: Config) -> Self {
        let current_db = config.dbname.clone();
        let mut repl = Self {
            db_manager,
            engine,
            catalog,
            config,
            current_db,
        };
        // If --dbname names a database other than "icedb", switch to it now so
        // that the initial engine/catalog point at the correct database.
        if repl.current_db != "icedb" {
            if let Ok(new_engine) = repl.db_manager.get_or_open(&repl.current_db) {
                repl.catalog = Arc::clone(&new_engine.catalog);
                repl.engine = new_engine;
            }
        }
        repl
    }

    pub fn run(&mut self) -> Result<(), CliError> {
        let mut rl = Editor::<SqlCompleter, rustyline::history::DefaultHistory>::new()
            .map_err(|e| CliError::Readline(e.to_string()))?;

        // Set up completer with known table names
        let table_names = self.get_table_names();
        rl.set_helper(Some(SqlCompleter { table_names }));

        // Load history
        if let Some(ref history_file) = self.config.history_file {
            let _ = rl.load_history(history_file);
        }

        println!("isql (icedb {})", env!("CARGO_PKG_VERSION"));
        println!("Type \"help\" for help, \"\\q\" to quit.\n");

        let mut timing = false;
        let mut expanded = false;
        let mut sql_buffer = String::new();

        loop {
            let prompt = if sql_buffer.is_empty() {
                format!("{}=# ", self.current_db)
            } else {
                format!("{}-# ", self.current_db)
            };

            let line = match rl.readline(&prompt) {
                Ok(line) => line,
                Err(ReadlineError::Interrupted) => {
                    sql_buffer.clear();
                    println!("^C");
                    continue;
                }
                Err(ReadlineError::Eof) => {
                    println!("\\q");
                    break;
                }
                Err(e) => {
                    eprintln!("Readline error: {}", e);
                    break;
                }
            };

            let trimmed = line.trim();

            if trimmed.is_empty() {
                continue;
            }

            // Skip pure SQL comment lines (same behaviour as psql).
            // A line that is entirely a comment contributes nothing to the
            // statement being built; accumulating it would combine it with
            // the next token (e.g. "-- remark\nBEGIN") which defeats the
            // keyword matching in execute_session.
            if trimmed.starts_with("--") {
                continue;
            }

            // Meta-commands
            if trimmed.starts_with('\\') {
                let _ = rl.add_history_entry(trimmed);
                match parse_meta_command(trimmed) {
                    Ok(cmd) => {
                        // Warn when there is pending SQL in the buffer so the
                        // user knows their statement hasn't executed yet.
                        if !sql_buffer.is_empty() && !matches!(cmd, crate::meta::MetaCommand::ResetBuffer) {
                            eprintln!("WARNING: query buffer is not empty (forgot a semicolon?).");
                            eprintln!("         Use \\r to discard the pending input.");
                        }
                        match execute_meta_command(
                            cmd,
                            &self.catalog,
                            &self.engine,
                            &mut timing,
                            &mut expanded,
                            self.db_manager.data_dir(),
                        ) {
                            Ok(output) => {
                                if output == "\\q" {
                                    break;
                                }
                                // Handle \r — reset query buffer
                                if output == "\\r" {
                                    sql_buffer.clear();
                                    println!("Query buffer reset.");
                                    continue;
                                }
                                // Handle \c dbname — switch database
                                if let Some(db_name) = output.strip_prefix("\\c ") {
                                    let db_name = db_name.trim().to_string();
                                    match self.db_manager.get_or_open(&db_name) {
                                        Ok(new_engine) => {
                                            self.catalog = Arc::clone(&new_engine.catalog);
                                            self.engine = new_engine;
                                            self.current_db = db_name.clone();
                                            println!("You are now connected to database \"{}\".", db_name);
                                        }
                                        Err(e) => eprintln!("ERROR: {}", e),
                                    }
                                    continue;
                                }
                                print!("{}", output);
                            }
                            Err(e) => eprintln!("ERROR: {}", e),
                        }
                    }
                    Err(e) => eprintln!("{}", e),
                }
                continue;
            }

            // Accumulate SQL
            if !sql_buffer.is_empty() {
                sql_buffer.push('\n');
            }
            sql_buffer.push_str(trimmed);

            // Execute when the buffer ends with a semicolon
            if sql_buffer.trim_end().ends_with(';') {
                let full_input = sql_buffer.trim().to_string();
                sql_buffer.clear();

                if full_input.is_empty() {
                    continue;
                }

                // Split on ';' so that a pasted block of multiple statements
                // (e.g. three CREATE TABLE statements separated by semicolons)
                // each produces its own output line.
                let stmts = split_statements(&full_input);
                for stmt in &stmts {
                    if stmt.is_empty() {
                        continue;
                    }
                    let _ = rl.add_history_entry(stmt.as_str());
                    let output = self.execute_sql(stmt, timing, expanded);
                    print!("{}", output);
                }
            }
        }

        // Save history
        if let Some(ref history_file) = self.config.history_file {
            let _ = rl.save_history(history_file);
        }

        Ok(())
    }

    fn execute_sql(&self, sql: &str, timing: bool, _expanded: bool) -> String {
        // Strip a trailing semicolon that may remain after splitting
        let sql = sql.trim().trim_end_matches(';').trim();
        let start = std::time::Instant::now();
        // Use execute_session so that BEGIN/COMMIT/ROLLBACK are handled
        // statelessly across successive calls (the "repl" session ID persists
        // for the lifetime of this process).
        match self.engine.execute_session("repl", sql) {
            Ok(result) => {
                let mut output = String::new();
                if !result.rows.is_empty() {
                    output.push_str(&format_table(&result.rows));
                } else {
                    output.push_str(&format_command_result(
                        &result.command,
                        result.rows_affected,
                    ));
                }
                if timing {
                    output.push_str(&format!(
                        "Time: {:.3} ms\n",
                        start.elapsed().as_secs_f64() * 1000.0
                    ));
                }
                output
            }
            Err(e) => format!("ERROR: {}\n", e),
        }
    }

    fn get_table_names(&self) -> Vec<String> {
        // Try to list tables for completion; ignore errors
        self.catalog.list_tables("public").unwrap_or_default()
    }
}

/// Split a SQL string on `;`, respecting single-quoted strings.
/// Returns non-empty trimmed statements.
fn split_statements(sql: &str) -> Vec<String> {
    let mut stmts = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut chars = sql.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            // Line comment: skip from '--' to end of line.
            // Must be checked before the general '-' arm and only outside quotes,
            // so that apostrophes inside comments don't corrupt quote tracking.
            '-' if !in_quote && chars.peek() == Some(&'-') => {
                chars.next(); // consume the second '-'
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '\'' => {
                in_quote = !in_quote;
                current.push(ch);
            }
            ';' if !in_quote => {
                let s = current.trim().to_string();
                if !s.is_empty() {
                    stmts.push(s);
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let s = current.trim().to_string();
    if !s.is_empty() {
        stmts.push(s);
    }
    stmts
}
