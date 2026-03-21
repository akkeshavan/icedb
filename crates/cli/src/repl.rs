use crate::completer::SqlCompleter;
use crate::config::Config;
use crate::error::CliError;
use crate::formatter::{format_command_result, format_table};
use crate::meta::{execute_meta_command, parse_meta_command};
use catalog::manager::CatalogManager;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use sql::engine::QueryEngine;
use std::sync::Arc;

pub struct Repl {
    engine: Arc<QueryEngine>,
    catalog: Arc<CatalogManager>,
    config: Config,
}

impl Repl {
    pub fn new(engine: Arc<QueryEngine>, catalog: Arc<CatalogManager>, config: Config) -> Self {
        Self {
            engine,
            catalog,
            config,
        }
    }

    pub fn run(&self) -> Result<(), CliError> {
        let mut rl = Editor::<SqlCompleter, rustyline::history::DefaultHistory>::new()
            .map_err(|e| CliError::Readline(e.to_string()))?;

        // Set up completer with known table names
        let table_names = self.get_table_names();
        rl.set_helper(Some(SqlCompleter { table_names }));

        // Load history
        if let Some(ref history_file) = self.config.history_file {
            let _ = rl.load_history(history_file);
        }

        println!("nkv-psql (icedb {})", env!("CARGO_PKG_VERSION"));
        println!("Type \"help\" for help, \"\\q\" to quit.\n");

        let mut timing = false;
        let mut expanded = false;
        let mut sql_buffer = String::new();

        loop {
            let prompt = if sql_buffer.is_empty() {
                format!("{}=# ", self.config.dbname)
            } else {
                format!("{}-# ", self.config.dbname)
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

            // Meta-commands
            if trimmed.starts_with('\\') {
                let _ = rl.add_history_entry(trimmed);
                match parse_meta_command(trimmed) {
                    Ok(cmd) => {
                        match execute_meta_command(
                            cmd,
                            &self.catalog,
                            &self.engine,
                            &mut timing,
                            &mut expanded,
                        ) {
                            Ok(output) => {
                                if output == "\\q" {
                                    break;
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
                sql_buffer.push(' ');
            }
            sql_buffer.push_str(trimmed);

            // Execute when we see a semicolon at the end
            if sql_buffer.trim_end().ends_with(';') {
                let sql = sql_buffer.trim().trim_end_matches(';').to_string();
                sql_buffer.clear();

                if sql.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(&sql);
                let output = self.execute_sql(&sql, timing, expanded);
                print!("{}", output);
            }
        }

        // Save history
        if let Some(ref history_file) = self.config.history_file {
            let _ = rl.save_history(history_file);
        }

        Ok(())
    }

    fn execute_sql(&self, sql: &str, timing: bool, _expanded: bool) -> String {
        let start = std::time::Instant::now();
        match self.engine.execute(sql) {
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
