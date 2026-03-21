use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use catalog::manager::{AclPrivilege, CatalogManager};
use parking_lot::Mutex;
use txn::manager::TransactionManager;
use txn::transaction::IsolationLevel;
use txn::xid::Xid;

use crate::error::SqlError;
use crate::executor::{ExecutionContext, ExecutionResult, Executor};
use crate::optimizer::Optimizer;
use crate::parser::{ParseResult, Parser};
use crate::plan::LogicalPlan;
use crate::planner::Planner;
use crate::value::Value;

pub struct QueryEngine {
    pub txn_manager: Arc<TransactionManager>,
    pub catalog: Arc<CatalogManager>,
    pub data_dir: PathBuf,
    /// The role executing queries (None = superuser/bootstrap mode, allow all)
    pub current_role: Option<String>,
    /// Prepared statement cache: name → original SQL string
    prepared_statements: Mutex<HashMap<String, String>>,
    /// Per-session open transactions (session_id → Xid).
    /// Used by the wire protocol to support multi-statement transactions.
    open_sessions: Mutex<HashMap<String, Xid>>,
    /// Per-session savepoint stacks (session_id → Vec<savepoint_name>).
    /// Each savepoint records the names defined within the current transaction.
    session_savepoints: Mutex<HashMap<String, Vec<String>>>,
}

