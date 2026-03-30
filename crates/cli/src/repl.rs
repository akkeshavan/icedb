use crate::completer::SqlCompleter;
use crate::config::Config;
use crate::error::CliError;
use crate::formatter::{
    format_command_result, format_expanded, format_pg_table, format_table,
};
use crate::meta::{execute_meta_command, parse_meta_command, MetaCommand};
use crate::pg_client::PgClient;
use catalog::manager::CatalogManager;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use sql::db_manager::DatabaseManager;
use sql::engine::QueryEngine;
use std::sync::Arc;

// ── Backend ───────────────────────────────────────────────────────────────────

/// Dispatch target for SQL execution and meta-commands.
pub enum Backend {
    /// Engine is embedded in this process — no TCP involved.
    Embedded {
        db_manager: Arc<DatabaseManager>,
        engine: Arc<QueryEngine>,
        catalog: Arc<CatalogManager>,
    },
    /// Engine is running in a separate process; we talk to it over TCP using
    /// the PostgreSQL wire protocol (Simple Query sub-protocol).
    Network(PgClient),
}

// ── REPL ──────────────────────────────────────────────────────────────────────

pub struct Repl {
    backend: Backend,
    config: Config,
    current_db: String,
}

impl Repl {
    /// Construct a REPL backed by the embedded engine.
    pub fn new_embedded(
        db_manager: Arc<DatabaseManager>,
        engine: Arc<QueryEngine>,
        catalog: Arc<CatalogManager>,
        config: Config,
    ) -> Self {
        let current_db = config.dbname.clone();
        let mut repl = Self {
            backend: Backend::Embedded {
                db_manager,
                engine,
                catalog,
            },
            config,
            current_db,
        };
        // Switch to the requested database if it isn't the default.
        if repl.current_db != "icedb" {
            let db_name = repl.current_db.clone();
            if let Backend::Embedded {
                ref db_manager,
                ref mut engine,
                ref mut catalog,
            } = repl.backend
            {
                if let Ok(new_engine) = db_manager.get_or_open(&db_name) {
                    *catalog = Arc::clone(&new_engine.catalog);
                    *engine = new_engine;
                }
            }
        }
        repl
    }

    /// Construct a REPL backed by a TCP connection to a running server.
    pub fn new_network(client: PgClient, config: Config) -> Self {
        let current_db = client.current_db.clone();
        Self {
            backend: Backend::Network(client),
            config,
            current_db,
        }
    }

    // ── Run loop ──────────────────────────────────────────────────────────────