impl QueryEngine {
    pub fn new(
        txn_manager: Arc<TransactionManager>,
        catalog: Arc<CatalogManager>,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            txn_manager,
            catalog,
            data_dir,
            current_role: None,
            prepared_statements: Mutex::new(HashMap::new()),
            open_sessions: Mutex::new(HashMap::new()),
            session_savepoints: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_role(mut self, role: String) -> Self {
        self.current_role = Some(role);
        self
    }

    /// Check if the current role has the privileges required by the given plan.
    fn check_privileges(&self, plan: &LogicalPlan) -> Result<(), SqlError> {
        let role_name = match &self.current_role {
            None => return Ok(()), // No role set = internal/bootstrap mode, allow all
            Some(r) => r,
        };

        // Get the role from catalog
        let role = self
            .catalog
            .get_role(role_name)
            .map_err(|_| SqlError::Execution(format!("Role '{}' not found", role_name)))?;

        // Superusers bypass all checks
        if role.rolsuper {
            return Ok(());
        }

        // Check operation-specific privileges
        match plan {
            LogicalPlan::CreateTable { .. }
            | LogicalPlan::DropTable { .. }
            | LogicalPlan::CreateIndex { .. } => {
                if !role.rolcreatedb && !role.rolsuper {
                    return Err(SqlError::Execution(format!(
                        "Role '{}' does not have CREATE TABLE privilege",
                        role_name
                    )));
                }
            }
            LogicalPlan::CreateRole { .. } => {
                if !role.rolcreaterole {
                    return Err(SqlError::Execution(format!(
                        "Role '{}' does not have CREATEROLE privilege",
                        role_name
                    )));
                }
            }
            LogicalPlan::TableScan { table_name, .. }
            | LogicalPlan::IndexScan { table_name, .. } => {
                if !role.rolcanlogin {
                    return Err(SqlError::Execution(format!(
                        "Role '{}' cannot execute queries (no LOGIN privilege)",
                        role_name
                    )));
                }
                if !self.catalog.check_table_privilege("public", table_name, role_name, AclPrivilege::Select) {
                    return Err(SqlError::Execution(format!(
                        "Permission denied for table '{}': role '{}' does not have SELECT privilege",
                        table_name, role_name
                    )));
                }
            }
            LogicalPlan::Insert { table_name, .. } => {
                if !role.rolcanlogin {
                    return Err(SqlError::Execution(format!(
                        "Role '{}' cannot execute queries (no LOGIN privilege)",
                        role_name
                    )));
                }
                if !self.catalog.check_table_privilege("public", table_name, role_name, AclPrivilege::Insert) {
                    return Err(SqlError::Execution(format!(
                        "Permission denied for table '{}': role '{}' does not have INSERT privilege",
                        table_name, role_name
                    )));
                }
            }
            LogicalPlan::Update { table_name, .. } => {
                if !role.rolcanlogin {
                    return Err(SqlError::Execution(format!(
                        "Role '{}' cannot execute queries (no LOGIN privilege)",
                        role_name
                    )));
                }
                if !self.catalog.check_table_privilege("public", table_name, role_name, AclPrivilege::Update) {
                    return Err(SqlError::Execution(format!(
                        "Permission denied for table '{}': role '{}' does not have UPDATE privilege",
                        table_name, role_name
                    )));
                }
            }
            LogicalPlan::Delete { table_name, .. } => {
                if !role.rolcanlogin {
                    return Err(SqlError::Execution(format!(
                        "Role '{}' cannot execute queries (no LOGIN privilege)",
                        role_name
                    )));
                }
                if !self.catalog.check_table_privilege("public", table_name, role_name, AclPrivilege::Delete) {
                    return Err(SqlError::Execution(format!(
                        "Permission denied for table '{}': role '{}' does not have DELETE privilege",
                        table_name, role_name
                    )));
                }
            }
            // Other plans: require login
            _ => {
                if !role.rolcanlogin {
                    return Err(SqlError::Execution(format!(
                        "Role '{}' cannot execute queries (no LOGIN privilege)",
                        role_name
                    )));
                }
            }
        }

        Ok(())
    }

    /// Execute SQL in the context of an existing transaction.
    pub fn execute_in_txn(&self, xid: Xid, sql: &str) -> Result<ExecutionResult, SqlError> {
        let ctx = Arc::new(ExecutionContext {
            xid,
            data_dir: self.data_dir.clone(),
            txn_manager: Arc::clone(&self.txn_manager),
            catalog: Arc::clone(&self.catalog),
        });
        let executor = Executor::new(ctx);

        // Handle VACUUM, LISTEN, NOTIFY specially (not parsed by sqlparser)
        let parse_result = Parser::parse_with_vacuum(sql)?;
        match parse_result {
            ParseResult::Vacuum { table_name, analyze } => {
                let plan = LogicalPlan::Vacuum {
                    table_name,
                    schema: "public".to_string(),
                    analyze,
                };
                self.check_privileges(&plan)?;
                executor.execute(plan)
            }
            ParseResult::Listen { channel } => {
                // In embedded mode, LISTEN is acknowledged but the receiver is not surfaced.
                // A full server implementation would track receivers per connection.
                let _ = self.catalog.listen(&channel);
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: 0,
                    command: "LISTEN".to_string(),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            ParseResult::Unlisten { channel } => {
                self.catalog.unlisten(channel.as_deref());
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: 0,
                    command: "UNLISTEN".to_string(),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            ParseResult::Notify { channel, payload } => {
                self.catalog.notify(&channel, payload.as_deref().unwrap_or(""));
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: 0,
                    command: "NOTIFY".to_string(),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            ParseResult::DropFunction { name, if_exists } => {
                let plan = LogicalPlan::DropFunction {
                    schema: "public".to_string(),
                    name,
                    if_exists,
                };
                executor.execute(plan)
            }
            ParseResult::Statements(stmts) => {
                if stmts.is_empty() {
                    return Ok(ExecutionResult {
                        rows: vec![],
                        rows_affected: 0,
                        command: "EMPTY".to_string(),
                        col_names: vec![],
                        col_types: vec![],
                    });
                }

                let planner = Planner::new(Arc::clone(&self.catalog));

                // Execute last statement and return its result
                let mut result = ExecutionResult {
                    rows: vec![],
                    rows_affected: 0,
                    command: "EMPTY".to_string(),
                    col_names: vec![],
                    col_types: vec![],
                };

                let optimizer = Optimizer::new(Arc::clone(&self.catalog));

                for stmt in &stmts {
                    let raw_plan = planner.plan_statement(stmt)?;
                    let plan = optimizer.optimize(raw_plan);
                    match &plan {
                        LogicalPlan::Prepare { name, sql } => {
                            self.prepared_statements.lock().insert(name.clone(), sql.clone());
                            result = ExecutionResult {
                                rows: vec![],
                                rows_affected: 0,
                                command: "PREPARE".to_string(),
                                col_names: vec![],
                                col_types: vec![],
                            };
                        }
                        LogicalPlan::ExecutePrepared { name, params } => {
                            let sql = {
                                let cache = self.prepared_statements.lock();
                                cache.get(name).cloned().ok_or_else(|| {
                                    SqlError::Execution(format!(
                                        "prepared statement \"{}\" does not exist",
                                        name
                                    ))
                                })?
                            };
                            // Substitute $1, $2, ... with literal values
                            let mut exec_sql = sql.clone();
                            for (i, val) in params.iter().enumerate() {
                                let placeholder = format!("${}", i + 1);
                                let literal = value_to_sql_literal(val);
                                exec_sql = exec_sql.replace(&placeholder, &literal);
                            }
                            result = self.execute_in_txn(xid, &exec_sql)?;
                        }
                        LogicalPlan::Deallocate { name } => {
                            let name_upper = name.to_uppercase();
                            if name_upper == "*" || name_upper == "ALL" {
                                self.prepared_statements.lock().clear();
                            } else {
                                self.prepared_statements.lock().remove(name);
                            }
                            result = ExecutionResult {
                                rows: vec![],
                                rows_affected: 0,
                                command: "DEALLOCATE".to_string(),
                                col_names: vec![],
                                col_types: vec![],
                            };
                        }
                        _ => {
                            self.check_privileges(&plan)?;
                            result = executor.execute(plan)?;
                        }
                    }
                }

                Ok(result)
            }
        }
    }

    /// Start the autovacuum background task.
    /// The task periodically vacuums tables that haven't been vacuumed recently.
    /// Returns a JoinHandle that can be aborted to stop the task.
    pub fn start_autovacuum(engine: Arc<QueryEngine>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            // Autovacuum interval: 60 seconds between sweeps
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                // Tables that haven't been vacuumed in the last 5 minutes
                let tables = engine.catalog.tables_needing_vacuum("public", 300);
                for table in tables {
                    let sql = format!("VACUUM {}", table);
                    // best-effort: ignore errors
                    let _ = engine.execute(&sql);
                }
            }
        })
    }

    /// Dump all tables in the public schema to a SQL file.
    /// Returns the number of SQL statements written.
    pub fn dump_to_file(&self, path: &str) -> Result<usize, SqlError> {
        use std::io::Write;
        let mut file = std::fs::File::create(path)
            .map_err(|e| SqlError::Execution(format!("Cannot create dump file: {}", e)))?;

        let mut statement_count = 0usize;

        writeln!(file, "-- icedb dump").map_err(|e| SqlError::Execution(e.to_string()))?;
        writeln!(file, "-- Generated by icedb").map_err(|e| SqlError::Execution(e.to_string()))?;
        writeln!(file).map_err(|e| SqlError::Execution(e.to_string()))?;

        let tables = self.catalog
            .list_tables("public")
            .map_err(|e| SqlError::Execution(e.to_string()))?;

        for table_name in &tables {
            let schema = self.catalog
                .get_table("public", table_name)
                .map_err(|e| SqlError::Execution(e.to_string()))?;

            // Write CREATE TABLE
            let create_sql = schema_to_create_table_sql(&schema, table_name);
            writeln!(file, "{};", create_sql).map_err(|e| SqlError::Execution(e.to_string()))?;
            writeln!(file).map_err(|e| SqlError::Execution(e.to_string()))?;
            statement_count += 1;

            // Write INSERT statements for all rows
            let result = self.execute(&format!("SELECT * FROM {}", table_name))?;
            for row in &result.rows {
                let insert_sql = row_to_insert_sql(table_name, row, &schema);
                writeln!(file, "{};", insert_sql).map_err(|e| SqlError::Execution(e.to_string()))?;
                statement_count += 1;
            }
            writeln!(file).map_err(|e| SqlError::Execution(e.to_string()))?;
        }

        // Generate CREATE INDEX statements
        // Build a map of table OID → table name using pg_class rows
        let class_rows = self.catalog
            .list_pg_class_rows()
            .map_err(|e| SqlError::Execution(e.to_string()))?;
        let oid_to_name: std::collections::HashMap<u32, String> = class_rows
            .into_iter()
            .map(|row| (row.oid, row.relname))
            .collect();

        let indexes = self.catalog.list_indexes();
        if !indexes.is_empty() {
            writeln!(file, "-- indexes").map_err(|e| SqlError::Execution(e.to_string()))?;
        }
        for (table_oid, column_name, _path) in &indexes {
            if let Some(table_name) = oid_to_name.get(table_oid) {
                let idx_name = format!("idx_{}_{}", table_name, column_name);
                let idx_sql = format!(
                    "CREATE INDEX IF NOT EXISTS {} ON {}({});\n",
                    idx_name, table_name, column_name
                );
                write!(file, "{}", idx_sql).map_err(|e| SqlError::Execution(e.to_string()))?;
                statement_count += 1;
            }
        }

        Ok(statement_count)
    }

    /// Restore from a SQL dump file produced by `dump_to_file`.
    /// Returns the number of statements executed.
    pub fn restore_from_file(&self, path: &str) -> Result<usize, SqlError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| SqlError::Execution(format!("Cannot read dump file: {}", e)))?;