    pub fn run(&mut self) -> Result<(), CliError> {
        let mut rl = Editor::<SqlCompleter, rustyline::history::DefaultHistory>::new()
            .map_err(|e| CliError::Readline(e.to_string()))?;

        let table_names = self.get_table_names();
        rl.set_helper(Some(SqlCompleter { table_names }));

        if let Some(ref history_file) = self.config.history_file {
            let _ = rl.load_history(history_file);
        }

        let mode_label = match &self.backend {
            Backend::Network(_) => format!(
                " (server {}:{})",
                self.config.host, self.config.port
            ),
            Backend::Embedded { .. } => " (embedded)".to_string(),
        };
        println!("isql (icedb {}){}", env!("CARGO_PKG_VERSION"), mode_label);
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
            if trimmed.starts_with("--") {
                continue;
            }

            // ── Meta-commands ─────────────────────────────────────────────────
            if trimmed.starts_with('\\') {
                let _ = rl.add_history_entry(trimmed);
                match parse_meta_command(trimmed) {
                    Ok(cmd) => {
                        if !sql_buffer.is_empty()
                            && !matches!(cmd, MetaCommand::ResetBuffer)
                        {
                            eprintln!(
                                "WARNING: query buffer is not empty (forgot a semicolon?)."
                            );
                            eprintln!("         Use \\r to discard the pending input.");
                        }
                        match self.execute_meta(cmd, &mut timing, &mut expanded) {
                            Ok(output) => {
                                if output == "\\q" {
                                    break;
                                }
                                if output == "\\r" {
                                    sql_buffer.clear();
                                    println!("Query buffer reset.");
                                    continue;
                                }
                                // \c dbname — switch database
                                if let Some(db_name) = output.strip_prefix("\\c ") {
                                    let db_name = db_name.trim().to_string();
                                    match self.switch_db(&db_name) {
                                        Ok(()) => println!(
                                            "You are now connected to database \"{}\".",
                                            db_name
                                        ),
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

            // ── SQL accumulation and execution ────────────────────────────────
            if !sql_buffer.is_empty() {
                sql_buffer.push('\n');
            }
            sql_buffer.push_str(trimmed);

            if sql_buffer.trim_end().ends_with(';') {
                let full_input = sql_buffer.trim().to_string();
                sql_buffer.clear();

                if full_input.is_empty() {
                    continue;
                }

                for stmt in split_statements(&full_input) {
                    if stmt.is_empty() {
                        continue;
                    }
                    let _ = rl.add_history_entry(stmt.as_str());
                    let output = self.execute_sql(&stmt, timing, expanded);
                    print!("{}", output);
                }
            }
        }

        // Save history with 0600 permissions
        if let Some(ref history_file) = self.config.history_file {
            let _ = rl.save_history(history_file);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(
                    history_file,
                    std::fs::Permissions::from_mode(0o600),
                );
            }
        }

        Ok(())
    }

    // ── SQL execution ─────────────────────────────────────────────────────────

    fn execute_sql(&mut self, sql: &str, timing: bool, expanded: bool) -> String {
        let sql = sql.trim().trim_end_matches(';').trim();
        let start = std::time::Instant::now();

        let output = match &mut self.backend {
            Backend::Embedded { engine, .. } => match engine.execute_session("repl", sql) {
                Ok(result) => {
                    if !result.rows.is_empty() {
                        if expanded {
                            format_expanded(&result.rows)
                        } else {
                            format_table(&result.rows)
                        }
                    } else {
                        format_command_result(&result.command, result.rows_affected)
                    }
                }
                Err(e) => format!("ERROR: {}\n", e),
            },
            Backend::Network(client) => match client.query(sql) {
                Ok(result) => {
                    if !result.columns.is_empty() {
                        format_pg_table(&result.columns, &result.rows, expanded)
                    } else {
                        format!("{}\n", result.command_tag)
                    }
                }
                Err(e) => format!("ERROR: {}\n", e),
            },
        };

        if timing {
            format!(
                "{}Time: {:.3} ms\n",
                output,
                start.elapsed().as_secs_f64() * 1000.0
            )
        } else {
            output
        }
    }

    // ── Meta-command dispatch ─────────────────────────────────────────────────

    fn execute_meta(
        &mut self,
        cmd: MetaCommand,
        timing: &mut bool,
        expanded: &mut bool,
    ) -> Result<String, CliError> {
        match &mut self.backend {
            Backend::Embedded {
                db_manager,
                engine,
                catalog,
            } => execute_meta_command(
                cmd,
                catalog,
                engine,
                timing,
                expanded,
                db_manager.data_dir(),
            ),
            Backend::Network(client) => {
                execute_meta_network(cmd, client, timing, expanded)
            }
        }
    }

    // ── Database switching ────────────────────────────────────────────────────

    fn switch_db(&mut self, db_name: &str) -> Result<(), CliError> {
        match &mut self.backend {
            Backend::Embedded {
                db_manager,
                engine,
                catalog,
            } => {
                let new_engine = db_manager
                    .get_or_open(db_name)
                    .map_err(|e| CliError::Network(e.to_string()))?;
                *catalog = Arc::clone(&new_engine.catalog);
                *engine = new_engine;
                self.current_db = db_name.to_string();
                Ok(())
            }
            Backend::Network(client) => {
                let new_client = client.reconnect_db(db_name)?;
                *client = new_client;
                self.current_db = db_name.to_string();
                Ok(())
            }
        }
    }

    // ── Table-name list for autocomplete ─────────────────────────────────────

    fn get_table_names(&self) -> Vec<String> {
        match &self.backend {
            Backend::Embedded { catalog, .. } => {
                catalog.list_tables("public").unwrap_or_default()
            }
            Backend::Network(_) => Vec::new(), // populated lazily after connect
        }
    }
}

// ── Network meta-command execution ───────────────────────────────────────────

/// Handle meta-commands when the REPL is in network mode.
///
/// Commands that require catalog access (`\dt`, `\du`, `\l`, `\d`) are
/// translated into SQL queries sent to the server.  Commands that are
/// client-side state (`\timing`, `\x`, `\q`, etc.) are handled locally.
fn execute_meta_network(
    cmd: MetaCommand,
    client: &mut PgClient,
    timing: &mut bool,
    expanded: &mut bool,
) -> Result<String, CliError> {
    match cmd {
        MetaCommand::Quit => Ok("\\q".to_string()),
        MetaCommand::ResetBuffer => Ok("\\r".to_string()),
        MetaCommand::Help => Ok(crate::meta::HELP_TEXT.to_string()),
        MetaCommand::Timing => {
            *timing = !*timing;
            Ok(format!("Timing is {}.\n", if *timing { "on" } else { "off" }))
        }
        MetaCommand::ExpandedOutput => {
            *expanded = !*expanded;
            Ok(format!(
                "Expanded display is {}.\n",
                if *expanded { "on" } else { "off" }
            ))
        }
        MetaCommand::Connect(db) => {
            // Return the sentinel; Repl::run handles the actual reconnection
            // via switch_db so we don't need a double-mutable-borrow here.
            Ok(format!("\\c {}", db))
        }
        MetaCommand::ListTables => {
            let sql = "SELECT schemaname AS schema, tablename AS name, 'table' AS type \
                       FROM pg_catalog.pg_tables \
                       WHERE schemaname NOT IN ('pg_catalog', 'information_schema') \
                       ORDER BY tablename";
            match client.query(sql) {
                Ok(r) if r.columns.is_empty() && r.rows.is_empty() => {
                    Ok("Did not find any relations.\n".to_string())
                }
                Ok(r) => Ok(format_pg_table(&r.columns, &r.rows, false)),
                Err(e) => Err(e),
            }
        }
        MetaCommand::ListRoles => {
            let sql = "SELECT rolname, \
                              CASE WHEN rolsuper THEN 'Superuser' ELSE '' END AS superuser, \
                              CASE WHEN rolcreatedb THEN 'Create DB' ELSE '' END AS createdb, \
                              CASE WHEN rolcreaterole THEN 'Create role' ELSE '' END AS createrole \
                       FROM pg_catalog.pg_roles \
                       ORDER BY rolname";
            match client.query(sql) {
                Ok(r) => Ok(format_pg_table(&r.columns, &r.rows, false)),
                Err(e) => Err(e),
            }
        }
        MetaCommand::ListDatabases => {
            let sql = "SELECT datname AS name FROM pg_catalog.pg_database ORDER BY datname";
            match client.query(sql) {
                Ok(r) => Ok(format_pg_table(&r.columns, &r.rows, false)),
                Err(e) => Err(e),
            }
        }
        MetaCommand::Describe(table_name) => {
            let safe_name = table_name.replace('\'', "''");
            let sql = format!(
                "SELECT column_name, data_type, is_nullable \
                 FROM information_schema.columns \
                 WHERE table_name = '{}' AND table_schema = 'public' \
                 ORDER BY ordinal_position",
                safe_name
            );
            match client.query(&sql) {
                Ok(r) if r.rows.is_empty() => Ok(format!(
                    "Did not find any relation named \"{}\".\n",
                    table_name
                )),
                Ok(r) => Ok(format_pg_table(&r.columns, &r.rows, false)),
                Err(e) => Err(e),
            }
        }
    }
}

// ── Statement splitter ────────────────────────────────────────────────────────

/// Split a SQL string on `;`, respecting single-quoted strings.
fn split_statements(sql: &str) -> Vec<String> {
    let mut stmts = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut chars = sql.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '-' if !in_quote && chars.peek() == Some(&'-') => {
                chars.next();
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