        let mut count = 0usize;
        // Split on ";\n" boundaries; each chunk is one statement
        let mut current = String::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("--") {
                continue;
            }
            current.push_str(line);
            current.push('\n');
            if trimmed.ends_with(';') {
                let stmt = current.trim().trim_end_matches(';').trim().to_string();
                current.clear();
                if !stmt.is_empty() {
                    self.execute(&stmt)?;
                    count += 1;
                }
            }
        }
        // Execute any remaining statement (no trailing semicolon)
        let stmt = current.trim().trim_end_matches(';').trim().to_string();
        if !stmt.is_empty() {
            self.execute(&stmt)?;
            count += 1;
        }

        Ok(count)
    }

    /// Execute SQL within a named session context, returning one result per statement.
    /// This is used by the wire protocol so that "SELECT 1; SELECT 2;" sends two
    /// separate result sets to the client instead of only the last one.
    pub fn execute_session_multi(&self, session_id: &str, sql: &str) -> Result<Vec<ExecutionResult>, SqlError> {
        let stmts = split_sql_statements(sql);
        if stmts.is_empty() {
            return Ok(vec![ExecutionResult {
                rows: vec![], rows_affected: 0,
                command: "EMPTY".to_string(), col_names: vec![], col_types: vec![],
            }]);
        }
        let mut results = Vec::with_capacity(stmts.len());
        for stmt in stmts {
            if !stmt.trim().is_empty() {
                results.push(self.execute_session_single(session_id, stmt.trim())?);
            }
        }
        if results.is_empty() {
            results.push(ExecutionResult {
                rows: vec![], rows_affected: 0,
                command: "EMPTY".to_string(), col_names: vec![], col_types: vec![],
            });
        }
        Ok(results)
    }

    /// Execute SQL within a named session context.
    /// Supports multi-statement transactions: BEGIN opens a transaction that persists
    /// across calls until COMMIT or ROLLBACK is sent in the same session.
    pub fn execute_session(&self, session_id: &str, sql: &str) -> Result<ExecutionResult, SqlError> {
        // Split into individual statements so that multi-statement strings like
        // "BEGIN; INSERT INTO t VALUES (1); COMMIT;" are handled correctly.
        let stmts = split_sql_statements(sql);
        if stmts.len() > 1 {
            let mut last = ExecutionResult {
                rows: vec![], rows_affected: 0,
                command: "EMPTY".to_string(), col_names: vec![], col_types: vec![],
            };
            for stmt in stmts {
                if !stmt.trim().is_empty() {
                    last = self.execute_session_single(session_id, stmt.trim())?;
                }
            }
            return Ok(last);
        }
        self.execute_session_single(session_id, sql.trim())
    }

    /// Execute a single (already-split) SQL statement in the session context.
    fn execute_session_single(&self, session_id: &str, sql: &str) -> Result<ExecutionResult, SqlError> {
        let sql_trimmed = sql.trim().trim_end_matches(';').trim();
        let sql_upper = sql_trimmed.to_ascii_uppercase();

        // BEGIN / START TRANSACTION
        if sql_upper == "BEGIN" || sql_upper.starts_with("BEGIN ") || sql_upper.starts_with("START TRANSACTION") {
            if !self.open_sessions.lock().contains_key(session_id) {
                let xid = self.txn_manager.begin(IsolationLevel::ReadCommitted);
                self.open_sessions.lock().insert(session_id.to_string(), xid);
            }
            return Ok(ExecutionResult {
                rows: vec![], rows_affected: 0,
                command: "BEGIN".to_string(), col_names: vec![], col_types: vec![],
            });
        }

        // COMMIT / END
        if sql_upper == "COMMIT" || sql_upper.starts_with("COMMIT ") || sql_upper == "END" || sql_upper.starts_with("END ") {
            if let Some(xid) = self.open_sessions.lock().remove(session_id) {
                self.txn_manager.commit(xid)?;
            }
            self.session_savepoints.lock().remove(session_id);
            return Ok(ExecutionResult {
                rows: vec![], rows_affected: 0,
                command: "COMMIT".to_string(), col_names: vec![], col_types: vec![],
            });
        }

        // ROLLBACK / ABORT
        if sql_upper == "ROLLBACK" || sql_upper == "ABORT"
            || (sql_upper.starts_with("ROLLBACK ") && !sql_upper.starts_with("ROLLBACK TO"))
            || sql_upper.starts_with("ABORT ")
        {
            if let Some(xid) = self.open_sessions.lock().remove(session_id) {
                let _ = self.txn_manager.abort(xid);
            }
            self.session_savepoints.lock().remove(session_id);
            return Ok(ExecutionResult {
                rows: vec![], rows_affected: 0,
                command: "ROLLBACK".to_string(), col_names: vec![], col_types: vec![],
            });
        }

        // SAVEPOINT <name>
        if sql_upper.starts_with("SAVEPOINT ") {
            let sp_name = sql_trimmed["SAVEPOINT ".len()..].trim().trim_matches('"').to_string();
            // Ensure there's an open transaction
            if !self.open_sessions.lock().contains_key(session_id) {
                let xid = self.txn_manager.begin(IsolationLevel::ReadCommitted);
                self.open_sessions.lock().insert(session_id.to_string(), xid);
            }
            self.session_savepoints.lock()
                .entry(session_id.to_string())
                .or_default()
                .push(sp_name);
            return Ok(ExecutionResult {
                rows: vec![], rows_affected: 0,
                command: "SAVEPOINT".to_string(), col_names: vec![], col_types: vec![],
            });
        }

        // ROLLBACK TO [SAVEPOINT] <name>
        if sql_upper.starts_with("ROLLBACK TO ") {
            let rest = sql_trimmed["ROLLBACK TO ".len()..].trim();
            let sp_name = rest.strip_prefix("SAVEPOINT ").unwrap_or(rest)
                .trim().trim_matches('"').to_string();
            let mut savepoints = self.session_savepoints.lock();
            let stack = savepoints.entry(session_id.to_string()).or_default();
            // Check savepoint exists
            if !stack.contains(&sp_name) {
                return Err(SqlError::Execution(format!("savepoint \"{sp_name}\" does not exist")));
            }
            // Pop savepoints after the target (keep the named one)
            if let Some(pos) = stack.iter().rposition(|s| s == &sp_name) {
                stack.truncate(pos + 1);
            }
            // Abort current txn and start a new one (data since BEGIN is lost;
            // full subtransaction support would require page-level undo)
            drop(savepoints);
            if let Some(xid) = self.open_sessions.lock().remove(session_id) {
                let _ = self.txn_manager.abort(xid);
            }
            let new_xid = self.txn_manager.begin(IsolationLevel::ReadCommitted);
            self.open_sessions.lock().insert(session_id.to_string(), new_xid);
            return Ok(ExecutionResult {
                rows: vec![], rows_affected: 0,
                command: "ROLLBACK".to_string(), col_names: vec![], col_types: vec![],
            });
        }

        // RELEASE [SAVEPOINT] <name>
        if sql_upper.starts_with("RELEASE ") {
            let rest = sql_trimmed["RELEASE ".len()..].trim();
            let sp_name = rest.strip_prefix("SAVEPOINT ").unwrap_or(rest)
                .trim().trim_matches('"').to_string();
            let mut savepoints = self.session_savepoints.lock();
            let stack = savepoints.entry(session_id.to_string()).or_default();
            if let Some(pos) = stack.iter().rposition(|s| s == &sp_name) {
                stack.remove(pos);
                return Ok(ExecutionResult {
                    rows: vec![], rows_affected: 0,
                    command: "RELEASE".to_string(), col_names: vec![], col_types: vec![],
                });
            }
            return Err(SqlError::Execution(format!("savepoint \"{sp_name}\" does not exist")));
        }

        // SET TRANSACTION ... — treat as a no-op (isolation level already set at BEGIN)
        if sql_upper.starts_with("SET TRANSACTION ") {
            return Ok(ExecutionResult {
                rows: vec![], rows_affected: 0,
                command: "SET".to_string(), col_names: vec![], col_types: vec![],
            });
        }

        // Use session transaction if one is open, otherwise auto-commit
        let open_xid = self.open_sessions.lock().get(session_id).copied();
        if let Some(xid) = open_xid {
            match self.execute_in_txn(xid, sql) {
                Ok(result) => Ok(result),
                Err(e) => {
                    self.open_sessions.lock().remove(session_id);
                    let _ = self.txn_manager.abort(xid);
                    Err(e)
                }
            }
        } else {
            self.execute(sql)
        }
    }

    /// Abort any open session transaction (called on connection drop).
    pub fn abort_session(&self, session_id: &str) {
        if let Some(xid) = self.open_sessions.lock().remove(session_id) {
            let _ = self.txn_manager.abort(xid);
        }
    }

    /// Execute SQL, auto-committing (begin → execute → commit/rollback).
    pub fn execute(&self, sql: &str) -> Result<ExecutionResult, SqlError> {
        let xid = self.txn_manager.begin(IsolationLevel::ReadCommitted);
        match self.execute_in_txn(xid, sql) {
            Ok(result) => {
                self.txn_manager.commit(xid)?;
                Ok(result)
            }
            Err(e) => {
                let _ = self.txn_manager.abort(xid);
                Err(e)
            }
        }
    }
}

/// Convert a Value to a SQL literal string for substitution into a prepared statement.
/// Split a SQL string into individual statements on `;`, respecting single-quoted strings.
fn split_sql_statements(sql: &str) -> Vec<String> {
    let mut stmts = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;

    for ch in sql.chars() {
        match ch {
            '\'' => {
                in_single_quote = !in_single_quote;
                current.push(ch);
            }
            ';' if !in_single_quote => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    stmts.push(trimmed);
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        stmts.push(trimmed);
    }
    stmts
}

fn value_to_sql_literal(val: &Value) -> String {
    match val {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => if *b { "TRUE".to_string() } else { "FALSE".to_string() },
        Value::Int4(n) => n.to_string(),
        Value::Int8(n) => n.to_string(),
        Value::Float8(f) => f.to_string(),
        Value::Text(s) => format!("'{}'", s.replace('\'', "''")),
        Value::Numeric(s) => s.clone(),
        Value::Uuid(s) => format!("'{}'", s),
        Value::Date(d) => format!("'{}'", d),
        Value::Timestamp(t) => format!("'{}'", t),
        Value::Bytes(_) => "NULL".to_string(),
    }
}

fn schema_to_create_table_sql(schema: &catalog::schema::TableSchema, table_name: &str) -> String {
    let cols: Vec<String> = schema.columns.iter().map(|col| {
        let type_str = match col.data_type {
            catalog::DataType::Boolean => "BOOLEAN",
            catalog::DataType::Int4 => "INTEGER",
            catalog::DataType::Int8 => "BIGINT",
            catalog::DataType::Float8 => "FLOAT",
            catalog::DataType::Text => "TEXT",
            catalog::DataType::VarChar(_) => "TEXT",
            catalog::DataType::Bytea => "BYTEA",
            catalog::DataType::Date => "DATE",
            catalog::DataType::Timestamp => "TIMESTAMP",
            catalog::DataType::TimestampTz => "TIMESTAMPTZ",
            catalog::DataType::Numeric => "NUMERIC",
            catalog::DataType::Uuid => "UUID",
        };
        let null_str = if col.not_null { " NOT NULL" } else { "" };
        format!("    {} {}{}", col.name, type_str, null_str)
    }).collect();
    format!("CREATE TABLE IF NOT EXISTS {} (\n{}\n)", table_name, cols.join(",\n"))
}

fn row_to_insert_sql(table_name: &str, row: &crate::row::Row, schema: &catalog::schema::TableSchema) -> String {
    let cols: Vec<String> = schema.columns.iter().map(|c| c.name.clone()).collect();
    let vals: Vec<String> = row.values.iter().map(value_to_sql_literal).collect();
    format!(
        "INSERT INTO {} ({}) VALUES ({})",
        table_name,
        cols.join(", "),
        vals.join(", ")
    )
}
