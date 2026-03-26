use std::collections::HashMap;
use std::sync::Arc;

use btree::BTree;
use catalog::manager::CatalogManager;
use storage::heap::HeapFile;
use txn::manager::TransactionManager;
use txn::xid::Xid;

use crate::codec::{decode_row, encode_row, encode_sort_key};
use crate::error::SqlError;
use crate::plan::{
    AggFunc, AlterTableOp, BinaryOp, Expr, InsertSource, JoinType, LogicalPlan, SetOperation,
    SortKey, UnaryOp, WindowExpr, WindowFunction,
};
use crate::row::Row;
use crate::value::Value;

pub struct ExecutionContext {
    pub xid: Xid,
    pub data_dir: std::path::PathBuf,
    pub db_name: String,
    pub txn_manager: Arc<TransactionManager>,
    pub catalog: Arc<CatalogManager>,
    /// Shared undo log for the current session transaction.
    /// When `Some`, every DML operation appends an `UndoEntry` so that
    /// `ROLLBACK TO SAVEPOINT` can reverse individual changes.
    /// `None` for auto-commit statements (no savepoint tracking needed).
    pub undo_sink: Option<Arc<parking_lot::Mutex<Vec<crate::subtxn::UndoEntry>>>>,
}

#[derive(Debug)]
pub struct ExecutionResult {
    pub rows: Vec<Row>,
    pub rows_affected: u64,
    pub command: String,
    /// Column names in result order. Present even when rows is empty, so that
    /// the wire protocol can send a proper RowDescription for 0-row queries.
    pub col_names: Vec<String>,
    /// Column types in result order. Parallel to col_names; used by the wire
    /// protocol to send the correct type OID in RowDescription for 0-row results.
    pub col_types: Vec<catalog::DataType>,
}

impl ExecutionResult {
    /// Construct a typed query result with explicit column names and types (for 0-row results).
    pub fn typed_query_result(col_names: Vec<String>, col_types: Vec<catalog::DataType>, rows: Vec<Row>, command: impl Into<String>) -> Self {
        Self { rows, rows_affected: 0, command: command.into(), col_names, col_types }
    }
}

pub struct Executor {
    ctx: Arc<ExecutionContext>,
    /// CTE context: active CTE materialized rows, set by exec_cte so that
    /// subquery evaluation (InSubquery, Exists, ScalarSubquery) can see CTE rows.
    cte_context: std::cell::RefCell<HashMap<String, Vec<Row>>>,
}

impl Executor {
    pub fn new(ctx: Arc<ExecutionContext>) -> Self {
        Self { ctx, cte_context: std::cell::RefCell::new(HashMap::new()) }
    }

    /// Append an undo entry to the session undo log, if one is active.
    #[inline]
    fn record_undo(&self, entry: crate::subtxn::UndoEntry) {
        if let Some(sink) = &self.ctx.undo_sink {
            sink.lock().push(entry);
        }
    }

    pub fn execute(&self, plan: LogicalPlan) -> Result<ExecutionResult, SqlError> {
        match &plan {
            LogicalPlan::CreateTable {
                schema_name,
                table_name,
                columns,
                if_not_exists,
                primary_key,
                unique_columns,
                foreign_keys,
                check_constraints,
            } => {
                self.exec_create_table(schema_name, table_name, columns, *if_not_exists, primary_key.as_deref(), unique_columns, foreign_keys, check_constraints)?;
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: 0,
                    command: format!("CREATE TABLE {}", table_name),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            LogicalPlan::DropTable {
                schema_name,
                table_name,
                if_exists,
            } => {
                self.exec_drop_table(schema_name, table_name, *if_exists)?;
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: 0,
                    command: format!("DROP TABLE {}", table_name),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            LogicalPlan::CreateRole {
                rolname,
                rolsuper,
                rolcanlogin,
                password,
            } => {
                self.ctx.catalog.create_role(
                    self.ctx.xid,
                    rolname,
                    *rolsuper,
                    *rolcanlogin,
                    password.clone(),
                )?;
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: 0,
                    command: "CREATE ROLE".to_string(),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            LogicalPlan::Insert {
                table_name,
                schema,
                columns,
                source,
                returning,
                on_conflict,
            } => {
                let (count, ret_rows) =
                    self.exec_insert(table_name, schema, columns, source, returning, on_conflict.as_ref())?;
                if !ret_rows.is_empty() {
                    let n = ret_rows.len() as u64;
                    let col_names = ret_rows[0].schema.iter().map(|(nm, _)| nm.clone()).collect();
                    let col_types = ret_rows[0].schema.iter().map(|(_, dt)| dt.clone()).collect();
                    return Ok(ExecutionResult {
                        rows: ret_rows,
                        rows_affected: n,
                        command: format!("INSERT 0 {n}"),
                        col_names,
                        col_types,
                    });
                }
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: count,
                    command: format!("INSERT 0 {count}"),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            LogicalPlan::Update {
                table_name,
                schema,
                assignments,
                filter,
                returning,
                from_plan,
            } => {
                let (count, ret_rows) =
                    self.exec_update(table_name, schema, assignments, filter, returning, from_plan.as_deref())?;
                if !ret_rows.is_empty() {
                    let n = ret_rows.len() as u64;
                    let col_names = ret_rows[0].schema.iter().map(|(nm, _)| nm.clone()).collect();
                    let col_types = ret_rows[0].schema.iter().map(|(_, dt)| dt.clone()).collect();
                    return Ok(ExecutionResult {
                        rows: ret_rows,
                        rows_affected: n,
                        command: format!("UPDATE {n}"),
                        col_names,
                        col_types,
                    });
                }
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: count,
                    command: format!("UPDATE {count}"),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            LogicalPlan::Delete {
                table_name,
                schema,
                filter,
                returning,
                using_plan,
            } => {
                let (count, ret_rows) =
                    self.exec_delete(table_name, schema, filter, returning, using_plan.as_deref())?;
                if !ret_rows.is_empty() {
                    let n = ret_rows.len() as u64;
                    let col_names = ret_rows[0].schema.iter().map(|(nm, _)| nm.clone()).collect();
                    let col_types = ret_rows[0].schema.iter().map(|(_, dt)| dt.clone()).collect();
                    return Ok(ExecutionResult {
                        rows: ret_rows,
                        rows_affected: n,
                        command: format!("DELETE {n}"),
                        col_names,
                        col_types,
                    });
                }
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: count,
                    command: format!("DELETE {count}"),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            LogicalPlan::CreateIndex {
                schema_name,
                table_name,
                column_name,
                index_name: _,
            } => {
                self.exec_create_index(schema_name, table_name, column_name)?;
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: 0,
                    command: "CREATE INDEX".to_string(),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            LogicalPlan::NoOp { command } => Ok(ExecutionResult {
                rows: vec![],
                rows_affected: 0,
                command: command.clone(),
                col_names: vec![],
                col_types: vec![],
            }),
            LogicalPlan::CreateDatabase { name, if_not_exists } => {
                use crate::db_manager::{DatabaseRegistry, database_dir, open_engine};
                let registry = DatabaseRegistry::new(&self.ctx.data_dir);
                if registry.database_exists(name) {
                    if *if_not_exists {
                        return Ok(ExecutionResult {
                            rows: vec![], rows_affected: 0,
                            command: format!("CREATE DATABASE {}", name),
                            col_names: vec![], col_types: vec![],
                        });
                    }
                    return Err(SqlError::Execution(format!(
                        "database \"{}\" already exists", name
                    )));
                }
                // Validate name
                if name.is_empty() || name.contains('/') || name.contains('\\') {
                    return Err(SqlError::Execution(format!(
                        "invalid database name \"{}\"", name
                    )));
                }
                // Create directory and bootstrap catalog
                let db_dir = database_dir(&self.ctx.data_dir, name);
                std::fs::create_dir_all(&db_dir).map_err(|e| {
                    SqlError::Execution(format!("cannot create database directory: {}", e))
                })?;
                open_engine(&db_dir, name)?;
                // Register
                registry.register(name, "icedb").map_err(|e| {
                    SqlError::Execution(format!("cannot register database: {}", e))
                })?;
                Ok(ExecutionResult {
                    rows: vec![], rows_affected: 0,
                    command: format!("CREATE DATABASE {}", name),
                    col_names: vec![], col_types: vec![],
                })
            }
            LogicalPlan::DropDatabase { name, if_exists } => {
                use crate::db_manager::{DatabaseRegistry, database_dir};
                if name == "icedb" {
                    return Err(SqlError::Execution(
                        "cannot drop the default database \"icedb\"".to_string()
                    ));
                }
                let registry = DatabaseRegistry::new(&self.ctx.data_dir);
                if !registry.database_exists(name) {
                    if *if_exists {
                        return Ok(ExecutionResult {
                            rows: vec![], rows_affected: 0,
                            command: format!("DROP DATABASE {}", name),
                            col_names: vec![], col_types: vec![],
                        });
                    }
                    return Err(SqlError::Execution(format!(
                        "database \"{}\" does not exist", name
                    )));
                }
                let db_dir = database_dir(&self.ctx.data_dir, name);
                if db_dir.exists() {
                    std::fs::remove_dir_all(&db_dir).map_err(|e| {
                        SqlError::Execution(format!("cannot remove database directory: {}", e))
                    })?;
                }
                registry.unregister(name).map_err(|e| {
                    SqlError::Execution(format!("cannot unregister database: {}", e))
                })?;
                Ok(ExecutionResult {
                    rows: vec![], rows_affected: 0,
                    command: format!("DROP DATABASE {}", name),
                    col_names: vec![], col_types: vec![],
                })
            }
            LogicalPlan::CreateSchema { name, if_not_exists } => {
                self.ctx.catalog.create_namespace(name, *if_not_exists)
                    .map_err(|e| SqlError::Execution(e.to_string()))?;
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: 0,
                    command: "CREATE SCHEMA".to_string(),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            LogicalPlan::AlterTable { table_name, operation } => {
                self.exec_alter_table(table_name, operation)?;
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: 0,
                    command: "ALTER TABLE".to_string(),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            // Transaction control statements: in auto-commit embedded mode, these are
            // accepted without error. BEGIN/COMMIT/ROLLBACK are processed at the engine
            // level (engine.execute() already wraps each call in a transaction).
            LogicalPlan::TransactionControl { kind } => {
                let cmd = match kind {
                    crate::plan::TransactionControlKind::Begin => "BEGIN",
                    crate::plan::TransactionControlKind::Commit => "COMMIT",
                    crate::plan::TransactionControlKind::Rollback => "ROLLBACK",
                    crate::plan::TransactionControlKind::Savepoint => "SAVEPOINT",
                    crate::plan::TransactionControlKind::RollbackToSavepoint => "ROLLBACK",
                    crate::plan::TransactionControlKind::ReleaseSavepoint => "RELEASE",
                };
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: 0,
                    command: cmd.to_string(),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            LogicalPlan::Grant { table_name, schema, grantee, privileges, columns } => {
                self.exec_grant(schema, table_name, grantee, privileges, columns)?;
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: 0,
                    command: "GRANT".to_string(),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            LogicalPlan::Revoke { table_name, schema, grantee, privileges, columns } => {
                self.exec_revoke(schema, table_name, grantee, privileges, columns)?;
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: 0,
                    command: "REVOKE".to_string(),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            LogicalPlan::Vacuum { table_name, schema, analyze } => {
                let count = self.exec_vacuum(schema, table_name.as_deref(), *analyze)?;
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: count,
                    command: "VACUUM".to_string(),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            LogicalPlan::CopyFrom {
                table_name,
                schema_name,
                file_path,
                delimiter,
                has_header,
                quote,
            } => {
                let count = self.exec_copy_from(table_name, schema_name, file_path, *delimiter, *has_header, *quote)?;
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: count,
                    command: format!("COPY {count}"),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            LogicalPlan::CopyTo {
                table_name,
                query,
                file_path,
                delimiter,
                has_header,
            } => {
                let count = self.exec_copy_to(table_name.as_deref(), query.as_deref(), file_path, *delimiter, *has_header)?;
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: count,
                    command: format!("COPY {count}"),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            // Prepare/Execute/Deallocate are handled at engine level; if they reach executor, no-op
            LogicalPlan::Prepare { .. } | LogicalPlan::ExecutePrepared { .. } | LogicalPlan::Deallocate { .. } => {
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: 0,
                    command: "OK".to_string(),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            LogicalPlan::CreateFunction { schema, name, params, return_type, body_sql, language } => {
                let func = catalog::schema::FunctionDef {
                    name: name.clone(),
                    schema: schema.clone(),
                    params: params.clone(),
                    return_type: return_type.clone(),
                    body_sql: body_sql.clone(),
                    language: language.clone(),
                };
                self.ctx.catalog.create_function(func)?;
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: 0,
                    command: "CREATE FUNCTION".to_string(),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            LogicalPlan::DropFunction { schema, name, if_exists } => {
                match self.ctx.catalog.drop_function(schema, name) {
                    Ok(_) => {}
                    Err(_) if *if_exists => {}
                    Err(e) => return Err(SqlError::from(e)),
                }
                Ok(ExecutionResult {
                    rows: vec![],
                    rows_affected: 0,
                    command: "DROP FUNCTION".to_string(),
                    col_names: vec![],
                    col_types: vec![],
                })
            }
            LogicalPlan::Explain { analyze, plan: inner_plan } => {
                let lines = format_plan(inner_plan, 0);
                let rows: Vec<Row> = lines.into_iter().map(|line| Row {
                    schema: vec![("QUERY PLAN".to_string(), catalog::DataType::Text)],
                    values: vec![Value::Text(line)],
                }).collect();
                let command = if *analyze { "EXPLAIN ANALYZE" } else { "EXPLAIN" };
                Ok(ExecutionResult {
                    rows_affected: 0,
                    command: command.to_string(),
                    col_names: vec!["QUERY PLAN".to_string()],
                    col_types: vec![catalog::DataType::Text],
                    rows,
                })
            }
            _ => {
                // Derive column names and types from the plan before executing (needed for 0-row results)
                let (plan_col_names_val, plan_col_types_val) = plan_col_names(&plan, &self.ctx.catalog);
                let rows = self.exec_plan(&plan)?;
                let count = rows.len() as u64;
                // If rows exist, prefer col names/types from the actual row schema
                let (col_names, col_types) = if !rows.is_empty() {
                    let names = rows[0].schema.iter().map(|(n, _)| n.clone()).collect();
                    let types = rows[0].schema.iter().map(|(_, dt)| dt.clone()).collect();
                    (names, types)
                } else {
                    (plan_col_names_val, plan_col_types_val)
                };
                Ok(ExecutionResult {
                    rows,
                    rows_affected: count,
                    command: format!("SELECT {count}"),
                    col_names,
                    col_types,
                })
            }
        }
    }

    fn exec_plan(&self, plan: &LogicalPlan) -> Result<Vec<Row>, SqlError> {
        match plan {
            LogicalPlan::TableScan {
                table_name,
                alias,
                schema,
                filter,
            } => self.exec_table_scan(table_name, alias.as_deref(), schema, filter),
            LogicalPlan::Filter { input, predicate } => {
                let rows = self.exec_plan(input)?;
                self.exec_filter(rows, predicate)
            }
            LogicalPlan::Project {
                input,
                columns,
                distinct,
            } => {
                let rows = self.exec_plan(input)?;
                let projected = self.exec_project(rows, columns)?;
                if *distinct {
                    Ok(deduplicate_rows(projected))
                } else {
                    Ok(projected)
                }
            }
            LogicalPlan::Join {
                left,
                right,
                join_type,
                condition,
                using_columns,
                algorithm,
            } => {
                let left_rows = self.exec_plan(left)?;
                let right_rows = self.exec_plan(right)?;
                self.exec_join(left_rows, right_rows, join_type.clone(), condition, using_columns, algorithm)
            }
            LogicalPlan::Aggregate {
                input,
                group_by,
                aggregates,
                having,
            } => {
                let rows = self.exec_plan(input)?;
                self.exec_aggregate(rows, group_by, aggregates, having.as_ref())
            }
            LogicalPlan::Sort { input, keys } => {
                let rows = self.exec_plan(input)?;
                self.exec_sort(rows, keys)
            }
            LogicalPlan::Limit {
                input,
                limit,
                offset,
            } => {
                let rows = self.exec_plan(input)?;
                Ok(self.exec_limit(rows, *limit, *offset))
            }
            LogicalPlan::IndexScan {
                table_name,
                schema,
                index_column,
                eq_value,
                range_start,
                range_end,
                filter,
            } => self.exec_index_scan(
                table_name,
                schema,
                index_column,
                eq_value,
                range_start,
                range_end,
                filter,
            ),
            LogicalPlan::SetOp {
                op,
                all,
                left,
                right,
            } => self.exec_set_op(op, *all, left, right),
            LogicalPlan::Cte { ctes, inner } => self.exec_cte(ctes, inner),
            LogicalPlan::Values { rows, schema } => {
                Ok(rows
                    .iter()
                    .map(|vals| Row::new(vals.clone(), schema.clone()))
                    .collect())
            }
            LogicalPlan::Window { input, window_exprs } => {
                let rows = self.exec_plan(input)?;
                self.exec_window(rows, window_exprs)
            }
            LogicalPlan::RecursiveCte {
                name,
                column_aliases,
                base_query,
                recursive_query,
                search_by_col,
                search_set_col,
                cycle_col,
                cycle_set_col,
                cycle_path_col,
            } => self.exec_recursive_cte(
                name,
                column_aliases,
                base_query,
                recursive_query,
                search_by_col.as_deref(),
                search_set_col.as_deref(),
                cycle_col.as_deref(),
                cycle_set_col.as_deref(),
                cycle_path_col.as_deref(),
            ),
            LogicalPlan::SystemCatalogScan { catalog_name, table_name, filter } => {
                self.exec_system_catalog_scan(catalog_name, table_name, filter.as_ref())
            }
            LogicalPlan::GenerateSeries { start, stop, step } => {
                self.exec_generate_series(start, stop, step)
            }
            LogicalPlan::Lateral { outer, subquery, alias } => {
                self.exec_lateral(outer, subquery, alias)
            }
            _ => Err(SqlError::Execution(format!(
                "cannot execute plan as query: {:?}",
                plan
            ))),
        }
    }

    /// Execute a LATERAL join: for each outer row, execute the subquery with the outer row
    /// as correlated context, then concatenate outer + inner row values.
    fn exec_lateral(
        &self,
        outer: &LogicalPlan,
        subquery: &LogicalPlan,
        alias: &str,
    ) -> Result<Vec<Row>, SqlError> {
        let outer_rows = self.exec_plan(outer)?;
        let mut result = Vec::new();
        for outer_row in &outer_rows {
            // Execute the subquery with this outer row as correlated context
            let inner_rows = self.exec_plan_correlated(subquery, outer_row)?;
            for inner_row in inner_rows {
                // Prefix inner columns with alias if alias is non-empty
                let inner_schema: Vec<(String, catalog::DataType)> = inner_row
                    .schema
                    .iter()
                    .map(|(col_name, col_type)| {
                        let qualified = if alias.is_empty() || col_name.contains('.') {
                            col_name.clone()
                        } else {
                            format!("{alias}.{col_name}")
                        };
                        (qualified, col_type.clone())
                    })
                    .collect();
                // Concatenate outer row + inner row
                let mut combined_schema = outer_row.schema.clone();
                combined_schema.extend(inner_schema);
                let mut combined_values = outer_row.values.clone();
                combined_values.extend(inner_row.values);
                result.push(Row::new(combined_values, combined_schema));
            }
        }
        Ok(result)
    }

    /// Execute a subquery plan with an optional outer row for correlated subquery resolution.
    /// When `outer_row` is provided, column references not found in the subquery's row schema
    /// will be looked up in the outer row.
    fn exec_plan_with_outer(
        &self,
        plan: &LogicalPlan,
        outer_row: Option<&Row>,
    ) -> Result<Vec<Row>, SqlError> {
        match outer_row {
            None => self.exec_plan(plan),
            Some(outer) => {
                // Rewrite the plan's column expressions by substituting outer row values
                // as literals for columns not found in the subquery schema.
                // We do this by executing the plan with filter evaluation that has access to outer row.
                self.exec_plan_correlated(plan, outer)
            }
        }
    }

    /// Execute a plan where column references that can't be resolved within the plan
    /// are resolved from the outer_row (correlated subquery evaluation).
    fn exec_plan_correlated(
        &self,
        plan: &LogicalPlan,
        outer_row: &Row,
    ) -> Result<Vec<Row>, SqlError> {
        match plan {
            LogicalPlan::TableScan { table_name, alias, schema, filter } => {
                // Check CTE context first: if this table name is a materialized CTE, use it.
                {
                    let cte_ctx = self.cte_context.borrow();
                    if let Some(cte_rows) = cte_ctx.get(table_name.as_str()) {
                        let rows: Vec<Row> = cte_rows
                            .iter()
                            .map(|r| qualify_row_schema(r.clone(), alias.as_deref()))
                            .collect();
                        let rows = if let Some(pred) = filter {
                            self.exec_filter_correlated(rows, pred, outer_row)?
                        } else {
                            rows
                        };
                        return Ok(rows);
                    }
                }
                // Execute the scan, then apply filter with outer row fallback
                let rows = self.exec_table_scan(table_name, alias.as_deref(), schema, &None)?;
                if let Some(pred) = filter {
                    self.exec_filter_correlated(rows, pred, outer_row)
                } else {
                    Ok(rows)
                }
            }
            LogicalPlan::Filter { input, predicate } => {
                let rows = self.exec_plan_correlated(input, outer_row)?;
                self.exec_filter_correlated(rows, predicate, outer_row)
            }
            LogicalPlan::Project { input, columns, distinct } => {
                let rows = self.exec_plan_correlated(input, outer_row)?;
                let projected = self.exec_project_correlated(rows, columns, outer_row)?;
                if *distinct {
                    Ok(deduplicate_rows(projected))
                } else {
                    Ok(projected)
                }
            }
            LogicalPlan::Aggregate { input, group_by, aggregates, having } => {
                let rows = self.exec_plan_correlated(input, outer_row)?;
                self.exec_aggregate(rows, group_by, aggregates, having.as_ref())
            }
            LogicalPlan::Sort { input, keys } => {
                let rows = self.exec_plan_correlated(input, outer_row)?;
                self.exec_sort(rows, keys)
            }
            LogicalPlan::Limit { input, limit, offset } => {
                let rows = self.exec_plan_correlated(input, outer_row)?;
                Ok(self.exec_limit(rows, *limit, *offset))
            }
            LogicalPlan::Join { left, right, join_type, condition, using_columns, algorithm } => {
                let left_rows = self.exec_plan_correlated(left, outer_row)?;
                let right_rows = self.exec_plan_correlated(right, outer_row)?;
                self.exec_join(left_rows, right_rows, join_type.clone(), condition, using_columns, algorithm)
            }
            LogicalPlan::Lateral { outer, subquery, alias } => {
                self.exec_lateral(outer, subquery, alias)
            }
            // Fall back to non-correlated for other plan types
            _ => self.exec_plan(plan),
        }
    }

    fn exec_filter_correlated(
        &self,
        rows: Vec<Row>,
        predicate: &Expr,
        outer_row: &Row,
    ) -> Result<Vec<Row>, SqlError> {
        let mut result = Vec::new();
        for row in rows {
            // Create a merged row for evaluation: inner row takes precedence
            let val = self.eval_expr_with_outer(predicate, &row, outer_row)?;
            match val {
                Value::Bool(true) => result.push(row),
                Value::Bool(false) | Value::Null => {}
                _ => {}
            }
        }
        Ok(result)
    }

    fn exec_project_correlated(
        &self,
        rows: Vec<Row>,
        columns: &[(String, Expr)],
        outer_row: &Row,
    ) -> Result<Vec<Row>, SqlError> {
        let mut result = Vec::new();
        for row in rows {
            let row_schema: Vec<(String, catalog::DataType)> = columns
                .iter()
                .map(|(name, expr)| (name.clone(), infer_expr_type(expr, &row.schema)))
                .collect();
            let mut new_values = Vec::new();
            for (_, expr) in columns {
                let val = self.eval_expr_with_outer(expr, &row, outer_row)?;
                new_values.push(val);
            }
            result.push(Row::new(new_values, row_schema));
        }
        Ok(result)
    }

    /// Evaluate an expression with fallback to outer_row for unresolved column references.
    fn eval_expr_with_outer(&self, expr: &Expr, row: &Row, outer_row: &Row) -> Result<Value, SqlError> {
        match expr {
            Expr::Column { table, name } => {
                // If there's a table qualifier, try qualified lookup in current row first
                if let Some(tbl) = table {
                    let qualified = format!("{tbl}.{name}");
                    if let Some(v) = row.get(&qualified) {
                        return Ok(v.clone());
                    }
                    // Try qualified in outer row
                    if let Some(v) = outer_row.get(&qualified) {
                        return Ok(v.clone());
                    }
                    // Try bare name with table qualifier — check if the table qualifier
                    // matches any alias in the current row schema
                    let in_current = row.schema.iter().any(|(cn, _)| {
                        cn.starts_with(&format!("{tbl}."))
                    });
                    if !in_current {
                        // Table qualifier doesn't match current row — must be outer
                        if let Some(v) = outer_row.get(name) {
                            return Ok(v.clone());
                        }
                        let outer_matches: Vec<_> = outer_row.schema.iter().enumerate()
                            .filter(|(_, (col_name, _))| col_name == name || col_name.ends_with(&format!(".{name}")))
                            .collect();
                        if !outer_matches.is_empty() {
                            return Ok(outer_row.values[outer_matches[0].0].clone());
                        }
                    }
                }

                // Try bare name in current row
                if let Some(v) = row.get(name) {
                    return Ok(v.clone());
                }
                // Check suffix matches in current row
                let matches: Vec<_> = row.schema.iter().enumerate()
                    .filter(|(_, (col_name, _))| col_name == name || col_name.ends_with(&format!(".{name}")))
                    .collect();
                if matches.len() == 1 {
                    return Ok(row.values[matches[0].0].clone());
                }
                if matches.len() > 1 {
                    // Ambiguous in current row - don't fall back
                    return Err(SqlError::AmbiguousColumn(name.clone()));
                }

                // Fall back to outer row (bare name)
                if let Some(v) = outer_row.get(name) {
                    return Ok(v.clone());
                }
                let outer_matches: Vec<_> = outer_row.schema.iter().enumerate()
                    .filter(|(_, (col_name, _))| col_name == name || col_name.ends_with(&format!(".{name}")))
                    .collect();
                match outer_matches.len() {
                    0 => Err(SqlError::ColumnNotFound(name.clone())),
                    1 => Ok(outer_row.values[outer_matches[0].0].clone()),
                    _ => Err(SqlError::AmbiguousColumn(name.clone())),
                }
            }
            // For all other expression types, recursively handle with outer row fallback
            Expr::BinaryOp { left, op, right } => {
                let l = self.eval_expr_with_outer(left, row, outer_row)?;
                let r = self.eval_expr_with_outer(right, row, outer_row)?;
                self.eval_binary_op(l, op, r)
            }
            Expr::UnaryOp { op, expr } => {
                let v = self.eval_expr_with_outer(expr, row, outer_row)?;
                match op {
                    UnaryOp::Neg => match v {
                        Value::Int4(i) => Ok(Value::Int4(-i)),
                        Value::Int8(i) => Ok(Value::Int8(-i)),
                        Value::Float8(f) => Ok(Value::Float8(-f)),
                        Value::Null => Ok(Value::Null),
                        _ => Err(SqlError::TypeError(format!("cannot negate {:?}", v))),
                    },
                    UnaryOp::Not => match v {
                        Value::Bool(b) => Ok(Value::Bool(!b)),
                        Value::Null => Ok(Value::Null),
                        _ => Err(SqlError::TypeError(format!("cannot NOT {:?}", v))),
                    },
                }
            }
            Expr::IsNull(inner) => {
                let v = self.eval_expr_with_outer(inner, row, outer_row)?;
                Ok(Value::Bool(matches!(v, Value::Null)))
            }
            Expr::IsNotNull(inner) => {
                let v = self.eval_expr_with_outer(inner, row, outer_row)?;
                Ok(Value::Bool(!matches!(v, Value::Null)))
            }
            // For complex expressions (subqueries etc), fall back to normal eval
            // (passing outer row context where possible)
            Expr::InSubquery { expr, subquery, negated } => {
                let val = self.eval_expr_with_outer(expr, row, outer_row)?;
                if matches!(val, Value::Null) {
                    return Ok(Value::Null);
                }
                // For nested correlated subqueries, use the inner row as outer context
                let merged = merge_rows(row, outer_row);
                let sub_rows = self.exec_plan_correlated(subquery, &merged)?;
                let found = sub_rows.iter().any(|r| {
                    if let Some(v) = r.get_by_idx(0) {
                        matches!(v.partial_cmp(&val), Some(std::cmp::Ordering::Equal))
                    } else {
                        false
                    }
                });
                if *negated { Ok(Value::Bool(!found)) } else { Ok(Value::Bool(found)) }
            }
            Expr::Exists { subquery, negated } => {
                let merged = merge_rows(row, outer_row);
                let sub_rows = self.exec_plan_correlated(subquery, &merged)?;
                let exists = !sub_rows.is_empty();
                if *negated { Ok(Value::Bool(!exists)) } else { Ok(Value::Bool(exists)) }
            }
            Expr::ScalarSubquery(subquery) => {
                let merged = merge_rows(row, outer_row);
                let sub_rows = self.exec_plan_correlated(subquery, &merged)?;
                if sub_rows.is_empty() {
                    return Ok(Value::Null);
                }
                if sub_rows.len() > 1 {
                    return Err(SqlError::Execution("scalar subquery returned more than one row".to_string()));
                }
                Ok(sub_rows[0].get_by_idx(0).cloned().unwrap_or(Value::Null))
            }
            // Fall back to normal eval for everything else
            other => self.eval_expr(other, row),
        }
    }

    fn exec_set_op(
        &self,
        op: &SetOperation,
        all: bool,
        left: &LogicalPlan,
        right: &LogicalPlan,
    ) -> Result<Vec<Row>, SqlError> {
        let left_rows = self.exec_plan(left)?;
        let right_rows = self.exec_plan(right)?;

        // Normalize all rows to use the left-side schema so that ORDER BY and DISTINCT
        // can find columns by name regardless of which branch they came from.
        let left_schema: Vec<(String, catalog::DataType)> = left_rows
            .first()
            .map(|r| r.schema.clone())
            .unwrap_or_default();
        let right_rows_normalized: Vec<Row> = right_rows
            .into_iter()
            .map(|mut r| {
                if !left_schema.is_empty() {
                    r.schema = left_schema.clone();
                }
                r
            })
            .collect();

        let result = match (op, all) {
            (SetOperation::Union, true) => {
                let mut out = left_rows;
                out.extend(right_rows_normalized);
                out
            }
            (SetOperation::Union, false) => {
                let mut out = left_rows;
                out.extend(right_rows_normalized);
                deduplicate_rows(out)
            }
            (SetOperation::Intersect, false) => {
                let right_keys: Vec<Vec<Value>> =
                    right_rows_normalized.iter().map(|r| r.values.clone()).collect();
                deduplicate_rows(left_rows.into_iter()
                    .filter(|lr| right_keys.contains(&lr.values))
                    .collect())
            }
            (SetOperation::Intersect, true) => {
                // INTERSECT ALL: keep min(count_left, count_right) copies
                let mut right_pool: Vec<Vec<Value>> =
                    right_rows_normalized.iter().map(|r| r.values.clone()).collect();
                let mut out = Vec::new();
                for lr in left_rows {
                    if let Some(pos) = right_pool.iter().position(|rv| rv == &lr.values) {
                        right_pool.remove(pos);
                        out.push(lr);
                    }
                }
                out
            }
            (SetOperation::Except, false) => {
                let right_keys: Vec<Vec<Value>> =
                    right_rows_normalized.iter().map(|r| r.values.clone()).collect();
                deduplicate_rows(left_rows.into_iter()
                    .filter(|lr| !right_keys.contains(&lr.values))
                    .collect())
            }
            (SetOperation::Except, true) => {
                // EXCEPT ALL: remove one copy of each right-side value from left
                let mut right_pool: Vec<Vec<Value>> =
                    right_rows_normalized.iter().map(|r| r.values.clone()).collect();
                let mut out = Vec::new();
                for lr in left_rows {
                    if let Some(pos) = right_pool.iter().position(|rv| rv == &lr.values) {
                        right_pool.remove(pos); // consume one copy
                    } else {
                        out.push(lr);
                    }
                }
                out
            }
        };

        Ok(result)
    }

    fn exec_cte(
        &self,
        ctes: &[(String, Box<LogicalPlan>)],
        inner: &LogicalPlan,
    ) -> Result<Vec<Row>, SqlError> {
        // Materialize each CTE in order, passing the accumulated CTE map to each one
        // so that later CTEs can reference earlier CTEs (chained CTE support).
        let mut cte_map: HashMap<String, Vec<Row>> = HashMap::new();
        for (name, cte_plan) in ctes {
            let rows = self.exec_plan_with_ctes(cte_plan, &cte_map)?;
            // For CTEs whose top-level plan is a Project over a non-aggregate input,
            // supplement projected rows with all additional columns from the source.
            // This allows outer queries to reference source columns not listed in the
            // CTE's SELECT list (e.g. JOIN conditions that use unlisted columns).
            let rows = self.supplement_cte_rows_from_source(cte_plan, rows, &cte_map)?;
            cte_map.insert(name.clone(), rows);
        }
        // Make the CTE map available to subquery eval (InSubquery, Exists, ScalarSubquery)
        // by merging it into the executor's CTE context.
        let prev_ctx = {
            let mut ctx = self.cte_context.borrow_mut();
            let old = ctx.clone();
            ctx.extend(cte_map.clone());
            old
        };
        let result = self.exec_plan_with_ctes(inner, &cte_map);
        // Restore previous CTE context
        *self.cte_context.borrow_mut() = prev_ctx;
        result
    }

    /// Supplement CTE rows with extra columns from the source plan.
    /// When a CTE body is `Project { input }` and the input is not an Aggregate,
    /// execute the input plan to get all source columns, then append any columns
    /// that are not already in the projected rows. This ensures that outer queries
    /// can reference source columns not explicitly listed in the CTE's SELECT.
    fn supplement_cte_rows_from_source(
        &self,
        cte_plan: &LogicalPlan,
        projected_rows: Vec<Row>,
        cte_map: &HashMap<String, Vec<Row>>,
    ) -> Result<Vec<Row>, SqlError> {
        // Only supplement for Project plans whose input is not an Aggregate
        let input = match cte_plan {
            LogicalPlan::Project { input, .. } => input.as_ref(),
            _ => return Ok(projected_rows),
        };
        // Don't supplement if input contains aggregation (aggregate outputs are complete)
        if plan_contains_aggregate(input) {
            return Ok(projected_rows);
        }
        // Execute the input plan to get all source columns
        let source_rows = self.exec_plan_with_ctes(input, cte_map)?;
        if source_rows.len() != projected_rows.len() {
            // If row counts differ, can't safely merge — return projected rows as-is
            return Ok(projected_rows);
        }
        // Merge: for each projected row, append columns from source row that are not
        // already present in the projected row.
        let merged: Vec<Row> = projected_rows
            .into_iter()
            .zip(source_rows)
            .map(|(proj_row, src_row)| {
                let mut new_schema = proj_row.schema.clone();
                let mut new_values = proj_row.values.clone();
                // Add extra columns from source that aren't in the projected row
                let proj_names: std::collections::HashSet<&str> =
                    proj_row.schema.iter().map(|(n, _)| n.as_str()).collect();
                for ((col_name, col_type), val) in
                    src_row.schema.iter().zip(src_row.values.iter())
                {
                    if !proj_names.contains(col_name.as_str()) {
                        new_schema.push((col_name.clone(), col_type.clone()));
                        new_values.push(val.clone());
                    }
                }
                Row::new(new_values, new_schema)
            })
            .collect();
        Ok(merged)
    }

    fn exec_plan_with_ctes(
        &self,
        plan: &LogicalPlan,
        cte_map: &HashMap<String, Vec<Row>>,
    ) -> Result<Vec<Row>, SqlError> {
        match plan {
            LogicalPlan::TableScan { table_name, alias, .. } => {
                if let Some(rows) = cte_map.get(table_name) {
                    let rows = rows.iter().map(|r| qualify_row_schema(r.clone(), alias.as_deref())).collect();
                    return Ok(rows);
                }
                // Fall through to regular exec
                self.exec_plan(plan)
            }
            LogicalPlan::Filter { input, predicate } => {
                let rows = self.exec_plan_with_ctes(input, cte_map)?;
                self.exec_filter(rows, predicate)
            }
            LogicalPlan::Project { input, columns, distinct } => {
                let rows = self.exec_plan_with_ctes(input, cte_map)?;
                let projected = self.exec_project(rows, columns)?;
                if *distinct {
                    Ok(deduplicate_rows(projected))
                } else {
                    Ok(projected)
                }
            }
            LogicalPlan::Join { left, right, join_type, condition, using_columns, algorithm } => {
                let left_rows = self.exec_plan_with_ctes(left, cte_map)?;
                let right_rows = self.exec_plan_with_ctes(right, cte_map)?;
                self.exec_join(left_rows, right_rows, join_type.clone(), condition, using_columns, algorithm)
            }
            LogicalPlan::Aggregate { input, group_by, aggregates, having } => {
                let rows = self.exec_plan_with_ctes(input, cte_map)?;
                self.exec_aggregate(rows, group_by, aggregates, having.as_ref())
            }
            LogicalPlan::Sort { input, keys } => {
                let rows = self.exec_plan_with_ctes(input, cte_map)?;
                self.exec_sort(rows, keys)
            }
            LogicalPlan::Limit { input, limit, offset } => {
                let rows = self.exec_plan_with_ctes(input, cte_map)?;
                Ok(self.exec_limit(rows, *limit, *offset))
            }
            LogicalPlan::SetOp { op, all, left, right } => {
                let left_rows = self.exec_plan_with_ctes(left, cte_map)?;
                let right_rows = self.exec_plan_with_ctes(right, cte_map)?;
                // Normalize right-side rows to use left-side schema (UNION column naming)
                let left_schema: Vec<(String, catalog::DataType)> = left_rows
                    .first()
                    .map(|r| r.schema.clone())
                    .unwrap_or_default();
                let right_rows_norm: Vec<Row> = right_rows
                    .into_iter()
                    .map(|mut r| {
                        if !left_schema.is_empty() { r.schema = left_schema.clone(); }
                        r
                    })
                    .collect();
                let result = match op {
                    SetOperation::Union => {
                        let mut out = left_rows;
                        out.extend(right_rows_norm);
                        out
                    }
                    SetOperation::Intersect => {
                        let right_keys: Vec<Vec<Value>> =
                            right_rows_norm.iter().map(|r| r.values.clone()).collect();
                        left_rows.into_iter().filter(|lr| right_keys.contains(&lr.values)).collect()
                    }
                    SetOperation::Except => {
                        let right_keys: Vec<Vec<Value>> =
                            right_rows_norm.iter().map(|r| r.values.clone()).collect();
                        left_rows.into_iter().filter(|lr| !right_keys.contains(&lr.values)).collect()
                    }
                };
                if *all { Ok(result) } else { Ok(deduplicate_rows(result)) }
            }
            LogicalPlan::Cte { ctes, inner } => {
                // Merge outer CTEs with inner CTEs
                let mut merged = cte_map.clone();
                for (name, cte_plan) in ctes {
                    let rows = self.exec_plan_with_ctes(cte_plan, &merged)?;
                    merged.insert(name.clone(), rows);
                }
                self.exec_plan_with_ctes(inner, &merged)
            }
            LogicalPlan::Window { input, window_exprs } => {
                let rows = self.exec_plan_with_ctes(input, cte_map)?;
                self.exec_window(rows, window_exprs)
            }
            LogicalPlan::RecursiveCte {
                name,
                column_aliases,
                base_query,
                recursive_query,
                search_by_col,
                search_set_col,
                cycle_col,
                cycle_set_col,
                cycle_path_col,
            } => self.exec_recursive_cte(
                name,
                column_aliases,
                base_query,
                recursive_query,
                search_by_col.as_deref(),
                search_set_col.as_deref(),
                cycle_col.as_deref(),
                cycle_set_col.as_deref(),
                cycle_path_col.as_deref(),
            ),
            _ => self.exec_plan(plan),
        }
    }

    fn exec_table_scan(
        &self,
        table_name: &str,
        alias: Option<&str>,
        schema: &catalog::schema::TableSchema,
        filter: &Option<Expr>,
    ) -> Result<Vec<Row>, SqlError> {
        // Special case: __dual__ is a virtual single-row table with no columns
        if table_name == "__dual__" {
            let row = Row::new(vec![], vec![]);
            return Ok(vec![row]);
        }

        // Look up actual table OID from schema (it's already in schema)
        let table_oid = schema.oid;
        let mut heap = self.open_heap(table_oid)?;

        let visible = self
            .ctx
            .txn_manager
            .scan_visible_tuples(self.ctx.xid, &mut heap)?;

        let mut rows = Vec::new();
        for (_tid, tuple) in visible {
            // The null bitmap is stored in t_bits - but t_bits is only 1 byte
            // Our codec uses a 4-byte null bitmap stored at the start of data
            // We need to read the null bitmap from the data
            let data = &tuple.data;
            if data.len() < 4 {
                // Empty row or schema with no columns
                let row = decode_row(data, 0, schema)?;
                let row = qualify_row_schema(row, alias);
                rows.push(row);
                continue;
            }
            // First 4 bytes of data are the null bitmap
            let null_bitmap = u32::from_le_bytes(data[0..4].try_into().unwrap());
            let row_data = &data[4..];
            let row = decode_row(row_data, null_bitmap, schema)?;
            let row = qualify_row_schema(row, alias);
            rows.push(row);
        }

        if let Some(pred) = filter {
            rows = self.exec_filter(rows, pred)?;
        }

        Ok(rows)
    }

    fn exec_insert(
        &self,
        _table_name: &str,
        schema: &catalog::schema::TableSchema,
        columns: &[String],
        source: &InsertSource,
        returning: &[(String, Expr)],
        on_conflict: Option<&crate::plan::OnConflict>,
    ) -> Result<(u64, Vec<Row>), SqlError> {
        let table_oid = schema.oid;
        let mut heap = self.open_heap(table_oid)?;

        // For INSERT INTO ... SELECT ..., execute the query and convert rows to values
        let owned_value_rows: Vec<Vec<Expr>>;
        let value_rows: &Vec<Vec<Expr>> = match source {
            InsertSource::Values(vr) => vr,
            InsertSource::Query(query_plan) => {
                let query_rows = self.exec_plan(query_plan)?;
                owned_value_rows = query_rows
                    .into_iter()
                    .map(|row| {
                        row.values
                            .into_iter()
                            .map(Expr::Literal)
                            .collect()
                    })
                    .collect();
                &owned_value_rows
            }
        };
        let mut count = 0u64;
        let mut returning_rows: Vec<Row> = Vec::new();

        for expr_row in value_rows {
            // Determine column order: if columns is empty, use schema order
            let col_names: Vec<String> = if columns.is_empty() {
                schema.columns.iter().map(|c| c.name.clone()).collect()
            } else {
                columns.to_vec()
            };

            if col_names.len() != expr_row.len() {
                return Err(SqlError::Execution(format!(
                    "INSERT: column count mismatch ({} columns, {} values)",
                    col_names.len(),
                    expr_row.len()
                )));
            }

            // Build a full-schema row, filling unspecified columns with Null/default/serial
            let mut values = vec![Value::Null; schema.columns.len()];
            // Track which columns were explicitly provided
            let mut provided = vec![false; schema.columns.len()];

            for (col_name, expr) in col_names.iter().zip(expr_row.iter()) {
                let val = self.eval_expr(expr, &Row::new(vec![], vec![]))?;
                // Find column index in schema
                let idx = schema
                    .columns
                    .iter()
                    .position(|c| c.name == *col_name)
                    .ok_or_else(|| SqlError::ColumnNotFound(col_name.clone()))?;
                // Cast value to column type
                let col_type = &schema.columns[idx].data_type;
                let casted = if matches!(val, Value::Null) {
                    val
                } else {
                    val.cast_to(col_type)?
                };
                values[idx] = casted;
                provided[idx] = true;
            }

            // For missing columns, fill in serial/default values
            for (idx, col) in schema.columns.iter().enumerate() {
                if provided[idx] {
                    continue;
                }
                if col.serial {
                    // Get next sequence value
                    let seq_val = self.ctx.catalog
                        .next_sequence_val("public", &schema.name, &col.name)
                        .map_err(|e| SqlError::Execution(format!("sequence error: {e}")))?;
                    let v = match &col.data_type {
                        catalog::DataType::Int8 => Value::Int8(seq_val),
                        _ => Value::Int4(seq_val as i32),
                    };
                    values[idx] = v;
                } else if let Some(default_str) = &col.default_expr {
                    // Evaluate the default expression
                    let val = self.eval_default_expr(default_str, &col.data_type)?;
                    values[idx] = val;
                }
                // else: stays Null
            }

            // Check NOT NULL constraints
            for (i, col) in schema.columns.iter().enumerate() {
                if col.not_null && matches!(values[i], Value::Null) {
                    return Err(SqlError::ConstraintViolation(format!(
                        "column '{}' violates NOT NULL constraint",
                        col.name
                    )));
                }
            }

            // Check FK constraints (INSERT: referenced row must exist)
            let fks = self.ctx.catalog.get_foreign_keys(schema.oid);
            for fk in &fks {
                if let Some(local_idx) = schema.columns.iter().position(|c| c.name == fk.local_col) {
                    let fk_val = &values[local_idx];
                    if matches!(fk_val, Value::Null) {
                        continue; // NULL FK values are allowed
                    }
                    // Check that the referenced row exists
                    if let Ok(ref_schema) = self.ctx.catalog.get_table("public", &fk.ref_table) {
                        let ref_rows = self.exec_table_scan(&fk.ref_table, None, &ref_schema, &None)?;
                        let found = ref_rows.iter().any(|row| {
                            if let Some(pos) = ref_schema.columns.iter().position(|c| c.name == fk.ref_col) {
                                row.get_by_idx(pos).map(|v| v.partial_cmp(fk_val) == Some(std::cmp::Ordering::Equal)).unwrap_or(false)
                            } else {
                                false
                            }
                        });
                        if !found {
                            return Err(SqlError::ConstraintViolation(format!(
                                "Foreign key violation: no row in {} where {} = {:?}",
                                fk.ref_table, fk.ref_col, fk_val
                            )));
                        }
                    }
                }
            }

            // Check CHECK constraints
            let checks = self.ctx.catalog.get_check_constraints(schema.oid);
            let row_schema: Vec<(String, catalog::DataType)> = schema
                .columns
                .iter()
                .map(|c| (c.name.clone(), c.data_type.clone()))
                .collect();
            let check_row = Row::new(values.clone(), row_schema.clone());
            for check in &checks {
                let check_result = self.eval_check_constraint(&check.expr, &check_row)?;
                if !check_result {
                    let name_part = check.name.as_deref().unwrap_or("unnamed");
                    return Err(SqlError::ConstraintViolation(format!(
                        "Check constraint '{}' violated: {}",
                        name_part, check.expr
                    )));
                }
            }

            // Check UNIQUE / PRIMARY KEY constraints
            let table_oid = schema.oid;
            let mut skip_row = false;
            let mut conflict_updated = false;
            if let Some(constraints) = self.ctx.catalog.get_unique_constraints(table_oid) {
                'col_loop: for col_name in &constraints.unique_columns {
                    if let Some(col_idx) = schema.columns.iter().position(|c| &c.name == col_name) {
                        let new_val = &values[col_idx];
                        if matches!(new_val, Value::Null) {
                            continue; // NULLs don't violate UNIQUE
                        }
                        // Scan existing rows
                        let existing_rows = self.exec_table_scan(&schema.name, None, schema, &None)?;
                        for row in &existing_rows {
                            if let Some(existing_val) = row.get_by_idx(col_idx) {
                                if !matches!(existing_val, Value::Null) && existing_val.partial_cmp(new_val) == Some(std::cmp::Ordering::Equal) {
                                    // Handle ON CONFLICT
                                    if let Some(oc) = on_conflict {
                                        match &oc.action {
                                            crate::plan::OnConflictAction::DoNothing => {
                                                // Skip this row silently
                                                skip_row = true;
                                                break 'col_loop;
                                            }
                                            crate::plan::OnConflictAction::DoUpdate { assignments } => {
                                                // Update the conflicting row
                                                let mut updated_row = row.clone();
                                                for (assign_col, assign_expr) in assignments {
                                                    let new_val = self.eval_expr(assign_expr, &updated_row)?;
                                                    if let Some(idx) = schema.columns.iter().position(|c| &c.name == assign_col) {
                                                        updated_row.values[idx] = new_val;
                                                    }
                                                }
                                                // Find the TID of the conflicting row and update it
                                                let visible = self.ctx.txn_manager.scan_visible_tuples(self.ctx.xid, &mut heap)?;
                                                for (tid, tuple) in &visible {
                                                    let tdata = &tuple.data;
                                                    let nb = if tdata.len() >= 4 { u32::from_le_bytes(tdata[0..4].try_into().unwrap()) } else { 0 };
                                                    let rd = if tdata.len() >= 4 { &tdata[4..] } else { tdata };
                                                    if let Ok(existing) = crate::codec::decode_row(rd, nb, schema) {
                                                        if existing.values == row.values {
                                                            let (new_data, new_bitmap) = encode_row(&updated_row);
                                                            let mut full = new_bitmap.to_le_bytes().to_vec();
                                                            full.extend_from_slice(&new_data);
                                                            let new_tid = self.ctx.txn_manager.update_tuple(self.ctx.xid, &mut heap, *tid, &full)?;
                                                            self.record_undo(crate::subtxn::UndoEntry::Delete { table_oid, tid: *tid });
                                                            self.record_undo(crate::subtxn::UndoEntry::Insert { table_oid, tid: new_tid });
                                                            count += 1;
                                                            if !returning.is_empty() {
                                                                returning_rows.push(self.build_returning_row(&updated_row, returning)?);
                                                            }
                                                            break;
                                                        }
                                                    }
                                                }
                                                // Skip the normal insert path
                                                conflict_updated = true;
                                                break 'col_loop;
                                            }
                                        }
                                    }
                                    let is_pk = constraints.primary_key.as_deref() == Some(col_name.as_str());
                                    let msg = if is_pk {
                                        format!("duplicate key value violates primary key constraint on column '{}'", col_name)
                                    } else {
                                        format!("duplicate key value violates unique constraint on column '{}'", col_name)
                                    };
                                    return Err(SqlError::UniqueViolation(msg));
                                }
                            }
                        }
                    }
                }
            }

            // Skip this row if ON CONFLICT DO NOTHING or DO UPDATE handled it
            if skip_row || conflict_updated {
                continue;
            }

            let row = Row::new(values, row_schema);

            // Encode row
            let (data, null_bitmap) = encode_row(&row);
            // Prepend null bitmap to data
            let mut full_data = null_bitmap.to_le_bytes().to_vec();
            full_data.extend_from_slice(&data);

            let tid = self.ctx
                .txn_manager
                .insert_tuple(self.ctx.xid, &mut heap, &full_data)?;
            self.record_undo(crate::subtxn::UndoEntry::Insert { table_oid, tid });
            count += 1;

            // Build RETURNING row
            if !returning.is_empty() {
                returning_rows.push(self.build_returning_row(&row, returning)?);
            }
        }

        Ok((count, returning_rows))
    }

    fn exec_update(
        &self,
        _table_name: &str,
        schema: &catalog::schema::TableSchema,
        assignments: &[(String, Expr)],
        filter: &Option<Expr>,
        returning: &[(String, Expr)],
        from_plan: Option<&LogicalPlan>,
    ) -> Result<(u64, Vec<Row>), SqlError> {
        let table_oid = schema.oid;
        let mut heap = self.open_heap(table_oid)?;

        let visible = self
            .ctx
            .txn_manager
            .scan_visible_tuples(self.ctx.xid, &mut heap)?;

        // Materialize FROM rows if UPDATE ... FROM is used
        let from_rows: Option<Vec<Row>> = if let Some(fp) = from_plan {
            Some(self.exec_plan(fp)?)
        } else {
            None
        };

        let mut to_update = Vec::new();
        for (tid, tuple) in visible {
            let data = &tuple.data;
            let null_bitmap = if data.len() >= 4 {
                u32::from_le_bytes(data[0..4].try_into().unwrap())
            } else {
                0
            };
            let row_data = if data.len() >= 4 { &data[4..] } else { data };
            let row = decode_row(row_data, null_bitmap, schema)?;

            if let Some(ref fr) = from_rows {
                // Cross-join target row with each from_row and check filter
                let mut matched = false;
                for from_row in fr {
                    let combined = merge_rows(&row, from_row);
                    let include = if let Some(pred) = filter {
                        matches!(self.eval_expr(pred, &combined)?, Value::Bool(true))
                    } else {
                        true
                    };
                    if include {
                        matched = true;
                        break;
                    }
                }
                if matched {
                    to_update.push((tid, row));
                }
            } else {
                let include = if let Some(pred) = filter {
                    matches!(self.eval_expr(pred, &row)?, Value::Bool(true))
                } else {
                    true
                };
                if include {
                    to_update.push((tid, row));
                }
            }
        }

        let count = to_update.len() as u64;
        let mut returning_rows: Vec<Row> = Vec::new();

        for (tid, mut row) in to_update {
            // Apply assignments
            for (col_name, expr) in assignments {
                let new_val = self.eval_expr(expr, &row)?;
                // Find column index
                let idx = schema
                    .columns
                    .iter()
                    .position(|c| c.name == *col_name)
                    .ok_or_else(|| SqlError::ColumnNotFound(col_name.clone()))?;
                let col_type = &schema.columns[idx].data_type;
                let casted = if matches!(new_val, Value::Null) {
                    new_val
                } else {
                    new_val.cast_to(col_type)?
                };
                row.values[idx] = casted;
            }

            // Check CHECK constraints after applying updates
            let checks = self.ctx.catalog.get_check_constraints(schema.oid);
            for check in &checks {
                let check_result = self.eval_check_constraint(&check.expr, &row)?;
                if !check_result {
                    let name_part = check.name.as_deref().unwrap_or("unnamed");
                    return Err(SqlError::ConstraintViolation(format!(
                        "Check constraint '{}' violated: {}",
                        name_part, check.expr
                    )));
                }
            }

            // Check FK constraints for updated FK columns
            let fks = self.ctx.catalog.get_foreign_keys(schema.oid);
            for fk in &fks {
                if let Some(local_idx) = schema.columns.iter().position(|c| c.name == fk.local_col) {
                    let fk_val = &row.values[local_idx];
                    if matches!(fk_val, Value::Null) { continue; }
                    if let Ok(ref_schema) = self.ctx.catalog.get_table("public", &fk.ref_table) {
                        let ref_rows = self.exec_table_scan(&fk.ref_table, None, &ref_schema, &None)?;
                        let found = ref_rows.iter().any(|rrow| {
                            if let Some(pos) = ref_schema.columns.iter().position(|c| c.name == fk.ref_col) {
                                rrow.get_by_idx(pos).map(|v| v.partial_cmp(fk_val) == Some(std::cmp::Ordering::Equal)).unwrap_or(false)
                            } else { false }
                        });
                        if !found {
                            return Err(SqlError::ConstraintViolation(format!(
                                "Foreign key violation: no row in {} where {} = {:?}",
                                fk.ref_table, fk.ref_col, fk_val
                            )));
                        }
                    }
                }
            }

            let (data, null_bitmap) = encode_row(&row);
            let mut full_data = null_bitmap.to_le_bytes().to_vec();
            full_data.extend_from_slice(&data);

            let new_tid = self.ctx
                .txn_manager
                .update_tuple(self.ctx.xid, &mut heap, tid, &full_data)?;
            self.record_undo(crate::subtxn::UndoEntry::Delete { table_oid: schema.oid, tid });
            self.record_undo(crate::subtxn::UndoEntry::Insert { table_oid: schema.oid, tid: new_tid });

            if !returning.is_empty() {
                returning_rows.push(self.build_returning_row(&row, returning)?);
            }
        }

        Ok((count, returning_rows))
    }

    fn exec_delete(
        &self,
        _table_name: &str,
        schema: &catalog::schema::TableSchema,
        filter: &Option<Expr>,
        returning: &[(String, Expr)],
        using_plan: Option<&LogicalPlan>,
    ) -> Result<(u64, Vec<Row>), SqlError> {
        let table_oid = schema.oid;
        let mut heap = self.open_heap(table_oid)?;

        let visible = self
            .ctx
            .txn_manager
            .scan_visible_tuples(self.ctx.xid, &mut heap)?;

        // Materialize USING rows if DELETE ... USING is used
        let using_rows: Option<Vec<Row>> = if let Some(up) = using_plan {
            Some(self.exec_plan(up)?)
        } else {
            None
        };

        let mut to_delete: Vec<(storage::tid::TID, Row)> = Vec::new();
        for (tid, tuple) in visible {
            let data = &tuple.data;
            let null_bitmap = if data.len() >= 4 {
                u32::from_le_bytes(data[0..4].try_into().unwrap())
            } else {
                0
            };
            let row_data = if data.len() >= 4 { &data[4..] } else { data };
            let row = decode_row(row_data, null_bitmap, schema)?;

            if let Some(ref ur) = using_rows {
                // Cross-join target row with each using_row and check filter
                let mut matched = false;
                for using_row in ur {
                    let combined = merge_rows(&row, using_row);
                    let include = if let Some(pred) = filter {
                        matches!(self.eval_expr(pred, &combined)?, Value::Bool(true))
                    } else {
                        true
                    };
                    if include {
                        matched = true;
                        break;
                    }
                }
                if matched {
                    to_delete.push((tid, row));
                }
            } else {
                let include = if let Some(pred) = filter {
                    matches!(self.eval_expr(pred, &row)?, Value::Bool(true))
                } else {
                    true
                };
                if include {
                    to_delete.push((tid, row));
                }
            }
        }

        let count = to_delete.len() as u64;
        let mut returning_rows: Vec<Row> = Vec::new();

        for (tid, row) in to_delete {
            // Check FK constraints: scan tables that reference this table's columns
            // Find the PK column (or any unique column) that might be referenced
            let table_oid = schema.oid;
            if let Some(unique_constraints) = self.ctx.catalog.get_unique_constraints(table_oid) {
                let pk_col = unique_constraints.primary_key.as_deref();
                if let Some(pk) = pk_col {
                    let referencing = self.ctx.catalog.find_referencing_tables("public", &schema.name, pk);
                    for (child_table_name, child_fk) in &referencing {
                        if let Ok(child_schema) = self.ctx.catalog.get_table("public", child_table_name) {
                            // Get the value of the PK column in the row being deleted
                            if let Some(pk_idx) = schema.columns.iter().position(|c| c.name == pk) {
                                let pk_val = &row.values[pk_idx];
                                // Check if any child row references this value
                                let child_rows = self.exec_table_scan(child_table_name, None, &child_schema, &None)?;
                                if let Some(child_col_idx) = child_schema.columns.iter().position(|c| c.name == child_fk.local_col) {
                                    for child_row in &child_rows {
                                        if let Some(child_val) = child_row.get_by_idx(child_col_idx) {
                                            if !matches!(child_val, Value::Null) && child_val.partial_cmp(pk_val) == Some(std::cmp::Ordering::Equal) {
                                                match &child_fk.on_delete {
                                                    catalog::schema::FkAction::Cascade => {
                                                        // Delete the child rows too
                                                        let mut child_heap = self.open_heap(child_schema.oid)?;
                                                        let child_visible = self.ctx.txn_manager.scan_visible_tuples(self.ctx.xid, &mut child_heap)?;
                                                        for (child_tid, child_tuple) in &child_visible {
                                                            let cd = &child_tuple.data;
                                                            let nb = if cd.len() >= 4 { u32::from_le_bytes(cd[0..4].try_into().unwrap()) } else { 0 };
                                                            let rd = if cd.len() >= 4 { &cd[4..] } else { cd };
                                                            if let Ok(cr) = crate::codec::decode_row(rd, nb, &child_schema) {
                                                                if cr.values == child_row.values && self.ctx.txn_manager.delete_tuple(self.ctx.xid, &mut child_heap, *child_tid).is_ok() {
                                                                        self.record_undo(crate::subtxn::UndoEntry::Delete { table_oid: child_schema.oid, tid: *child_tid });
                                                                }
                                                            }
                                                        }
                                                    }
                                                    catalog::schema::FkAction::SetNull | catalog::schema::FkAction::SetDefault => {
                                                        // UPDATE child row: set FK column to NULL
                                                        let mut child_heap = self.open_heap(child_schema.oid)?;
                                                        let child_visible2 = self.ctx.txn_manager.scan_visible_tuples(self.ctx.xid, &mut child_heap)?;
                                                        for (child_tid2, child_tuple2) in &child_visible2 {
                                                            let cd2 = &child_tuple2.data;
                                                            let nb2 = if cd2.len() >= 4 { u32::from_le_bytes(cd2[0..4].try_into().unwrap()) } else { 0 };
                                                            let rd2 = if cd2.len() >= 4 { &cd2[4..] } else { cd2 };
                                                            if let Ok(mut cr2) = crate::codec::decode_row(rd2, nb2, &child_schema) {
                                                                if cr2.values == child_row.values {
                                                                    if let Some(fk_col_idx) = child_schema.columns.iter().position(|c| c.name == child_fk.local_col) {
                                                                        cr2.values[fk_col_idx] = Value::Null;
                                                                        let (enc, nbm) = encode_row(&cr2);
                                                                        let mut data = nbm.to_le_bytes().to_vec();
                                                                        data.extend_from_slice(&enc);
                                                                        if let Ok(new_tid2) = self.ctx.txn_manager.update_tuple(self.ctx.xid, &mut child_heap, *child_tid2, &data) {
                                                                            self.record_undo(crate::subtxn::UndoEntry::Delete { table_oid: child_schema.oid, tid: *child_tid2 });
                                                                            self.record_undo(crate::subtxn::UndoEntry::Insert { table_oid: child_schema.oid, tid: new_tid2 });
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                    _ => {
                                                        // NoAction / Restrict
                                                        return Err(SqlError::ConstraintViolation(format!(
                                                            "Foreign key violation: row in {} references {}",
                                                            child_table_name, schema.name
                                                        )));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            self.ctx
                .txn_manager
                .delete_tuple(self.ctx.xid, &mut heap, tid)?;
            self.record_undo(crate::subtxn::UndoEntry::Delete { table_oid, tid });

            if !returning.is_empty() {
                returning_rows.push(self.build_returning_row(&row, returning)?);
            }
        }

        Ok((count, returning_rows))
    }

    fn build_returning_row(
        &self,
        row: &Row,
        returning: &[(String, Expr)],
    ) -> Result<Row, SqlError> {
        let mut values = Vec::new();
        let mut schema = Vec::new();
        for (name, expr) in returning {
            let val = self.eval_expr(expr, row)?;
            let dtype = infer_expr_type(expr, &row.schema);
            values.push(val);
            schema.push((name.clone(), dtype));
        }
        Ok(Row::new(values, schema))
    }

    #[allow(clippy::too_many_arguments)]
    fn exec_create_table(
        &self,
        schema_name: &str,
        table_name: &str,
        columns: &[catalog::schema::ColumnDef],
        if_not_exists: bool,
        primary_key: Option<&str>,
        unique_columns: &[String],
        foreign_keys: &[catalog::schema::TableForeignKey],
        check_constraints: &[catalog::schema::CheckConstraint],
    ) -> Result<(), SqlError> {
        match self
            .ctx
            .catalog
            .create_table(self.ctx.xid, schema_name, table_name, columns.to_vec())
        {
            Ok(oid) => {
                // Register unique/pk constraints in the catalog
                if !unique_columns.is_empty() || primary_key.is_some() {
                    let all_unique: Vec<String> = unique_columns.to_vec();
                    self.ctx.catalog.set_unique_constraints(oid, all_unique, primary_key.map(|s| s.to_string()));
                }
                // Create sequences for SERIAL columns
                for col in columns {
                    if col.serial {
                        let _ = self.ctx.catalog.create_sequence(schema_name, table_name, &col.name, 1);
                    }
                }
                // Register FK constraints
                if !foreign_keys.is_empty() {
                    self.ctx.catalog.set_foreign_keys(oid, foreign_keys.to_vec());
                }
                // Register CHECK constraints
                if !check_constraints.is_empty() {
                    self.ctx.catalog.set_check_constraints(oid, check_constraints.to_vec());
                }
                Ok(())
            }
            Err(catalog::error::CatalogError::DuplicateTable(_)) if if_not_exists => Ok(()),
            Err(e) => Err(SqlError::Catalog(e)),
        }
    }

    fn exec_drop_table(
        &self,
        schema_name: &str,
        table_name: &str,
        if_exists: bool,
    ) -> Result<(), SqlError> {
        match self
            .ctx
            .catalog
            .drop_table(self.ctx.xid, schema_name, table_name)
        {
            Ok(_) => Ok(()),
            Err(catalog::error::CatalogError::TableNotFound(_)) if if_exists => Ok(()),
            Err(e) => Err(SqlError::Catalog(e)),
        }
    }

    fn exec_project(
        &self,
        rows: Vec<Row>,
        columns: &[(String, Expr)],
    ) -> Result<Vec<Row>, SqlError> {
        let mut result = Vec::new();
        for row in rows {
            let row_schema: Vec<(String, catalog::DataType)> = columns
                .iter()
                .map(|(name, expr)| (name.clone(), infer_expr_type(expr, &row.schema)))
                .collect();
            let mut new_values = Vec::new();
            for (_, expr) in columns {
                let val = self.eval_expr(expr, &row)?;
                new_values.push(val);
            }
            result.push(Row::new(new_values, row_schema));
        }
        Ok(result)
    }

    fn exec_filter(&self, rows: Vec<Row>, predicate: &Expr) -> Result<Vec<Row>, SqlError> {
        let mut result = Vec::new();
        for row in rows {
            let val = self.eval_expr(predicate, &row)?;
            match val {
                Value::Bool(true) => result.push(row),
                Value::Bool(false) | Value::Null => {}
                _ => {} // non-boolean result means false
            }
        }
        Ok(result)
    }

    fn exec_sort(&self, mut rows: Vec<Row>, keys: &[SortKey]) -> Result<Vec<Row>, SqlError> {
        let mut errors: Vec<SqlError> = Vec::new();

        rows.sort_by(|a, b| {
            if !errors.is_empty() {
                return std::cmp::Ordering::Equal;
            }
            for key in keys {
                let va = match self.eval_expr(&key.expr, a) {
                    Ok(v) => v,
                    Err(e) => {
                        errors.push(e);
                        return std::cmp::Ordering::Equal;
                    }
                };
                let vb = match self.eval_expr(&key.expr, b) {
                    Ok(v) => v,
                    Err(e) => {
                        errors.push(e);
                        return std::cmp::Ordering::Equal;
                    }
                };

                let ord = match (&va, &vb) {
                    (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
                    (Value::Null, _) => {
                        if key.nulls_first {
                            std::cmp::Ordering::Less
                        } else {
                            std::cmp::Ordering::Greater
                        }
                    }
                    (_, Value::Null) => {
                        if key.nulls_first {
                            std::cmp::Ordering::Greater
                        } else {
                            std::cmp::Ordering::Less
                        }
                    }
                    _ => va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal),
                };

                if ord != std::cmp::Ordering::Equal {
                    return if key.ascending { ord } else { ord.reverse() };
                }
            }
            std::cmp::Ordering::Equal
        });

        if let Some(e) = errors.into_iter().next() {
            return Err(e);
        }

        Ok(rows)
    }

    /// Evaluate a CHECK constraint expression string against a row.
    /// Returns true if the constraint passes (is satisfied or NULL), false if violated.
    fn eval_check_constraint(&self, expr_str: &str, row: &Row) -> Result<bool, SqlError> {
        let planner = crate::planner::Planner::new(Arc::clone(&self.ctx.catalog));
        let expr = planner.expr_from_str(expr_str)?;
        match self.eval_expr(&expr, row)? {
            Value::Bool(b) => Ok(b),
            Value::Null => Ok(true), // NULL check result is not a violation
            _ => Ok(true),
        }
    }

    fn exec_limit(&self, rows: Vec<Row>, limit: u64, offset: u64) -> Vec<Row> {
        let start = offset.min(rows.len() as u64) as usize;
        let end = if limit == u64::MAX {
            rows.len()
        } else {
            (start + limit as usize).min(rows.len())
        };
        rows[start..end].to_vec()
    }

    fn exec_join(
        &self,
        left: Vec<Row>,
        right: Vec<Row>,
        join_type: JoinType,
        condition: &Expr,
        using_columns: &[String],
        algorithm: &crate::plan::JoinAlgorithm,
    ) -> Result<Vec<Row>, SqlError> {
        use crate::plan::JoinAlgorithm;
        // Use hash join for equality conditions (inner/left join only)
        if *algorithm == JoinAlgorithm::Hash
            && using_columns.is_empty()
            && matches!(join_type, JoinType::Inner | JoinType::Left)
        {
            return self.exec_hash_join(left, right, join_type, condition);
        }
        let mut result = Vec::new();

        match join_type {
            JoinType::Cross | JoinType::Inner => {
                for lr in &left {
                    for rr in &right {
                        if !using_columns.is_empty() {
                            if !rows_match_using(lr, rr, using_columns) {
                                continue;
                            }
                        } else {
                            let combined = merge_rows(lr, rr);
                            let val = self.eval_expr(condition, &combined)?;
                            if !matches!(val, Value::Bool(true)) {
                                continue;
                            }
                        }
                        let combined = merge_rows(lr, rr);
                        result.push(deduplicate_using_cols(combined, using_columns));
                    }
                }
            }
            JoinType::Left => {
                for lr in &left {
                    let mut matched = false;
                    for rr in &right {
                        let matches = if !using_columns.is_empty() {
                            rows_match_using(lr, rr, using_columns)
                        } else {
                            let combined = merge_rows(lr, rr);
                            matches!(self.eval_expr(condition, &combined)?, Value::Bool(true))
                        };
                        if matches {
                            let combined = merge_rows(lr, rr);
                            result.push(deduplicate_using_cols(combined, using_columns));
                            matched = true;
                        }
                    }
                    if !matched {
                        // Pad right with NULLs
                        result.push(left_pad_row(lr, &right));
                    }
                }
            }
            JoinType::Right => {
                for rr in &right {
                    let mut matched = false;
                    for lr in &left {
                        let matches = if !using_columns.is_empty() {
                            rows_match_using(lr, rr, using_columns)
                        } else {
                            let combined = merge_rows(lr, rr);
                            matches!(self.eval_expr(condition, &combined)?, Value::Bool(true))
                        };
                        if matches {
                            let combined = merge_rows(lr, rr);
                            result.push(deduplicate_using_cols(combined, using_columns));
                            matched = true;
                        }
                    }
                    if !matched {
                        result.push(right_pad_row(&left, rr));
                    }
                }
            }
            JoinType::Full => {
                let mut left_matched = vec![false; left.len()];
                for rr in &right {
                    let mut matched = false;
                    for (i, lr) in left.iter().enumerate() {
                        let matches = if !using_columns.is_empty() {
                            rows_match_using(lr, rr, using_columns)
                        } else {
                            let combined = merge_rows(lr, rr);
                            matches!(self.eval_expr(condition, &combined)?, Value::Bool(true))
                        };
                        if matches {
                            let combined = merge_rows(lr, rr);
                            result.push(deduplicate_using_cols(combined, using_columns));
                            left_matched[i] = true;
                            matched = true;
                        }
                    }
                    if !matched {
                        result.push(right_pad_row(&left, rr));
                    }
                }
                for (i, lr) in left.iter().enumerate() {
                    if !left_matched[i] {
                        result.push(left_pad_row(lr, &right));
                    }
                }
            }
        }

        Ok(result)
    }

    /// Hash join implementation for inner and left joins with equality conditions.
    fn exec_hash_join(
        &self,
        left: Vec<Row>,
        right: Vec<Row>,
        join_type: JoinType,
        condition: &Expr,
    ) -> Result<Vec<Row>, SqlError> {
        // Extract equality key columns from condition
        let (eq_left_expr, eq_right_expr) = extract_hash_join_keys(condition);

        // Determine which equality operand belongs to which input side.
        // The condition `a.col = b.col` may have operands in either order relative
        // to the join's left/right inputs.  Try both orientations using a sample row.
        let (left_key_expr, right_key_expr) = if let (Some(ref le), Some(ref re)) =
            (&eq_left_expr, &eq_right_expr)
        {
            let _left_sample = left.first();
            let _right_sample = right.first();
            // Check if le resolves on the left side and re resolves on the right side
            // Use is_ok() not non-null: a column may legitimately hold NULL.
            // We just need to know if the column *exists* on that side.
            let le_on_left = left.iter().any(|r| self.eval_expr(le, r).is_ok());
            let re_on_right = right.iter().any(|r| self.eval_expr(re, r).is_ok());
            if le_on_left && re_on_right {
                (eq_left_expr.clone(), eq_right_expr.clone())
            } else {
                // Try swapped orientation
                (eq_right_expr.clone(), eq_left_expr.clone())
            }
        } else {
            (eq_left_expr, eq_right_expr)
        };

        // Build hash table from right side keyed by the right-side key value.
        // Fall back to nested-loop if key extraction still fails for all right rows.
        let mut hash_table: HashMap<String, Vec<Row>> = HashMap::new();
        let mut hash_build_failures = 0usize;
        for rr in &right {
            let key_val = if let Some(ref rexpr) = right_key_expr {
                match self.eval_expr(rexpr, rr) {
                    Ok(v) if !matches!(v, Value::Null) => format!("{v:?}"),
                    _ => { hash_build_failures += 1; continue; }
                }
            } else {
                format!("{:?}", rr.values)
            };
            hash_table.entry(key_val).or_default().push(rr.clone());
        }
        // If we couldn't build a useful hash table, fall back to nested-loop join.
        if !right.is_empty() && hash_build_failures == right.len() {
            return self.exec_nested_loop_join(left, right, join_type, condition);
        }

        let mut result = Vec::new();
        for lr in &left {
            let probe_key = if let Some(ref lexpr) = left_key_expr {
                match self.eval_expr(lexpr, lr) {
                    Ok(v) if !matches!(v, Value::Null) => format!("{v:?}"),
                    _ => {
                        // Key not found on left side — fall through to NL for this row
                        if matches!(join_type, JoinType::Left) {
                            result.push(left_pad_row(lr, &right));
                        }
                        continue;
                    }
                }
            } else {
                format!("{:?}", lr.values)
            };

            let matching_right = hash_table.get(&probe_key);
            match join_type {
                JoinType::Inner => {
                    if let Some(right_rows) = matching_right {
                        for rr in right_rows {
                            // Verify with full condition (handles AND of equalities)
                            let combined = merge_rows(lr, rr);
                            if matches!(self.eval_expr(condition, &combined)?, Value::Bool(true)) {
                                result.push(combined);
                            }
                        }
                    }
                }
                JoinType::Left => {
                    let mut matched = false;
                    if let Some(right_rows) = matching_right {
                        for rr in right_rows {
                            let combined = merge_rows(lr, rr);
                            if matches!(self.eval_expr(condition, &combined)?, Value::Bool(true)) {
                                result.push(combined);
                                matched = true;
                            }
                        }
                    }
                    if !matched {
                        result.push(left_pad_row(lr, &right));
                    }
                }
                _ => unreachable!("hash join only for inner/left"),
            }
        }
        Ok(result)
    }

    /// Nested-loop join fallback used when hash join key extraction fails.
    fn exec_nested_loop_join(
        &self,
        left: Vec<Row>,
        right: Vec<Row>,
        join_type: JoinType,
        condition: &Expr,
    ) -> Result<Vec<Row>, SqlError> {
        let mut result = Vec::new();
        match join_type {
            JoinType::Inner | JoinType::Cross => {
                for lr in &left {
                    for rr in &right {
                        let combined = merge_rows(lr, rr);
                        if matches!(self.eval_expr(condition, &combined)?, Value::Bool(true)) {
                            result.push(combined);
                        }
                    }
                }
            }
            JoinType::Left => {
                for lr in &left {
                    let mut matched = false;
                    for rr in &right {
                        let combined = merge_rows(lr, rr);
                        if matches!(self.eval_expr(condition, &combined)?, Value::Bool(true)) {
                            result.push(combined);
                            matched = true;
                        }
                    }
                    if !matched {
                        result.push(left_pad_row(lr, &right));
                    }
                }
            }
            _ => {
                for lr in &left {
                    for rr in &right {
                        let combined = merge_rows(lr, rr);
                        if matches!(self.eval_expr(condition, &combined)?, Value::Bool(true)) {
                            result.push(combined);
                        }
                    }
                }
            }
        }
        Ok(result)
    }

    fn exec_aggregate(
        &self,
        rows: Vec<Row>,
        group_by: &[Expr],
        aggregates: &[(String, AggFunc, Expr)],
        having: Option<&Expr>,
    ) -> Result<Vec<Row>, SqlError> {
        // If no group by, treat all rows as one group
        if group_by.is_empty() {
            let agg_row = self.compute_aggregates(&rows, aggregates)?;
            let mut result = vec![agg_row];
            if let Some(pred) = having {
                result = self.exec_filter(result, pred)?;
            }
            return Ok(result);
        }

        // Group rows by group-by key
        let mut groups: Vec<Vec<Value>> = Vec::new();
        let mut group_rows: Vec<Vec<&Row>> = Vec::new();
        let mut group_index: HashMap<Vec<OrderableValue>, usize> = HashMap::new();

        for row in &rows {
            let key: Vec<Value> = group_by
                .iter()
                .map(|e| self.eval_expr(e, row))
                .collect::<Result<Vec<_>, _>>()?;

            let orderable_key: Vec<OrderableValue> =
                key.iter().map(|v| OrderableValue(v.clone())).collect();

            if let Some(&idx) = group_index.get(&orderable_key) {
                group_rows[idx].push(row);
            } else {
                let idx = groups.len();
                group_index.insert(orderable_key, idx);
                groups.push(key);
                group_rows.push(vec![row]);
            }
        }

        let mut result = Vec::new();
        for (group_key, g_rows) in groups.iter().zip(group_rows.iter()) {
            let owned_rows: Vec<Row> = g_rows.iter().map(|r| (*r).clone()).collect();
            let mut agg_row = self.compute_aggregates(&owned_rows, aggregates)?;
            // Prepend group key values to the row
            let mut combined_values = group_key.clone();
            combined_values.extend_from_slice(&agg_row.values);
            let group_input_schema: &[(String, catalog::DataType)] = owned_rows
                .first()
                .map(|r| r.schema.as_slice())
                .unwrap_or(&[]);
            let mut combined_schema: Vec<(String, catalog::DataType)> = group_by
                .iter()
                .enumerate()
                .map(|(i, expr)| {
                    // Use actual column name from the expr if it's a Column reference
                    let col_name = match expr {
                        Expr::Column { name, .. } => name.clone(),
                        _ => format!("group_{i}"),
                    };
                    (col_name, infer_expr_type(expr, group_input_schema))
                })
                .collect();
            combined_schema.extend_from_slice(&agg_row.schema);
            agg_row = Row::new(combined_values, combined_schema);
            result.push(agg_row);
        }

        // Apply HAVING filter after grouping
        if let Some(pred) = having {
            result = self.exec_filter(result, pred)?;
        }

        Ok(result)
    }

    fn compute_aggregates(
        &self,
        rows: &[Row],
        aggregates: &[(String, AggFunc, Expr)],
    ) -> Result<Row, SqlError> {
        let mut values = Vec::new();
        let mut schema = Vec::new();

        for (name, func, expr) in aggregates {
            let val = match func {
                AggFunc::Count => {
                    // COUNT(*) or COUNT(expr) - count non-null values
                    let count = if matches!(expr, Expr::Literal(Value::Int4(1))) {
                        // COUNT(*) - count all rows
                        rows.len() as i64
                    } else {
                        rows.iter()
                            .filter(|row| {
                                self.eval_expr(expr, row)
                                    .map(|v| !matches!(v, Value::Null))
                                    .unwrap_or(false)
                            })
                            .count() as i64
                    };
                    Value::Int8(count)
                }
                AggFunc::CountDistinct => {
                    // COUNT(DISTINCT expr) — count unique non-null values
                    let mut seen: std::collections::HashSet<OrderableValue> =
                        std::collections::HashSet::new();
                    for row in rows {
                        let v = self.eval_expr(expr, row)?;
                        if !matches!(v, Value::Null) {
                            seen.insert(OrderableValue(v));
                        }
                    }
                    Value::Int8(seen.len() as i64)
                }
                AggFunc::Sum => {
                    let mut sum: Option<Value> = None;
                    for row in rows {
                        let v = self.eval_expr(expr, row)?;
                        if matches!(v, Value::Null) {
                            continue;
                        }
                        sum = Some(match sum {
                            None => v,
                            Some(acc) => self.eval_binary_op(acc, &BinaryOp::Add, v)?,
                        });
                    }
                    sum.unwrap_or(Value::Null)
                }
                AggFunc::Min => {
                    let mut min: Option<Value> = None;
                    for row in rows {
                        let v = self.eval_expr(expr, row)?;
                        if matches!(v, Value::Null) {
                            continue;
                        }
                        min = Some(match min {
                            None => v.clone(),
                            Some(acc) => {
                                if v.partial_cmp(&acc) == Some(std::cmp::Ordering::Less) {
                                    v
                                } else {
                                    acc
                                }
                            }
                        });
                    }
                    min.unwrap_or(Value::Null)
                }
                AggFunc::Max => {
                    let mut max: Option<Value> = None;
                    for row in rows {
                        let v = self.eval_expr(expr, row)?;
                        if matches!(v, Value::Null) {
                            continue;
                        }
                        max = Some(match max {
                            None => v.clone(),
                            Some(acc) => {
                                if v.partial_cmp(&acc) == Some(std::cmp::Ordering::Greater) {
                                    v
                                } else {
                                    acc
                                }
                            }
                        });
                    }
                    max.unwrap_or(Value::Null)
                }
                AggFunc::Avg => {
                    let mut sum: Option<f64> = None;
                    let mut count = 0i64;
                    for row in rows {
                        let v = self.eval_expr(expr, row)?;
                        let f = match v {
                            Value::Int4(i) => i as f64,
                            Value::Int8(i) => i as f64,
                            Value::Float8(f) => f,
                            Value::Null => continue,
                            _ => continue,
                        };
                        sum = Some(sum.unwrap_or(0.0) + f);
                        count += 1;
                    }
                    if count == 0 {
                        Value::Null
                    } else {
                        Value::Float8(sum.unwrap_or(0.0) / count as f64)
                    }
                }
                AggFunc::Stddev | AggFunc::StddevPop => {
                    let mut vals: Vec<f64> = Vec::new();
                    for row in rows {
                        let v = self.eval_expr(expr, row)?;
                        match v {
                            Value::Int4(i) => vals.push(i as f64),
                            Value::Int8(i) => vals.push(i as f64),
                            Value::Float8(f) => vals.push(f),
                            Value::Null => {}
                            _ => {}
                        }
                    }
                    let n = vals.len();
                    if n == 0 || (matches!(func, AggFunc::Stddev) && n < 2) {
                        Value::Null
                    } else {
                        let mean = vals.iter().sum::<f64>() / n as f64;
                        let variance = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>();
                        let denom = if matches!(func, AggFunc::Stddev) { (n - 1) as f64 } else { n as f64 };
                        Value::Float8((variance / denom).sqrt())
                    }
                }
                AggFunc::Variance | AggFunc::VarPop => {
                    let mut vals: Vec<f64> = Vec::new();
                    for row in rows {
                        let v = self.eval_expr(expr, row)?;
                        match v {
                            Value::Int4(i) => vals.push(i as f64),
                            Value::Int8(i) => vals.push(i as f64),
                            Value::Float8(f) => vals.push(f),
                            Value::Null => {}
                            _ => {}
                        }
                    }
                    let n = vals.len();
                    if n == 0 || (matches!(func, AggFunc::Variance) && n < 2) {
                        Value::Null
                    } else {
                        let mean = vals.iter().sum::<f64>() / n as f64;
                        let variance_sum = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>();
                        let denom = if matches!(func, AggFunc::Variance) { (n - 1) as f64 } else { n as f64 };
                        Value::Float8(variance_sum / denom)
                    }
                }
                AggFunc::StringAgg { delimiter } => {
                    let mut parts: Vec<String> = Vec::new();
                    for row in rows {
                        let v = self.eval_expr(expr, row)?;
                        match v {
                            Value::Text(s) => parts.push(s),
                            Value::Null => {}
                            other => parts.push(other.to_string()),
                        }
                    }
                    if parts.is_empty() {
                        Value::Null
                    } else {
                        Value::Text(parts.join(delimiter))
                    }
                }
                AggFunc::BoolAnd => {
                    let mut result = true;
                    let mut any = false;
                    for row in rows {
                        let v = self.eval_expr(expr, row)?;
                        match v {
                            Value::Bool(b) => { result = result && b; any = true; }
                            Value::Null => {}
                            _ => {}
                        }
                    }
                    if any { Value::Bool(result) } else { Value::Null }
                }
                AggFunc::BoolOr => {
                    let mut result = false;
                    let mut any = false;
                    for row in rows {
                        let v = self.eval_expr(expr, row)?;
                        match v {
                            Value::Bool(b) => { result = result || b; any = true; }
                            Value::Null => {}
                            _ => {}
                        }
                    }
                    if any { Value::Bool(result) } else { Value::Null }
                }
                AggFunc::ArrayAgg => {
                    // Collect all values as text joined with comma (simplified)
                    let mut parts: Vec<String> = Vec::new();
                    for row in rows {
                        let v = self.eval_expr(expr, row)?;
                        if !matches!(v, Value::Null) {
                            parts.push(v.to_string());
                        }
                    }
                    Value::Text(format!("{{{}}}", parts.join(",")))
                }
            };
            let dtype = match func {
                AggFunc::Count | AggFunc::CountDistinct => catalog::DataType::Int8,
                AggFunc::BoolAnd | AggFunc::BoolOr => catalog::DataType::Boolean,
                AggFunc::Avg | AggFunc::Stddev | AggFunc::StddevPop | AggFunc::Variance | AggFunc::VarPop => catalog::DataType::Float8,
                AggFunc::StringAgg { .. } | AggFunc::ArrayAgg => catalog::DataType::Text,
                AggFunc::Sum | AggFunc::Min | AggFunc::Max => {
                    // Infer from the expression type using the first row's schema,
                    // or fallback to Text if no rows.
                    let input_schema = rows.first().map(|r| r.schema.as_slice()).unwrap_or(&[]);
                    infer_expr_type(expr, input_schema)
                }
            };
            values.push(val);
            schema.push((name.clone(), dtype));
        }

        Ok(Row::new(values, schema))
    }

    fn eval_expr(&self, expr: &Expr, row: &Row) -> Result<Value, SqlError> {
        match expr {
            Expr::Literal(v) => Ok(v.clone()),
            Expr::Column { table, name } => {
                // Try with table qualifier first
                if let Some(tbl) = table {
                    let qualified = format!("{tbl}.{name}");
                    if let Some(v) = row.get(&qualified) {
                        return Ok(v.clone());
                    }
                }
                // Try bare name
                if let Some(v) = row.get(name) {
                    return Ok(v.clone());
                }
                // If not found, check if it appears multiple times (ambiguous)
                let matches: Vec<_> = row
                    .schema
                    .iter()
                    .enumerate()
                    .filter(|(_, (col_name, _))| {
                        col_name == name || col_name.ends_with(&format!(".{name}"))
                    })
                    .collect();

                match matches.len() {
                    0 => Err(SqlError::ColumnNotFound(name.clone())),
                    1 => Ok(row.values[matches[0].0].clone()),
                    _ => Err(SqlError::AmbiguousColumn(name.clone())),
                }
            }
            Expr::BinaryOp { left, op, right } => {
                let l = self.eval_expr(left, row)?;
                let r = self.eval_expr(right, row)?;
                self.eval_binary_op(l, op, r)
            }
            Expr::UnaryOp { op, expr } => {
                let v = self.eval_expr(expr, row)?;
                match op {
                    UnaryOp::Neg => match v {
                        Value::Int4(i) => Ok(Value::Int4(-i)),
                        Value::Int8(i) => Ok(Value::Int8(-i)),
                        Value::Float8(f) => Ok(Value::Float8(-f)),
                        Value::Null => Ok(Value::Null),
                        _ => Err(SqlError::TypeError(format!("cannot negate {:?}", v))),
                    },
                    UnaryOp::Not => match v {
                        Value::Bool(b) => Ok(Value::Bool(!b)),
                        Value::Null => Ok(Value::Null),
                        _ => Err(SqlError::TypeError(format!("cannot NOT {:?}", v))),
                    },
                }
            }
            Expr::IsNull(inner) => {
                let v = self.eval_expr(inner, row)?;
                Ok(Value::Bool(matches!(v, Value::Null)))
            }
            Expr::IsNotNull(inner) => {
                let v = self.eval_expr(inner, row)?;
                Ok(Value::Bool(!matches!(v, Value::Null)))
            }
            Expr::IsDistinctFrom { left, right } => {
                let l = self.eval_expr(left, row)?;
                let r = self.eval_expr(right, row)?;
                // IS DISTINCT FROM: treats NULLs as distinct — NULL IS DISTINCT FROM NULL = false
                let distinct = match (&l, &r) {
                    (Value::Null, Value::Null) => false,
                    (Value::Null, _) | (_, Value::Null) => true,
                    _ => l != r,
                };
                Ok(Value::Bool(distinct))
            }
            Expr::IsNotDistinctFrom { left, right } => {
                let l = self.eval_expr(left, row)?;
                let r = self.eval_expr(right, row)?;
                let not_distinct = match (&l, &r) {
                    (Value::Null, Value::Null) => true,
                    (Value::Null, _) | (_, Value::Null) => false,
                    _ => l == r,
                };
                Ok(Value::Bool(not_distinct))
            }
            Expr::InSubquery {
                expr,
                subquery,
                negated,
            } => {
                let val = self.eval_expr(expr, row)?;
                if matches!(val, Value::Null) {
                    return Ok(Value::Null);
                }
                let sub_rows = self.exec_plan_with_outer(subquery, Some(row))?;
                let found = sub_rows.iter().any(|r| {
                    if let Some(v) = r.get_by_idx(0) {
                        matches!(v.partial_cmp(&val), Some(std::cmp::Ordering::Equal))
                    } else {
                        false
                    }
                });
                if *negated {
                    Ok(Value::Bool(!found))
                } else {
                    Ok(Value::Bool(found))
                }
            }
            Expr::Exists { subquery, negated } => {
                let sub_rows = self.exec_plan_with_outer(subquery, Some(row))?;
                let exists = !sub_rows.is_empty();
                if *negated {
                    Ok(Value::Bool(!exists))
                } else {
                    Ok(Value::Bool(exists))
                }
            }
            Expr::ScalarSubquery(subquery) => {
                let sub_rows = self.exec_plan_with_outer(subquery, Some(row))?;
                if sub_rows.is_empty() {
                    return Ok(Value::Null);
                }
                if sub_rows.len() > 1 {
                    return Err(SqlError::Execution(
                        "scalar subquery returned more than one row".to_string(),
                    ));
                }
                Ok(sub_rows[0].get_by_idx(0).cloned().unwrap_or(Value::Null))
            }
            Expr::Cast { expr, data_type } => {
                let v = self.eval_expr(expr, row)?;
                v.cast_to(data_type)
            }
            Expr::FunctionCall { name, args } => self.eval_function(name, args, row),
            Expr::Case {
                operand,
                when_clauses,
                else_clause,
            } => {
                if let Some(op_expr) = operand {
                    // Simple CASE: CASE expr WHEN val THEN result END
                    let op_val = self.eval_expr(op_expr, row)?;
                    for (when_val_expr, result_expr) in when_clauses {
                        let when_val = self.eval_expr(when_val_expr, row)?;
                        let matches = match (&op_val, &when_val) {
                            (Value::Null, _) | (_, Value::Null) => false,
                            _ => op_val.partial_cmp(&when_val) == Some(std::cmp::Ordering::Equal),
                        };
                        if matches {
                            return self.eval_expr(result_expr, row);
                        }
                    }
                } else {
                    // Searched CASE: CASE WHEN condition THEN result END
                    for (condition_expr, result_expr) in when_clauses {
                        let cond_val = self.eval_expr(condition_expr, row)?;
                        if matches!(cond_val, Value::Bool(true)) {
                            return self.eval_expr(result_expr, row);
                        }
                    }
                }
                // No WHEN matched — return ELSE or NULL
                if let Some(else_expr) = else_clause {
                    self.eval_expr(else_expr, row)
                } else {
                    Ok(Value::Null)
                }
            }
            Expr::Coalesce(args) => {
                for arg in args {
                    let v = self.eval_expr(arg, row)?;
                    if !matches!(v, Value::Null) {
                        return Ok(v);
                    }
                }
                Ok(Value::Null)
            }
            Expr::NullIf(expr1, expr2) => {
                let v1 = self.eval_expr(expr1, row)?;
                let v2 = self.eval_expr(expr2, row)?;
                let equal = match (&v1, &v2) {
                    (Value::Null, _) | (_, Value::Null) => false,
                    _ => v1.partial_cmp(&v2) == Some(std::cmp::Ordering::Equal),
                };
                if equal {
                    Ok(Value::Null)
                } else {
                    Ok(v1)
                }
            }
        }
    }

    fn eval_function(&self, name: &str, args: &[Expr], row: &Row) -> Result<Value, SqlError> {
        // Helper: evaluate first arg as string
        let eval_str_arg = |idx: usize| -> Result<Option<String>, SqlError> {
            if idx >= args.len() { return Ok(None); }
            match self.eval_expr(&args[idx], row)? {
                Value::Text(s) | Value::Numeric(s) | Value::Uuid(s) => Ok(Some(s)),
                Value::Null => Ok(None),
                v => Ok(Some(v.to_string())),
            }
        };
        let eval_f64_arg = |idx: usize| -> Result<Option<f64>, SqlError> {
            if idx >= args.len() { return Ok(None); }
            match self.eval_expr(&args[idx], row)? {
                Value::Float8(f) => Ok(Some(f)),
                Value::Int4(i) => Ok(Some(i as f64)),
                Value::Int8(i) => Ok(Some(i as f64)),
                Value::Numeric(s) => s.parse::<f64>().map(Some).map_err(|_| SqlError::TypeError(format!("invalid numeric: {s}"))),
                Value::Null => Ok(None),
                v => Err(SqlError::TypeError(format!("expected numeric, got {v:?}"))),
            }
        };

        match name {
            "lower" => {
                match eval_str_arg(0)? {
                    Some(s) => Ok(Value::Text(s.to_lowercase())),
                    None => Ok(Value::Null),
                }
            }
            "upper" => {
                match eval_str_arg(0)? {
                    Some(s) => Ok(Value::Text(s.to_uppercase())),
                    None => Ok(Value::Null),
                }
            }
            "length" | "char_length" | "character_length" | "len" => {
                match eval_str_arg(0)? {
                    Some(s) => Ok(Value::Int8(s.chars().count() as i64)),
                    None => Ok(Value::Null),
                }
            }
            "trim" | "btrim" => {
                match eval_str_arg(0)? {
                    Some(s) => Ok(Value::Text(s.trim().to_string())),
                    None => Ok(Value::Null),
                }
            }
            "ltrim" => {
                match eval_str_arg(0)? {
                    Some(s) => Ok(Value::Text(s.trim_start().to_string())),
                    None => Ok(Value::Null),
                }
            }
            "rtrim" => {
                match eval_str_arg(0)? {
                    Some(s) => Ok(Value::Text(s.trim_end().to_string())),
                    None => Ok(Value::Null),
                }
            }
            "substring" | "substr" => {
                let s = match eval_str_arg(0)? { Some(s) => s, None => return Ok(Value::Null) };
                let chars: Vec<char> = s.chars().collect();
                let start = match self.eval_expr(&args[1], row)? {
                    Value::Int4(i) => (i - 1).max(0) as usize,
                    Value::Int8(i) => (i - 1).max(0) as usize,
                    _ => 0,
                };
                let result = if args.len() >= 3 {
                    let len = match self.eval_expr(&args[2], row)? {
                        Value::Int4(i) => i.max(0) as usize,
                        Value::Int8(i) => i.max(0) as usize,
                        _ => chars.len(),
                    };
                    chars[start.min(chars.len())..].iter().take(len).collect()
                } else {
                    chars[start.min(chars.len())..].iter().collect()
                };
                Ok(Value::Text(result))
            }
            "replace" => {
                let s = match eval_str_arg(0)? { Some(s) => s, None => return Ok(Value::Null) };
                let from = match eval_str_arg(1)? { Some(s) => s, None => return Ok(Value::Text(s)) };
                let to = eval_str_arg(2)?.unwrap_or_default();
                Ok(Value::Text(s.replace(&from, &to)))
            }
            "split_part" => {
                let s = match eval_str_arg(0)? { Some(s) => s, None => return Ok(Value::Null) };
                let delim = match eval_str_arg(1)? { Some(s) => s, None => return Ok(Value::Null) };
                let n = match self.eval_expr(&args[2], row)? {
                    Value::Int4(i) => i as usize,
                    Value::Int8(i) => i as usize,
                    _ => 1,
                };
                let parts: Vec<&str> = s.split(&delim as &str).collect();
                let result = if n >= 1 && n <= parts.len() {
                    parts[n - 1].to_string()
                } else {
                    String::new()
                };
                Ok(Value::Text(result))
            }
            "starts_with" => {
                let s = match eval_str_arg(0)? { Some(s) => s, None => return Ok(Value::Null) };
                let prefix = match eval_str_arg(1)? { Some(s) => s, None => return Ok(Value::Null) };
                Ok(Value::Bool(s.starts_with(&prefix as &str)))
            }
            "ends_with" => {
                let s = match eval_str_arg(0)? { Some(s) => s, None => return Ok(Value::Null) };
                let suffix = match eval_str_arg(1)? { Some(s) => s, None => return Ok(Value::Null) };
                Ok(Value::Bool(s.ends_with(&suffix as &str)))
            }
            "repeat" => {
                let s = match eval_str_arg(0)? { Some(s) => s, None => return Ok(Value::Null) };
                let n = match self.eval_expr(&args[1], row)? {
                    Value::Int4(i) => i.max(0) as usize,
                    Value::Int8(i) => i.max(0) as usize,
                    _ => 0,
                };
                Ok(Value::Text(s.repeat(n)))
            }
            "reverse" => {
                match eval_str_arg(0)? {
                    Some(s) => Ok(Value::Text(s.chars().rev().collect())),
                    None => Ok(Value::Null),
                }
            }
            "lpad" => {
                let s = match eval_str_arg(0)? { Some(s) => s, None => return Ok(Value::Null) };
                let len = match self.eval_expr(&args[1], row)? {
                    Value::Int4(i) => i.max(0) as usize,
                    Value::Int8(i) => i.max(0) as usize,
                    _ => return Ok(Value::Text(s)),
                };
                let fill = if args.len() >= 3 {
                    eval_str_arg(2)?.unwrap_or_else(|| " ".to_string())
                } else {
                    " ".to_string()
                };
                let chars: Vec<char> = s.chars().collect();
                if chars.len() >= len {
                    Ok(Value::Text(chars[..len].iter().collect()))
                } else {
                    let fill_chars: Vec<char> = fill.chars().collect();
                    let needed = len - chars.len();
                    let pad: String = fill_chars.iter().cycle().take(needed).collect();
                    Ok(Value::Text(format!("{pad}{s}")))
                }
            }
            "rpad" => {
                let s = match eval_str_arg(0)? { Some(s) => s, None => return Ok(Value::Null) };
                let len = match self.eval_expr(&args[1], row)? {
                    Value::Int4(i) => i.max(0) as usize,
                    Value::Int8(i) => i.max(0) as usize,
                    _ => return Ok(Value::Text(s)),
                };
                let fill = if args.len() >= 3 {
                    eval_str_arg(2)?.unwrap_or_else(|| " ".to_string())
                } else {
                    " ".to_string()
                };
                let chars: Vec<char> = s.chars().collect();
                if chars.len() >= len {
                    Ok(Value::Text(chars[..len].iter().collect()))
                } else {
                    let fill_chars: Vec<char> = fill.chars().collect();
                    let needed = len - chars.len();
                    let pad: String = fill_chars.iter().cycle().take(needed).collect();
                    Ok(Value::Text(format!("{s}{pad}")))
                }
            }
            "strpos" | "position" => {
                let haystack = match eval_str_arg(0)? { Some(s) => s, None => return Ok(Value::Null) };
                let needle = match eval_str_arg(1)? { Some(s) => s, None => return Ok(Value::Null) };
                let pos = haystack.find(&needle as &str).map(|i| {
                    haystack[..i].chars().count() as i64 + 1
                }).unwrap_or(0);
                Ok(Value::Int8(pos))
            }
            "concat" | "concat_ws" => {
                let start = if name == "concat_ws" { 1 } else { 0 };
                let sep = if name == "concat_ws" {
                    eval_str_arg(0)?.unwrap_or_default()
                } else {
                    String::new()
                };
                let mut parts = Vec::new();
                for arg in args.iter().skip(start) {
                    match self.eval_expr(arg, row)? {
                        Value::Null => {
                            if name == "concat" { parts.push(String::new()); }
                        }
                        v => parts.push(v.to_string()),
                    }
                }
                Ok(Value::Text(parts.join(&sep)))
            }
            "left" => {
                let s = match eval_str_arg(0)? { Some(s) => s, None => return Ok(Value::Null) };
                let n = match self.eval_expr(&args[1], row)? {
                    Value::Int4(i) => i,
                    Value::Int8(i) => i as i32,
                    _ => return Ok(Value::Text(s)),
                };
                let chars: Vec<char> = s.chars().collect();
                let result: String = if n >= 0 {
                    chars[..(n as usize).min(chars.len())].iter().collect()
                } else {
                    let skip = (-n as usize).min(chars.len());
                    chars[..chars.len().saturating_sub(skip)].iter().collect()
                };
                Ok(Value::Text(result))
            }
            "right" => {
                let s = match eval_str_arg(0)? { Some(s) => s, None => return Ok(Value::Null) };
                let n = match self.eval_expr(&args[1], row)? {
                    Value::Int4(i) => i,
                    Value::Int8(i) => i as i32,
                    _ => return Ok(Value::Text(s)),
                };
                let chars: Vec<char> = s.chars().collect();
                let result: String = if n >= 0 {
                    let start = chars.len().saturating_sub(n as usize);
                    chars[start..].iter().collect()
                } else {
                    chars[(-n as usize).min(chars.len())..].iter().collect()
                };
                Ok(Value::Text(result))
            }
            "to_hex" => {
                let v = self.eval_expr(&args[0], row)?;
                let n = match v {
                    Value::Int4(i) => i as i64,
                    Value::Int8(i) => i,
                    _ => return Ok(Value::Null),
                };
                Ok(Value::Text(format!("{:x}", n)))
            }
            "md5" => {
                // Return a stub — not worth implementing without a crate
                Ok(Value::Text("00000000000000000000000000000000".to_string()))
            }
            "encode" => {
                // Stub
                Ok(Value::Text(String::new()))
            }
            // ── Math functions ────────────────────────────────────────────────
            "abs" => {
                match self.eval_expr(&args[0], row)? {
                    Value::Int4(i) => Ok(Value::Int4(i.abs())),
                    Value::Int8(i) => Ok(Value::Int8(i.abs())),
                    Value::Float8(f) => Ok(Value::Float8(f.abs())),
                    Value::Null => Ok(Value::Null),
                    v => Err(SqlError::TypeError(format!("abs: cannot apply to {v:?}"))),
                }
            }
            "ceil" | "ceiling" => {
                match eval_f64_arg(0)? {
                    Some(f) => Ok(Value::Float8(f.ceil())),
                    None => Ok(Value::Null),
                }
            }
            "floor" => {
                match eval_f64_arg(0)? {
                    Some(f) => Ok(Value::Float8(f.floor())),
                    None => Ok(Value::Null),
                }
            }
            "round" => {
                let f = match eval_f64_arg(0)? { Some(f) => f, None => return Ok(Value::Null) };
                if args.len() >= 2 {
                    let places = match self.eval_expr(&args[1], row)? {
                        Value::Int4(i) => i,
                        Value::Int8(i) => i as i32,
                        _ => 0,
                    };
                    let factor = 10f64.powi(places);
                    Ok(Value::Float8((f * factor).round() / factor))
                } else {
                    Ok(Value::Float8(f.round()))
                }
            }
            "trunc" | "truncate" => {
                let f = match eval_f64_arg(0)? { Some(f) => f, None => return Ok(Value::Null) };
                if args.len() >= 2 {
                    let places = match self.eval_expr(&args[1], row)? {
                        Value::Int4(i) => i,
                        Value::Int8(i) => i as i32,
                        _ => 0,
                    };
                    let factor = 10f64.powi(places);
                    Ok(Value::Float8((f * factor).trunc() / factor))
                } else {
                    Ok(Value::Float8(f.trunc()))
                }
            }
            "sqrt" => {
                match eval_f64_arg(0)? {
                    Some(f) => Ok(Value::Float8(f.sqrt())),
                    None => Ok(Value::Null),
                }
            }
            "power" | "pow" => {
                let base = match eval_f64_arg(0)? { Some(f) => f, None => return Ok(Value::Null) };
                let exp = match eval_f64_arg(1)? { Some(f) => f, None => return Ok(Value::Null) };
                Ok(Value::Float8(base.powf(exp)))
            }
            "sign" => {
                match eval_f64_arg(0)? {
                    Some(f) => Ok(Value::Float8(f.signum())),
                    None => Ok(Value::Null),
                }
            }
            "greatest" => {
                let mut best: Option<Value> = None;
                for arg in args {
                    let v = self.eval_expr(arg, row)?;
                    if matches!(v, Value::Null) { continue; }
                    best = Some(match best {
                        None => v.clone(),
                        Some(acc) => if v.partial_cmp(&acc) == Some(std::cmp::Ordering::Greater) { v } else { acc },
                    });
                }
                Ok(best.unwrap_or(Value::Null))
            }
            "least" => {
                let mut best: Option<Value> = None;
                for arg in args {
                    let v = self.eval_expr(arg, row)?;
                    if matches!(v, Value::Null) { continue; }
                    best = Some(match best {
                        None => v.clone(),
                        Some(acc) => if v.partial_cmp(&acc) == Some(std::cmp::Ordering::Less) { v } else { acc },
                    });
                }
                Ok(best.unwrap_or(Value::Null))
            }
            "random" => {
                Ok(Value::Float8(random_f64()))
            }
            "pi" => Ok(Value::Float8(std::f64::consts::PI)),
            "exp" => {
                match eval_f64_arg(0)? {
                    Some(f) => Ok(Value::Float8(f.exp())),
                    None => Ok(Value::Null),
                }
            }
            "ln" => {
                match eval_f64_arg(0)? {
                    Some(f) => Ok(Value::Float8(f.ln())),
                    None => Ok(Value::Null),
                }
            }
            "log" => {
                if args.len() == 1 {
                    match eval_f64_arg(0)? {
                        Some(f) => Ok(Value::Float8(f.log10())),
                        None => Ok(Value::Null),
                    }
                } else {
                    let base = match eval_f64_arg(0)? { Some(f) => f, None => return Ok(Value::Null) };
                    let n = match eval_f64_arg(1)? { Some(f) => f, None => return Ok(Value::Null) };
                    Ok(Value::Float8(n.log(base)))
                }
            }
            "log10" => {
                match eval_f64_arg(0)? {
                    Some(f) => Ok(Value::Float8(f.log10())),
                    None => Ok(Value::Null),
                }
            }
            "log2" => {
                match eval_f64_arg(0)? {
                    Some(f) => Ok(Value::Float8(f.log2())),
                    None => Ok(Value::Null),
                }
            }
            "degrees" => {
                match eval_f64_arg(0)? {
                    Some(f) => Ok(Value::Float8(f.to_degrees())),
                    None => Ok(Value::Null),
                }
            }
            "radians" => {
                match eval_f64_arg(0)? {
                    Some(f) => Ok(Value::Float8(f.to_radians())),
                    None => Ok(Value::Null),
                }
            }
            "sin" => { match eval_f64_arg(0)? { Some(f) => Ok(Value::Float8(f.sin())), None => Ok(Value::Null) } }
            "cos" => { match eval_f64_arg(0)? { Some(f) => Ok(Value::Float8(f.cos())), None => Ok(Value::Null) } }
            "tan" => { match eval_f64_arg(0)? { Some(f) => Ok(Value::Float8(f.tan())), None => Ok(Value::Null) } }
            "asin" => { match eval_f64_arg(0)? { Some(f) => Ok(Value::Float8(f.asin())), None => Ok(Value::Null) } }
            "acos" => { match eval_f64_arg(0)? { Some(f) => Ok(Value::Float8(f.acos())), None => Ok(Value::Null) } }
            "atan" => { match eval_f64_arg(0)? { Some(f) => Ok(Value::Float8(f.atan())), None => Ok(Value::Null) } }
            "atan2" => {
                let y = match eval_f64_arg(0)? { Some(f) => f, None => return Ok(Value::Null) };
                let x = match eval_f64_arg(1)? { Some(f) => f, None => return Ok(Value::Null) };
                Ok(Value::Float8(y.atan2(x)))
            }
            // ── Date/time functions ────────────────────────────────────────────
            "now" | "current_timestamp" => {
                Ok(Value::Timestamp(current_timestamp_micros()))
            }
            "current_date" => {
                Ok(Value::Date(current_date_days()))
            }
            "date_trunc" => {
                // date_trunc(field, timestamp) - basic implementation
                let field = match eval_str_arg(0)? { Some(s) => s.to_lowercase(), None => return Ok(Value::Null) };
                let ts = match self.eval_expr(&args[1], row)? {
                    Value::Timestamp(t) => t,
                    Value::Date(d) => d as i64 * 86_400_000_000,
                    Value::Null => return Ok(Value::Null),
                    v => return Err(SqlError::TypeError(format!("date_trunc: expected timestamp, got {v:?}"))),
                };
                let truncated = crate::value::date_trunc(ts, &field);
                Ok(Value::Timestamp(truncated))
            }
            "extract" | "date_part" => {
                let field = match eval_str_arg(0)? { Some(s) => s.to_lowercase(), None => return Ok(Value::Null) };
                let val = self.eval_expr(&args[1], row)?;
                let result = crate::value::extract_field(&field, &val);
                Ok(result.map(Value::Float8).unwrap_or(Value::Null))
            }
            "age" => {
                let ts1 = match self.eval_expr(&args[0], row)? {
                    Value::Timestamp(t) => t,
                    Value::Date(d) => d as i64 * 86_400_000_000,
                    Value::Null => return Ok(Value::Null),
                    _ => return Ok(Value::Null),
                };
                let ts2 = if args.len() >= 2 {
                    match self.eval_expr(&args[1], row)? {
                        Value::Timestamp(t) => t,
                        Value::Date(d) => d as i64 * 86_400_000_000,
                        Value::Null => return Ok(Value::Null),
                        _ => current_timestamp_micros(),
                    }
                } else {
                    current_timestamp_micros()
                };
                let diff_days = (ts1 - ts2) / 86_400_000_000;
                Ok(Value::Int8(diff_days))
            }
            // ── UUID functions ────────────────────────────────────────────────
            "gen_random_uuid" | "uuid_generate_v4" | "uuid" => {
                Ok(Value::Uuid(generate_uuid()))
            }
            // ── Type conversion ────────────────────────────────────────────────
            "to_char" => {
                let val = self.eval_expr(&args[0], row)?;
                // Simple to_char: just convert to string
                Ok(Value::Text(val.to_string()))
            }
            "to_number" | "to_float" => {
                let s = match eval_str_arg(0)? { Some(s) => s, None => return Ok(Value::Null) };
                s.trim().parse::<f64>().map(Value::Float8).map_err(|_| {
                    SqlError::TypeError(format!("to_number: cannot parse '{s}'"))
                })
            }
            "to_timestamp" => {
                let v = self.eval_expr(&args[0], row)?;
                match v {
                    Value::Float8(f) => Ok(Value::Timestamp((f * 1_000_000.0) as i64)),
                    Value::Int4(i) => Ok(Value::Timestamp(i as i64 * 1_000_000)),
                    Value::Int8(i) => Ok(Value::Timestamp(i * 1_000_000)),
                    Value::Text(s) => {
                        crate::value::parse_timestamp_str(&s)
                            .map(Value::Timestamp)
                            .ok_or_else(|| SqlError::TypeError(format!("to_timestamp: cannot parse '{s}'")))
                    }
                    _ => Ok(Value::Null),
                }
            }
            "to_date" => {
                let s = match eval_str_arg(0)? { Some(s) => s, None => return Ok(Value::Null) };
                crate::value::parse_date_str(&s)
                    .map(Value::Date)
                    .ok_or_else(|| SqlError::TypeError(format!("to_date: cannot parse '{s}'")))
            }
            // ── Existing functions ─────────────────────────────────────────────
            "like" => {
                if args.len() != 2 {
                    return Err(SqlError::Execution("like() requires 2 arguments".to_string()));
                }
                let text = self.eval_expr(&args[0], row)?;
                let pattern = self.eval_expr(&args[1], row)?;
                match (text, pattern) {
                    (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                    (Value::Text(t), Value::Text(p)) => Ok(Value::Bool(like_match(&t, &p))),
                    _ => Ok(Value::Bool(false)),
                }
            }
            "ilike" => {
                if args.len() != 2 {
                    return Err(SqlError::Execution("ilike() requires 2 arguments".to_string()));
                }
                let text = self.eval_expr(&args[0], row)?;
                let pattern = self.eval_expr(&args[1], row)?;
                match (text, pattern) {
                    (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                    (Value::Text(t), Value::Text(p)) => {
                        Ok(Value::Bool(like_match(&t.to_lowercase(), &p.to_lowercase())))
                    }
                    _ => Ok(Value::Bool(false)),
                }
            }
            "coalesce" => {
                for arg in args {
                    let v = self.eval_expr(arg, row)?;
                    if !matches!(v, Value::Null) {
                        return Ok(v);
                    }
                }
                Ok(Value::Null)
            }
            "nullif" => {
                if args.len() < 2 { return Ok(Value::Null); }
                let v1 = self.eval_expr(&args[0], row)?;
                let v2 = self.eval_expr(&args[1], row)?;
                let equal = match (&v1, &v2) {
                    (Value::Null, _) | (_, Value::Null) => false,
                    _ => v1.partial_cmp(&v2) == Some(std::cmp::Ordering::Equal),
                };
                if equal { Ok(Value::Null) } else { Ok(v1) }
            }
            "if" | "iff" => {
                // if(cond, then, else)
                let cond = self.eval_expr(&args[0], row)?;
                if matches!(cond, Value::Bool(true)) {
                    self.eval_expr(&args[1], row)
                } else if args.len() >= 3 {
                    self.eval_expr(&args[2], row)
                } else {
                    Ok(Value::Null)
                }
            }
            "floor_div" => {
                let a = match eval_f64_arg(0)? { Some(f) => f, None => return Ok(Value::Null) };
                let b = match eval_f64_arg(1)? { Some(f) => f, None => return Ok(Value::Null) };
                if b == 0.0 { return Err(SqlError::DivisionByZero); }
                Ok(Value::Int8((a / b).floor() as i64))
            }
            "mod" => {
                let a = self.eval_expr(&args[0], row)?;
                let b = self.eval_expr(&args[1], row)?;
                self.eval_binary_op(a, &BinaryOp::Div, b)
            }
            "int4" | "integer" => {
                let v = self.eval_expr(&args[0], row)?;
                v.cast_to(&catalog::DataType::Int4)
            }
            "int8" | "bigint" => {
                let v = self.eval_expr(&args[0], row)?;
                v.cast_to(&catalog::DataType::Int8)
            }
            "float8" | "float" | "double" => {
                let v = self.eval_expr(&args[0], row)?;
                v.cast_to(&catalog::DataType::Float8)
            }
            "bool" | "boolean" => {
                match self.eval_expr(&args[0], row)? {
                    Value::Bool(b) => Ok(Value::Bool(b)),
                    Value::Int4(i) => Ok(Value::Bool(i != 0)),
                    Value::Int8(i) => Ok(Value::Bool(i != 0)),
                    Value::Text(s) => Ok(Value::Bool(s.eq_ignore_ascii_case("true") || s == "1")),
                    Value::Null => Ok(Value::Null),
                    v => Err(SqlError::TypeError(format!("cannot cast {v:?} to bool"))),
                }
            }
            "text" => {
                let v = self.eval_expr(&args[0], row)?;
                v.cast_to(&catalog::DataType::Text)
            }
            "array_length" => {
                // Not really supported but return null
                Ok(Value::Null)
            }
            "unnest" => {
                // Not supported in scalar context
                Ok(Value::Null)
            }
            // ── System information functions ───────────────────────────────────
            "current_user" | "user" | "session_user" => {
                Ok(Value::Text("icedb".to_string()))
            }
            "version" => {
                Ok(Value::Text(format!("icedb {} ({})", env!("CARGO_PKG_VERSION"), std::env::consts::OS)))
            }
            "current_database" | "current_catalog" => {
                Ok(Value::Text(self.ctx.db_name.clone()))
            }
            "current_schema" => {
                Ok(Value::Text("public".to_string()))
            }
            "pg_backend_pid" => {
                Ok(Value::Int8(std::process::id() as i64))
            }
            "inet_server_addr" | "inet_client_addr" => {
                Ok(Value::Null)
            }
            "pg_postmaster_start_time" => {
                Ok(Value::Null)
            }
            "pg_is_in_recovery" => {
                Ok(Value::Bool(false))
            }
            "txid_current" => {
                Ok(Value::Int8(0))
            }
            _ => {
                // Look up user-defined SQL functions in the catalog
                let func_name = name.to_lowercase();
                if let Some(func) = self.ctx.catalog.get_function("public", &func_name) {
                    if func.language == "sql" {
                        let evaluated: Vec<Value> = args.iter()
                            .map(|a| self.eval_expr(a, row))
                            .collect::<Result<Vec<_>, _>>()?;
                        return self.eval_sql_function(&func, &evaluated);
                    }
                }
                Err(SqlError::NotImplemented(format!("function: {name}")))
            }
        }
    }

    /// Execute a SQL-language user-defined function by substituting argument values
    /// into the body SQL and executing it.
    fn eval_sql_function(
        &self,
        func: &catalog::schema::FunctionDef,
        arg_values: &[Value],
    ) -> Result<Value, SqlError> {
        let mut sql = func.body_sql.clone();
        // Substitute $1, $2, ... with literal values
        for (i, val) in arg_values.iter().enumerate() {
            let placeholder = format!("${}", i + 1);
            let literal = value_to_sql_literal(val);
            sql = sql.replace(&placeholder, &literal);
        }
        // Also substitute named params if defined (positional)
        for (i, (param_name, _)) in func.params.iter().enumerate() {
            if let Some(val) = arg_values.get(i) {
                let literal = value_to_sql_literal(val);
                sql = sql.replace(param_name.as_str(), &literal);
            }
        }

        // Plan and execute the body
        let stmts = crate::parser::Parser::parse(sql.trim())?;
        let stmt = stmts
            .first()
            .ok_or_else(|| SqlError::Execution("empty function body".to_string()))?;
        let planner = crate::planner::Planner::new(Arc::clone(&self.ctx.catalog));
        let plan = planner.plan_statement(stmt)?;
        let rows = self.exec_plan(&plan)?;
        rows.into_iter()
            .next()
            .and_then(|r| r.values.into_iter().next())
            .ok_or_else(|| SqlError::Execution("function returned no value".to_string()))
    }


    fn eval_binary_op(&self, left: Value, op: &BinaryOp, right: Value) -> Result<Value, SqlError> {
        // NULL propagation
        if matches!((&left, op), (Value::Null, BinaryOp::And)) {
            if matches!(right, Value::Bool(false)) {
                return Ok(Value::Bool(false));
            }
            return Ok(Value::Null);
        }
        if matches!((&right, op), (Value::Null, BinaryOp::And)) {
            if matches!(left, Value::Bool(false)) {
                return Ok(Value::Bool(false));
            }
            return Ok(Value::Null);
        }
        if matches!((&left, op), (Value::Null, BinaryOp::Or)) {
            if matches!(right, Value::Bool(true)) {
                return Ok(Value::Bool(true));
            }
            return Ok(Value::Null);
        }
        if matches!((&right, op), (Value::Null, BinaryOp::Or)) {
            if matches!(left, Value::Bool(true)) {
                return Ok(Value::Bool(true));
            }
            return Ok(Value::Null);
        }
        if matches!(&left, Value::Null) || matches!(&right, Value::Null) {
            return Ok(Value::Null);
        }

        // Auto-coerce Text to match typed columns for comparison operators
        let (left, right) = match (&left, &right) {
            (Value::Date(_), Value::Text(s)) => {
                if let Some(d) = crate::value::parse_date_str(s) {
                    (left, Value::Date(d))
                } else {
                    (left, right)
                }
            }
            (Value::Text(s), Value::Date(_)) => {
                if let Some(d) = crate::value::parse_date_str(s) {
                    (Value::Date(d), right)
                } else {
                    (left, right)
                }
            }
            (Value::Timestamp(_), Value::Text(s)) => {
                if let Some(ts) = crate::value::parse_timestamp_str(s) {
                    (left, Value::Timestamp(ts))
                } else {
                    (left, right)
                }
            }
            (Value::Text(s), Value::Timestamp(_)) => {
                if let Some(ts) = crate::value::parse_timestamp_str(s) {
                    (Value::Timestamp(ts), right)
                } else {
                    (left, right)
                }
            }
            (Value::Uuid(_), Value::Text(s)) => (left, Value::Uuid(s.clone())),
            (Value::Text(s), Value::Uuid(_)) => (Value::Uuid(s.clone()), right),
            _ => (left, right),
        };

        match op {
            BinaryOp::Eq => Ok(Value::Bool(
                left == right || left.partial_cmp(&right) == Some(std::cmp::Ordering::Equal),
            )),
            BinaryOp::NotEq => Ok(Value::Bool(
                left != right && left.partial_cmp(&right) != Some(std::cmp::Ordering::Equal),
            )),
            BinaryOp::Lt => Ok(Value::Bool(
                left.partial_cmp(&right) == Some(std::cmp::Ordering::Less),
            )),
            BinaryOp::Le => Ok(Value::Bool(
                left.partial_cmp(&right) == Some(std::cmp::Ordering::Less)
                    || left.partial_cmp(&right) == Some(std::cmp::Ordering::Equal),
            )),
            BinaryOp::Gt => Ok(Value::Bool(
                left.partial_cmp(&right) == Some(std::cmp::Ordering::Greater),
            )),
            BinaryOp::Ge => Ok(Value::Bool(
                left.partial_cmp(&right) == Some(std::cmp::Ordering::Greater)
                    || left.partial_cmp(&right) == Some(std::cmp::Ordering::Equal),
            )),
            BinaryOp::And => match (&left, &right) {
                (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a && *b)),
                _ => Err(SqlError::TypeError(format!(
                    "AND requires booleans, got {left:?} and {right:?}"
                ))),
            },
            BinaryOp::Or => match (&left, &right) {
                (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a || *b)),
                _ => Err(SqlError::TypeError(format!(
                    "OR requires booleans, got {left:?} and {right:?}"
                ))),
            },
            BinaryOp::Add => checked_numeric_op(left, right, i32::checked_add, i64::checked_add, |a, b| a + b),
            BinaryOp::Sub => checked_numeric_op(left, right, i32::checked_sub, i64::checked_sub, |a, b| a - b),
            BinaryOp::Mul => checked_numeric_op(left, right, i32::checked_mul, i64::checked_mul, |a, b| a * b),
            BinaryOp::Div => match (&left, &right) {
                (_, Value::Int4(0)) | (_, Value::Int8(0)) => {
                    Err(SqlError::DivisionByZero)
                }
                (_, Value::Float8(f)) if *f == 0.0 => {
                    Err(SqlError::DivisionByZero)
                }
                _ => checked_numeric_op(left, right, i32::checked_div, i64::checked_div, |a, b| a / b),
            },
            BinaryOp::Concat => {
                // || operator: convert both sides to string and concatenate
                match (left, right) {
                    (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                    (l, r) => {
                        let ls = value_to_string(&l);
                        let rs = value_to_string(&r);
                        Ok(Value::Text(ls + &rs))
                    }
                }
            }
            BinaryOp::Mod => match (&left, &right) {
                (_, Value::Int4(0)) | (_, Value::Int8(0)) => Err(SqlError::DivisionByZero),
                (Value::Int4(a), Value::Int4(b)) => Ok(Value::Int4(a % b)),
                (Value::Int8(a), Value::Int8(b)) => Ok(Value::Int8(a % b)),
                (Value::Int4(a), Value::Int8(b)) => Ok(Value::Int8(*a as i64 % b)),
                (Value::Int8(a), Value::Int4(b)) => Ok(Value::Int8(a % *b as i64)),
                (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                _ => Err(SqlError::Execution("modulo requires integer operands".to_string())),
            },
        }
    }

    fn exec_window(
        &self,
        rows: Vec<Row>,
        window_exprs: &[WindowExpr],
    ) -> Result<Vec<Row>, SqlError> {
        if rows.is_empty() {
            return Ok(rows);
        }

        // We process each window expression and augment each row with the result.
        // For now, we process all window expressions together (they share the same partition/order).
        // If window exprs have different OVER clauses, they're computed independently.

        let mut result_rows = rows;

        for win_expr in window_exprs {
            result_rows = self.apply_window_expr(result_rows, win_expr)?;
        }

        Ok(result_rows)
    }

    fn apply_window_expr(
        &self,
        rows: Vec<Row>,
        win_expr: &WindowExpr,
    ) -> Result<Vec<Row>, SqlError> {
        // Group rows by partition key
        let mut partition_groups: Vec<(Vec<Value>, Vec<usize>)> = Vec::new();
        let mut partition_index: HashMap<Vec<OrderableValue>, usize> = HashMap::new();

        for (row_idx, row) in rows.iter().enumerate() {
            let key: Vec<Value> = win_expr.partition_by.iter()
                .map(|e| self.eval_expr(e, row))
                .collect::<Result<Vec<_>, _>>()?;
            let orderable_key: Vec<OrderableValue> = key.iter().map(|v| OrderableValue(v.clone())).collect();

            if let Some(&idx) = partition_index.get(&orderable_key) {
                partition_groups[idx].1.push(row_idx);
            } else {
                let idx = partition_groups.len();
                partition_index.insert(orderable_key, idx);
                partition_groups.push((key, vec![row_idx]));
            }
        }

        // For each partition, sort by order_by and compute window values
        // We need to collect all computed values indexed by row position
        let mut window_values: Vec<Value> = vec![Value::Null; rows.len()];

        for (_partition_key, row_indices) in &partition_groups {
            // Sort this partition's indices by order_by
            let mut sorted_indices = row_indices.clone();
            if !win_expr.order_by.is_empty() {
                let keys = &win_expr.order_by;
                let rows_ref = &rows;
                let mut sort_errors: Vec<SqlError> = Vec::new();
                sorted_indices.sort_by(|&ia, &ib| {
                    if !sort_errors.is_empty() { return std::cmp::Ordering::Equal; }
                    for key in keys {
                        let va = match self.eval_expr(&key.expr, &rows_ref[ia]) {
                            Ok(v) => v,
                            Err(e) => { sort_errors.push(e); return std::cmp::Ordering::Equal; }
                        };
                        let vb = match self.eval_expr(&key.expr, &rows_ref[ib]) {
                            Ok(v) => v,
                            Err(e) => { sort_errors.push(e); return std::cmp::Ordering::Equal; }
                        };
                        let ord = match (&va, &vb) {
                            (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
                            (Value::Null, _) => std::cmp::Ordering::Less,
                            (_, Value::Null) => std::cmp::Ordering::Greater,
                            _ => va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal),
                        };
                        if ord != std::cmp::Ordering::Equal {
                            return if key.ascending { ord } else { ord.reverse() };
                        }
                    }
                    std::cmp::Ordering::Equal
                });
                if let Some(e) = sort_errors.into_iter().next() {
                    return Err(e);
                }
            }

            // Compute window function values for this partition
            match &win_expr.function {
                WindowFunction::RowNumber => {
                    for (rank, &row_idx) in sorted_indices.iter().enumerate() {
                        window_values[row_idx] = Value::Int8((rank + 1) as i64);
                    }
                }
                WindowFunction::Rank => {
                    let mut rank = 1i64;
                    let mut prev_vals: Option<Vec<Value>> = None;
                    let mut next_rank = 1i64;
                    for &row_idx in &sorted_indices {
                        let cur_vals: Vec<Value> = win_expr.order_by.iter()
                            .map(|k| self.eval_expr(&k.expr, &rows[row_idx]))
                            .collect::<Result<Vec<_>, _>>()?;
                        if let Some(ref prev) = prev_vals {
                            if cur_vals == *prev {
                                window_values[row_idx] = Value::Int8(rank);
                            } else {
                                rank = next_rank;
                                window_values[row_idx] = Value::Int8(rank);
                                prev_vals = Some(cur_vals);
                            }
                        } else {
                            window_values[row_idx] = Value::Int8(rank);
                            prev_vals = Some(cur_vals);
                        }
                        next_rank += 1;
                    }
                }
                WindowFunction::DenseRank => {
                    let mut rank = 1i64;
                    let mut prev_vals: Option<Vec<Value>> = None;
                    for &row_idx in &sorted_indices {
                        let cur_vals: Vec<Value> = win_expr.order_by.iter()
                            .map(|k| self.eval_expr(&k.expr, &rows[row_idx]))
                            .collect::<Result<Vec<_>, _>>()?;
                        if let Some(ref prev) = prev_vals {
                            if cur_vals != *prev {
                                rank += 1;
                                prev_vals = Some(cur_vals);
                            }
                        } else {
                            prev_vals = Some(cur_vals);
                        }
                        window_values[row_idx] = Value::Int8(rank);
                    }
                }
                WindowFunction::Sum(arg_expr) => {
                    // Running sum (with ORDER BY) or full partition sum (without ORDER BY)
                    if win_expr.order_by.is_empty() {
                        // Full partition sum
                        let mut sum: Option<Value> = None;
                        for &row_idx in &sorted_indices {
                            let v = self.eval_expr(arg_expr, &rows[row_idx])?;
                            if !matches!(v, Value::Null) {
                                sum = Some(match sum {
                                    None => v,
                                    Some(acc) => self.eval_binary_op(acc, &BinaryOp::Add, v)?,
                                });
                            }
                        }
                        let s = sum.unwrap_or(Value::Null);
                        for &row_idx in &sorted_indices {
                            window_values[row_idx] = s.clone();
                        }
                    } else {
                        // Running sum
                        let mut running: Option<Value> = None;
                        for &row_idx in &sorted_indices {
                            let v = self.eval_expr(arg_expr, &rows[row_idx])?;
                            if !matches!(v, Value::Null) {
                                running = Some(match running {
                                    None => v,
                                    Some(acc) => self.eval_binary_op(acc, &BinaryOp::Add, v)?,
                                });
                            }
                            window_values[row_idx] = running.clone().unwrap_or(Value::Null);
                        }
                    }
                }
                WindowFunction::Count(arg_expr) => {
                    let n = sorted_indices.iter().filter(|&&idx| {
                        self.eval_expr(arg_expr, &rows[idx])
                            .map(|v| !matches!(v, Value::Null))
                            .unwrap_or(false)
                    }).count();
                    let count_val = Value::Int8(n as i64);
                    for &row_idx in &sorted_indices {
                        window_values[row_idx] = count_val.clone();
                    }
                }
                WindowFunction::Min(arg_expr) => {
                    let mut min: Option<Value> = None;
                    for &row_idx in &sorted_indices {
                        let v = self.eval_expr(arg_expr, &rows[row_idx])?;
                        if !matches!(v, Value::Null) {
                            min = Some(match min {
                                None => v.clone(),
                                Some(acc) => if v.partial_cmp(&acc) == Some(std::cmp::Ordering::Less) { v } else { acc },
                            });
                        }
                    }
                    let min_val = min.unwrap_or(Value::Null);
                    for &row_idx in &sorted_indices {
                        window_values[row_idx] = min_val.clone();
                    }
                }
                WindowFunction::Max(arg_expr) => {
                    let mut max: Option<Value> = None;
                    for &row_idx in &sorted_indices {
                        let v = self.eval_expr(arg_expr, &rows[row_idx])?;
                        if !matches!(v, Value::Null) {
                            max = Some(match max {
                                None => v.clone(),
                                Some(acc) => if v.partial_cmp(&acc) == Some(std::cmp::Ordering::Greater) { v } else { acc },
                            });
                        }
                    }
                    let max_val = max.unwrap_or(Value::Null);
                    for &row_idx in &sorted_indices {
                        window_values[row_idx] = max_val.clone();
                    }
                }
                WindowFunction::Avg(arg_expr) => {
                    let mut sum = 0.0f64;
                    let mut count = 0i64;
                    for &row_idx in &sorted_indices {
                        let v = self.eval_expr(arg_expr, &rows[row_idx])?;
                        let f = match v {
                            Value::Int4(i) => i as f64,
                            Value::Int8(i) => i as f64,
                            Value::Float8(f) => f,
                            _ => continue,
                        };
                        sum += f;
                        count += 1;
                    }
                    let avg_val = if count == 0 { Value::Null } else { Value::Float8(sum / count as f64) };
                    for &row_idx in &sorted_indices {
                        window_values[row_idx] = avg_val.clone();
                    }
                }
                WindowFunction::Lead { expr: arg_expr, offset, default } => {
                    let n = sorted_indices.len();
                    for (pos, &row_idx) in sorted_indices.iter().enumerate() {
                        let target_pos = pos as i64 + offset;
                        let val = if target_pos >= 0 && (target_pos as usize) < n {
                            self.eval_expr(arg_expr, &rows[sorted_indices[target_pos as usize]])?
                        } else if let Some(def_expr) = default {
                            self.eval_expr(def_expr, &rows[row_idx])?
                        } else {
                            Value::Null
                        };
                        window_values[row_idx] = val;
                    }
                }
                WindowFunction::Lag { expr: arg_expr, offset, default } => {
                    let n = sorted_indices.len();
                    for (pos, &row_idx) in sorted_indices.iter().enumerate() {
                        let target_pos = pos as i64 - offset;
                        let val = if target_pos >= 0 && (target_pos as usize) < n {
                            self.eval_expr(arg_expr, &rows[sorted_indices[target_pos as usize]])?
                        } else if let Some(def_expr) = default {
                            self.eval_expr(def_expr, &rows[row_idx])?
                        } else {
                            Value::Null
                        };
                        window_values[row_idx] = val;
                    }
                }
                WindowFunction::FirstValue(arg_expr) => {
                    let first_val = if !sorted_indices.is_empty() {
                        self.eval_expr(arg_expr, &rows[sorted_indices[0]])?
                    } else {
                        Value::Null
                    };
                    for &row_idx in &sorted_indices {
                        window_values[row_idx] = first_val.clone();
                    }
                }
                WindowFunction::LastValue(arg_expr) => {
                    let last_val = if !sorted_indices.is_empty() {
                        self.eval_expr(arg_expr, &rows[*sorted_indices.last().unwrap()])?
                    } else {
                        Value::Null
                    };
                    for &row_idx in &sorted_indices {
                        window_values[row_idx] = last_val.clone();
                    }
                }
                WindowFunction::NthValue { expr: arg_expr, n } => {
                    let idx_n = (n - 1).max(0) as usize;
                    let nth_val = if idx_n < sorted_indices.len() {
                        self.eval_expr(arg_expr, &rows[sorted_indices[idx_n]])?
                    } else {
                        Value::Null
                    };
                    for &row_idx in &sorted_indices {
                        window_values[row_idx] = nth_val.clone();
                    }
                }
                WindowFunction::CumeDist => {
                    let total = sorted_indices.len() as f64;
                    for (pos, &row_idx) in sorted_indices.iter().enumerate() {
                        window_values[row_idx] = Value::Float8((pos + 1) as f64 / total);
                    }
                }
                WindowFunction::PercentRank => {
                    let total = sorted_indices.len();
                    for (pos, &row_idx) in sorted_indices.iter().enumerate() {
                        let pr = if total <= 1 { 0.0 } else { pos as f64 / (total - 1) as f64 };
                        window_values[row_idx] = Value::Float8(pr);
                    }
                }
                WindowFunction::Ntile(bucket_expr) => {
                    let n_buckets = match self.eval_expr(bucket_expr, &rows[0])? {
                        Value::Int4(i) => i.max(1) as usize,
                        Value::Int8(i) => i.max(1) as usize,
                        _ => 1,
                    };
                    let total = sorted_indices.len();
                    for (pos, &row_idx) in sorted_indices.iter().enumerate() {
                        let bucket = (pos * n_buckets / total) + 1;
                        window_values[row_idx] = Value::Int8(bucket as i64);
                    }
                }
            }
        }

        // Append window column to each row
        let output_name = win_expr.output_name.clone();
        let result: Result<Vec<Row>, _> = rows.into_iter().enumerate().map(|(i, row)| {
            let mut new_values = row.values.clone();
            new_values.push(window_values[i].clone());
            let mut new_schema = row.schema.clone();
            new_schema.push((output_name.clone(), catalog::DataType::Int8));
            Ok(Row::new(new_values, new_schema))
        }).collect();
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn exec_recursive_cte(
        &self,
        name: &str,
        column_aliases: &[String],
        base_query: &LogicalPlan,
        recursive_query: &LogicalPlan,
        search_by_col: Option<&str>,
        search_set_col: Option<&str>,
        cycle_col: Option<&str>,
        cycle_set_col: Option<&str>,
        cycle_path_col: Option<&str>,
    ) -> Result<Vec<Row>, SqlError> {
        // Execute the base case
        let mut base_rows = self.exec_plan(base_query)?;
        if base_rows.is_empty() {
            return Ok(base_rows);
        }

        // Apply CTE column aliases declared in the signature (e.g. `series(n)`)
        // by renaming schema entries positionally.
        if !column_aliases.is_empty() {
            for row in &mut base_rows {
                if row.schema.len() == column_aliases.len() {
                    for (i, alias) in column_aliases.iter().enumerate() {
                        row.schema[i].0 = alias.clone();
                    }
                }
            }
        }

        let base_schema = base_rows.first().map(|r| r.schema.clone()).unwrap_or_default();
        let mut accumulated = base_rows.clone();
        // frontier = rows produced in the previous iteration (delta evaluation)
        let mut frontier = base_rows;

        // For CYCLE detection: track the set of cycle-column values already accumulated.
        let mut cycle_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        if let Some(c_col) = cycle_col {
            for row in &accumulated {
                let col_idx = row.schema.iter().position(|(n, _)| n == c_col).unwrap_or(0);
                if let Some(v) = row.values.get(col_idx) {
                    cycle_seen.insert(v.to_string());
                }
            }
        }

        const MAX_ITERATIONS: usize = 1000;
        for _iter in 0..MAX_ITERATIONS {
            // Expose only the frontier as the CTE (delta evaluation prevents infinite loops)
            let mut cte_map = HashMap::new();
            cte_map.insert(name.to_string(), frontier.clone());

            // Execute the recursive term with the current frontier
            let new_rows = self.exec_plan_with_ctes(recursive_query, &cte_map)?;

            if new_rows.is_empty() {
                break; // Fixed point reached
            }

            // Normalize schema to match base query's schema
            let mut normalized: Vec<Row> = new_rows.into_iter().map(|mut r| {
                if !base_schema.is_empty() {
                    r.schema = base_schema.clone();
                }
                r
            }).collect();

            // CYCLE: drop rows whose cycle_col value has already been seen; stop when all
            // candidates are cycles so we reach the fixed point immediately.
            if let Some(c_col) = cycle_col {
                let mut non_cycle: Vec<Row> = Vec::new();
                for row in normalized {
                    let col_idx = row.schema.iter().position(|(n, _)| n == c_col).unwrap_or(0);
                    let val_str = row.values.get(col_idx).map(|v| v.to_string()).unwrap_or_default();
                    if !cycle_seen.contains(&val_str) {
                        cycle_seen.insert(val_str);
                        non_cycle.push(row);
                    }
                    // rows that would form a cycle are silently dropped
                }
                normalized = non_cycle;
                if normalized.is_empty() {
                    break;
                }
            }

            frontier = normalized.clone();
            accumulated.extend(normalized);
        }

        // SEARCH DEPTH FIRST BY col SET order_col:
        // The recursive accumulation is already in depth-first order (each frontier batch
        // is appended in the order the recursive term produces it). Stamp a monotonically-
        // increasing sequence number into a new column named order_col.
        if let (Some(_by_col), Some(set_col)) = (search_by_col, search_set_col) {
            for (i, row) in accumulated.iter_mut().enumerate() {
                row.schema.push((set_col.to_string(), catalog::DataType::Int8));
                row.values.push(Value::Int8(i as i64));
            }
        }

        // CYCLE col SET cycle_col USING path_col:
        // After the loop, accumulated contains only non-cycle rows. Append the is_cycle
        // boolean (always false -- cycle rows were suppressed above) and a path column
        // whose value is the stringified cycle key for this row.
        if let (Some(c_col), Some(set_col), Some(path_col)) = (cycle_col, cycle_set_col, cycle_path_col) {
            for row in accumulated.iter_mut() {
                let col_idx = row.schema.iter().position(|(n, _)| n == c_col).unwrap_or(0);
                let val_str = row.values.get(col_idx).map(|v| v.to_string()).unwrap_or_default();
                row.schema.push((set_col.to_string(), catalog::DataType::Boolean));
                row.values.push(Value::Bool(false));
                row.schema.push((path_col.to_string(), catalog::DataType::Text));
                row.values.push(Value::Text(format!("{{{}}}", val_str)));
            }
        }

        Ok(accumulated)
    }

    fn exec_alter_table(
        &self,
        table_name: &str,
        operation: &AlterTableOp,
    ) -> Result<(), SqlError> {
        match operation {
            AlterTableOp::AddColumn { name, data_type: _, nullable: _ } => {
                self.ctx.catalog.alter_table_add_column(
                    self.ctx.xid,
                    "public",
                    table_name,
                    name,
                ).map_err(SqlError::Catalog)
            }
            AlterTableOp::DropColumn { name } => {
                self.ctx.catalog.alter_table_drop_column(
                    self.ctx.xid,
                    "public",
                    table_name,
                    name,
                ).map_err(SqlError::Catalog)
            }
            AlterTableOp::RenameColumn { old_name, new_name } => {
                self.ctx.catalog.alter_table_rename_column(
                    self.ctx.xid,
                    "public",
                    table_name,
                    old_name,
                    new_name,
                ).map_err(SqlError::Catalog)
            }
            AlterTableOp::RenameTable { new_name } => {
                self.ctx.catalog.alter_table_rename_table(
                    self.ctx.xid,
                    "public",
                    table_name,
                    new_name,
                ).map_err(SqlError::Catalog)
            }
        }
    }

    fn open_heap(&self, table_oid: u32) -> Result<HeapFile, SqlError> {
        let path = self.ctx.data_dir.join(format!("{table_oid}.heap"));
        HeapFile::open(&path).map_err(|e| SqlError::Storage(storage::error::StorageError::Heap(e)))
    }

    fn exec_generate_series(
        &self,
        start: &Expr,
        stop: &Expr,
        step: &Expr,
    ) -> Result<Vec<Row>, SqlError> {
        let empty_row = Row::new(vec![], vec![]);
        let start_val = self.eval_expr(start, &empty_row)?;
        let stop_val = self.eval_expr(stop, &empty_row)?;
        let step_val = self.eval_expr(step, &empty_row)?;

        let to_i64 = |v: &Value| -> Result<i64, SqlError> {
            match v {
                Value::Int4(i) => Ok(*i as i64),
                Value::Int8(i) => Ok(*i),
                Value::Float8(f) => Ok(*f as i64),
                other => Err(SqlError::Execution(format!(
                    "generate_series: expected integer, got {:?}", other
                ))),
            }
        };

        let start_i = to_i64(&start_val)?;
        let stop_i = to_i64(&stop_val)?;
        let step_i = to_i64(&step_val)?;

        if step_i == 0 {
            return Err(SqlError::Execution(
                "generate_series: step cannot be 0".to_string(),
            ));
        }

        let schema = vec![("generate_series".to_string(), catalog::DataType::Int8)];
        let mut rows = Vec::new();
        let mut i = start_i;
        while (step_i > 0 && i <= stop_i) || (step_i < 0 && i >= stop_i) {
            rows.push(Row::new(vec![Value::Int8(i)], schema.clone()));
            i += step_i;
        }
        Ok(rows)
    }

    /// Evaluate a default expression string for a column of the given data type.
    fn eval_default_expr(&self, expr_str: &str, dtype: &catalog::DataType) -> Result<Value, SqlError> {
        let s = expr_str.trim();
        // Check for common function calls
        let upper = s.to_uppercase();
        if upper == "NOW()" || upper == "CURRENT_TIMESTAMP" || upper == "NOW" {
            let micros = current_timestamp_micros();
            return Ok(Value::Timestamp(micros));
        }
        if upper == "CURRENT_DATE" {
            let days = current_date_days();
            return Ok(Value::Date(days));
        }
        if upper.starts_with("GEN_RANDOM_UUID") || upper.starts_with("UUID_GENERATE") {
            return Ok(Value::Uuid(generate_uuid()));
        }
        // Try to parse as a SQL literal
        // Remove surrounding quotes for string literals
        if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
            let inner = &s[1..s.len()-1];
            return Value::Text(inner.to_string()).cast_to(dtype).or(Ok(Value::Text(inner.to_string())));
        }
        // Try as number
        if let Ok(i) = s.parse::<i64>() {
            return Value::Int8(i).cast_to(dtype).or(Ok(Value::Int8(i)));
        }
        if let Ok(f) = s.parse::<f64>() {
            return Value::Float8(f).cast_to(dtype).or(Ok(Value::Float8(f)));
        }
        if s.eq_ignore_ascii_case("true") {
            return Ok(Value::Bool(true));
        }
        if s.eq_ignore_ascii_case("false") {
            return Ok(Value::Bool(false));
        }
        if s.eq_ignore_ascii_case("null") {
            return Ok(Value::Null);
        }
        // Return Null for anything else
        Ok(Value::Null)
    }

    /// Build a B+ tree index on `column_name` for `table_name`.
    fn exec_create_index(
        &self,
        schema_name: &str,
        table_name: &str,
        column_name: &str,
    ) -> Result<(), SqlError> {
        let schema = self.ctx.catalog.get_table(schema_name, table_name)?;
        let table_oid = self.ctx.catalog.get_table_oid(schema_name, table_name)?;

        // For multi-column indexes ("a, b"), extract the leading column for actual index building.
        // The index is registered under the full spec so that single-column equality queries
        // (which may have duplicate leading-column values) will NOT incorrectly use this index.
        let leading_col = column_name.split(',').next().unwrap_or(column_name).trim();

        let col_def = schema
            .column_by_name(leading_col)
            .ok_or_else(|| SqlError::ColumnNotFound(leading_col.to_string()))?;
        let col_idx = (col_def.attnum as usize).saturating_sub(1);

        let index_path = self.ctx.catalog.create_index_entry(table_oid, column_name);

        let wal =
            Arc::new(wal::writer::WalWriter::open(&self.ctx.data_dir).map_err(SqlError::Wal)?);
        let btree = BTree::open(&index_path, wal, table_oid)
            .map_err(|e| SqlError::Execution(e.to_string()))?;

        // Scan existing tuples and insert into the index.
        let mut heap = self.open_heap(table_oid)?;
        let visible = self
            .ctx
            .txn_manager
            .scan_visible_tuples(self.ctx.xid, &mut heap)
            .map_err(SqlError::Txn)?;

        for (tid, tuple) in visible {
            let data = &tuple.data;
            let null_bitmap = if data.len() >= 4 {
                u32::from_le_bytes(data[0..4].try_into().unwrap())
            } else {
                0
            };
            let row_data = if data.len() >= 4 { &data[4..] } else { data };
            let row = decode_row(row_data, null_bitmap, &schema)?;
            if let Some(val) = row.get_by_idx(col_idx) {
                if !matches!(val, Value::Null) {
                    let key = encode_sort_key(val);
                    // Ignore duplicate key errors (may happen on re-index).
                    let _ = btree.insert(self.ctx.xid, &key, tid);
                }
            }
        }

        Ok(())
    }

    /// Execute an index scan using the B+ tree index.
    #[allow(clippy::too_many_arguments)]
    fn exec_index_scan(
        &self,
        table_name: &str,
        schema: &catalog::schema::TableSchema,
        index_column: &str,
        eq_value: &Option<Value>,
        range_start: &Option<Value>,
        range_end: &Option<Value>,
        filter: &Option<Expr>,
    ) -> Result<Vec<Row>, SqlError> {
        let table_oid = self.ctx.catalog.get_table_oid("public", table_name)?;

        let index_path = self
            .ctx
            .catalog
            .get_index_path(table_oid, index_column)
            .ok_or_else(|| {
                SqlError::Execution(format!("No index on {}.{}", table_name, index_column))
            })?;

        let wal =
            Arc::new(wal::writer::WalWriter::open(&self.ctx.data_dir).map_err(SqlError::Wal)?);
        let btree = BTree::open(&index_path, wal, table_oid)
            .map_err(|e| SqlError::Execution(e.to_string()))?;

        // Look up TIDs from the index.
        let tids: Vec<storage::tid::TID> = if let Some(eq_val) = eq_value {
            // Use range_scan to find all TIDs with this exact key value (handles duplicate keys).
            let key = encode_sort_key(eq_val);
            btree
                .range_scan(Some(&key), Some(&key))
                .map_err(|e| SqlError::Execution(e.to_string()))?
                .into_iter()
                .map(|(_, tid)| tid)
                .collect()
        } else {
            let start = range_start.as_ref().map(encode_sort_key);
            let end = range_end.as_ref().map(encode_sort_key);
            btree
                .range_scan(start.as_deref(), end.as_deref())
                .map_err(|e| SqlError::Execution(e.to_string()))?
                .into_iter()
                .map(|(_, tid)| tid)
                .collect()
        };

        // Fetch and visibility-check each tuple from the heap.
        let mut heap = self.open_heap(table_oid)?;
        let mut rows = Vec::new();

        for tid in tids {
            let tuple = match heap.get_tuple(tid) {
                Ok(t) => t,
                Err(_) => continue,
            };

            let h = &tuple.header;
            let visible = if h.t_xmin == self.ctx.xid {
                h.t_xmax != self.ctx.xid
            } else {
                self.ctx.txn_manager.is_committed(h.t_xmin)
                    && (h.t_xmax == 0 || !self.ctx.txn_manager.is_committed(h.t_xmax))
            };

            if !visible {
                continue;
            }

            let data = &tuple.data;
            let null_bitmap = if data.len() >= 4 {
                u32::from_le_bytes(data[0..4].try_into().unwrap())
            } else {
                0
            };
            let row_data = if data.len() >= 4 { &data[4..] } else { data };
            let row = decode_row(row_data, null_bitmap, schema)?;

            if let Some(pred) = filter {
                if !matches!(self.eval_expr(pred, &row)?, Value::Bool(true)) {
                    continue;
                }
            }
            rows.push(row);
        }

        Ok(rows)
    }

    fn exec_grant(
        &self,
        schema: &str,
        table: &str,
        grantee: &str,
        privileges: &[String],
        columns: &[String],
    ) -> Result<(), SqlError> {
        let privs = privileges
            .iter()
            .map(|p| Self::parse_acl_privilege(p))
            .collect::<Result<Vec<_>, _>>()?;
        if columns.is_empty() {
            // Table-level grant
            self.ctx
                .catalog
                .grant_table(schema, table, grantee, privs)
                .map_err(SqlError::Catalog)
        } else {
            // Column-level grant
            self.ctx
                .catalog
                .grant_column_privileges(schema, table, grantee, columns, &privs)
                .map_err(SqlError::Catalog)
        }
    }

    fn exec_revoke(
        &self,
        schema: &str,
        table: &str,
        grantee: &str,
        privileges: &[String],
        columns: &[String],
    ) -> Result<(), SqlError> {
        let privs = privileges
            .iter()
            .map(|p| Self::parse_acl_privilege(p))
            .collect::<Result<Vec<_>, _>>()?;
        if columns.is_empty() {
            // Table-level revoke
            self.ctx
                .catalog
                .revoke_table(schema, table, grantee, privs)
                .map_err(SqlError::Catalog)
        } else {
            // Column-level revoke
            self.ctx
                .catalog
                .revoke_column_privileges(schema, table, grantee, columns, &privs)
                .map_err(SqlError::Catalog)
        }
    }

    fn parse_acl_privilege(s: &str) -> Result<catalog::manager::AclPrivilege, SqlError> {
        match s.to_uppercase().as_str() {
            "SELECT" => Ok(catalog::manager::AclPrivilege::Select),
            "INSERT" => Ok(catalog::manager::AclPrivilege::Insert),
            "UPDATE" => Ok(catalog::manager::AclPrivilege::Update),
            "DELETE" => Ok(catalog::manager::AclPrivilege::Delete),
            "ALL" | "ALL PRIVILEGES" => Ok(catalog::manager::AclPrivilege::All),
            other => Err(SqlError::Execution(format!("Unknown privilege: {}", other))),
        }
    }

    fn exec_vacuum(
        &self,
        schema: &str,
        table_name: Option<&str>,
        analyze: bool,
    ) -> Result<u64, SqlError> {
        let tables_to_vacuum: Vec<String> = if let Some(tbl) = table_name {
            vec![tbl.to_string()]
        } else {
            self.ctx
                .catalog
                .list_tables(schema)
                .map_err(SqlError::Catalog)?
        };

        let mut total_dead = 0u64;

        for table in &tables_to_vacuum {
            let schema_info = match self.ctx.catalog.get_table(schema, table) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let table_oid = schema_info.oid;
            let mut heap = self.open_heap(table_oid)?;

            // Get the set of committed XIDs for dead tuple detection
            let committed = self.ctx.txn_manager.committed_set();

            let num_pages = heap.num_pages();
            for page_no in 0..num_pages {
                let mut page = match heap.read_page(page_no) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let num_slots = page.num_slots();
                let mut page_had_dead = false;

                for slot in 0..num_slots {
                    let bytes_owned: Vec<u8> = match page.get_tuple(slot) {
                        Ok(b) => b.to_vec(),
                        Err(_) => continue, // already dead
                    };
                    if bytes_owned.len() < storage::tuple::TUPLE_HEADER_SIZE {
                        continue;
                    }
                    let header = match storage::tuple::TupleHeader::decode_from(&bytes_owned) {
                        Ok(h) => h,
                        Err(_) => continue,
                    };
                    // Dead tuple: t_xmax != 0 AND deleting txn is committed
                    if header.t_xmax != 0 && committed.contains(&header.t_xmax) {
                        // Mark slot as dead in the page
                        page.mark_dead(slot);
                        total_dead += 1;
                        page_had_dead = true;
                    }
                }

                if page_had_dead {
                    // Update pd_prune_xid to indicate this page has been vacuumed
                    page.set_prune_xid(self.ctx.xid);
                    let _ = heap.write_page(page_no, &page);
                }
            }
        }

        if analyze {
            for table in &tables_to_vacuum {
                let schema_info = match self.ctx.catalog.get_table(schema, table) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let table_oid = schema_info.oid;
                let col_defs = schema_info.columns.clone();

                // Use scan_visible_tuples to collect all live rows
                let mut heap = match self.open_heap(table_oid) {
                    Ok(h) => h,
                    Err(_) => continue,
                };
                let visible = match self
                    .ctx
                    .txn_manager
                    .scan_visible_tuples(self.ctx.xid, &mut heap)
                {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                // Decode all live tuples into per-column string values
                let mut all_rows: Vec<Vec<Option<String>>> = Vec::new();
                for (_tid, tuple) in visible {
                    let data = &tuple.data;
                    let row = if data.len() < 4 {
                        match decode_row(data, 0, &schema_info) {
                            Ok(r) => r,
                            Err(_) => continue,
                        }
                    } else {
                        let null_bitmap =
                            u32::from_le_bytes(data[0..4].try_into().unwrap());
                        let row_data = &data[4..];
                        match decode_row(row_data, null_bitmap, &schema_info) {
                            Ok(r) => r,
                            Err(_) => continue,
                        }
                    };
                    let str_vals: Vec<Option<String>> = row
                        .values
                        .iter()
                        .map(|v| match v {
                            Value::Null => None,
                            other => Some(other.to_string()),
                        })
                        .collect();
                    all_rows.push(str_vals);
                }

                let total = all_rows.len();
                let mut col_stats_map = std::collections::HashMap::new();

                for (col_idx, col_def) in col_defs.iter().enumerate() {
                    if total == 0 {
                        col_stats_map.insert(
                            col_def.name.clone(),
                            catalog::ColumnStats {
                                null_frac: 0.0,
                                n_distinct: 0.0,
                                most_common_vals: vec![],
                                most_common_freqs: vec![],
                            },
                        );
                        continue;
                    }

                    let vals: Vec<Option<String>> = all_rows
                        .iter()
                        .map(|row| row.get(col_idx).cloned().flatten())
                        .collect();

                    let null_count = vals.iter().filter(|v| v.is_none()).count();
                    let null_frac = null_count as f64 / total as f64;

                    let non_null: Vec<&str> =
                        vals.iter().filter_map(|v| v.as_deref()).collect();

                    // Count distinct values and their frequencies
                    let mut freq_map: std::collections::HashMap<String, usize> =
                        std::collections::HashMap::new();
                    for v in &non_null {
                        *freq_map.entry(v.to_string()).or_insert(0) += 1;
                    }
                    let n_distinct = freq_map.len() as f64;

                    // Most common values: top 10 by frequency
                    let mut freqs: Vec<(String, usize)> =
                        freq_map.into_iter().collect();
                    freqs.sort_by(|a, b| b.1.cmp(&a.1));
                    freqs.truncate(10);

                    let non_null_count = non_null.len();
                    let most_common_vals: Vec<String> =
                        freqs.iter().map(|(v, _)| v.clone()).collect();
                    let most_common_freqs: Vec<f64> = freqs
                        .iter()
                        .map(|(_, c)| *c as f64 / non_null_count.max(1) as f64)
                        .collect();

                    col_stats_map.insert(
                        col_def.name.clone(),
                        catalog::ColumnStats {
                            null_frac,
                            n_distinct,
                            most_common_vals,
                            most_common_freqs,
                        },
                    );
                }

                self.ctx.catalog.store_table_stats(table, col_stats_map);
                log::info!("ANALYZE: collected statistics for table '{}'", table);
            }
        }

        // Record vacuum timestamps for autovacuum tracking
        for table in &tables_to_vacuum {
            self.ctx.catalog.record_vacuum(table);
        }

        Ok(total_dead)
    }

    // ── System catalog virtual scans ──────────────────────────────────────────

    fn exec_system_catalog_scan(
        &self,
        catalog_name: &str,
        table_name: &str,
        filter: Option<&crate::plan::Expr>,
    ) -> Result<Vec<Row>, SqlError> {
        let rows = self.build_system_catalog_rows(catalog_name, table_name)?;
        if let Some(pred) = filter {
            self.exec_filter(rows, pred)
        } else {
            Ok(rows)
        }
    }

    fn build_system_catalog_rows(
        &self,
        catalog_name: &str,
        table_name: &str,
    ) -> Result<Vec<Row>, SqlError> {
        use catalog::DataType;

        match (catalog_name, table_name) {
            // ── information_schema.tables ──────────────────────────────────
            ("information_schema", "tables") => {
                let schema_cols: Vec<(String, DataType)> = vec![
                    ("table_catalog".into(), DataType::Text),
                    ("table_schema".into(), DataType::Text),
                    ("table_name".into(), DataType::Text),
                    ("table_type".into(), DataType::Text),
                ];
                let tables = self.ctx.catalog.list_tables("public")
                    .map_err(SqlError::Catalog)?;
                let mut rows = Vec::new();
                for tbl in tables {
                    rows.push(Row::new(
                        vec![
                            Value::Text("icedb".into()),
                            Value::Text("public".into()),
                            Value::Text(tbl),
                            Value::Text("BASE TABLE".into()),
                        ],
                        schema_cols.clone(),
                    ));
                }
                Ok(rows)
            }

            // ── information_schema.columns ─────────────────────────────────
            ("information_schema", "columns") => {
                let schema_cols: Vec<(String, DataType)> = vec![
                    ("table_catalog".into(), DataType::Text),
                    ("table_schema".into(), DataType::Text),
                    ("table_name".into(), DataType::Text),
                    ("column_name".into(), DataType::Text),
                    ("ordinal_position".into(), DataType::Int4),
                    ("is_nullable".into(), DataType::Text),
                    ("data_type".into(), DataType::Text),
                    ("column_default".into(), DataType::Text),
                ];
                let tables = self.ctx.catalog.list_tables("public")
                    .map_err(SqlError::Catalog)?;
                let mut rows = Vec::new();
                for tbl in tables {
                    if let Ok(ts) = self.ctx.catalog.get_table("public", &tbl) {
                        for col in &ts.columns {
                            let type_name = datatype_name(&col.data_type).to_string();
                            let nullable = if col.not_null { "NO" } else { "YES" };
                            let default_val = if col.has_default {
                                Value::Text("".into())
                            } else {
                                Value::Null
                            };
                            rows.push(Row::new(
                                vec![
                                    Value::Text("icedb".into()),
                                    Value::Text("public".into()),
                                    Value::Text(tbl.clone()),
                                    Value::Text(col.name.clone()),
                                    Value::Int4(col.attnum as i32),
                                    Value::Text(nullable.into()),
                                    Value::Text(type_name),
                                    default_val,
                                ],
                                schema_cols.clone(),
                            ));
                        }
                    }
                }
                Ok(rows)
            }

            // ── information_schema.schemata ────────────────────────────────
            ("information_schema", "schemata") => {
                let schema_cols: Vec<(String, DataType)> = vec![
                    ("catalog_name".into(), DataType::Text),
                    ("schema_name".into(), DataType::Text),
                    ("schema_owner".into(), DataType::Text),
                ];
                let ns_rows = self.ctx.catalog.list_namespaces()
                    .map_err(SqlError::Catalog)?;
                let mut rows = Vec::new();
                for ns in ns_rows {
                    rows.push(Row::new(
                        vec![
                            Value::Text("icedb".into()),
                            Value::Text(ns.nspname.clone()),
                            Value::Text("icedb".into()),
                        ],
                        schema_cols.clone(),
                    ));
                }
                Ok(rows)
            }

            // ── information_schema.table_constraints ───────────────────────
            ("information_schema", "table_constraints") => {
                let schema_cols: Vec<(String, DataType)> = vec![
                    ("constraint_catalog".into(), DataType::Text),
                    ("constraint_schema".into(), DataType::Text),
                    ("constraint_name".into(), DataType::Text),
                    ("table_catalog".into(), DataType::Text),
                    ("table_schema".into(), DataType::Text),
                    ("table_name".into(), DataType::Text),
                    ("constraint_type".into(), DataType::Text),
                ];
                let tables = self.ctx.catalog.list_tables("public")
                    .map_err(SqlError::Catalog)?;
                let mut rows = Vec::new();
                for tbl in &tables {
                    if let Ok(ts) = self.ctx.catalog.get_table("public", tbl) {
                        // Primary key
                        if let Some(uc) = self.ctx.catalog.get_unique_constraints(ts.oid) {
                            if let Some(ref pk) = uc.primary_key {
                                let cname = format!("{}_pkey", tbl);
                                rows.push(Row::new(
                                    vec![
                                        Value::Text("icedb".into()),
                                        Value::Text("public".into()),
                                        Value::Text(cname),
                                        Value::Text("icedb".into()),
                                        Value::Text("public".into()),
                                        Value::Text(tbl.clone()),
                                        Value::Text("PRIMARY KEY".into()),
                                    ],
                                    schema_cols.clone(),
                                ));
                                let _ = pk;
                            }
                            for uk in &uc.unique_columns {
                                let cname = format!("{}_{}_key", tbl, uk);
                                rows.push(Row::new(
                                    vec![
                                        Value::Text("icedb".into()),
                                        Value::Text("public".into()),
                                        Value::Text(cname),
                                        Value::Text("icedb".into()),
                                        Value::Text("public".into()),
                                        Value::Text(tbl.clone()),
                                        Value::Text("UNIQUE".into()),
                                    ],
                                    schema_cols.clone(),
                                ));
                            }
                        }
                        // Foreign keys
                        let fks = self.ctx.catalog.get_foreign_keys(ts.oid);
                        for fk in &fks {
                            let cname = format!("{}_{}_fkey", tbl, fk.local_col);
                            rows.push(Row::new(
                                vec![
                                    Value::Text("icedb".into()),
                                    Value::Text("public".into()),
                                    Value::Text(cname),
                                    Value::Text("icedb".into()),
                                    Value::Text("public".into()),
                                    Value::Text(tbl.clone()),
                                    Value::Text("FOREIGN KEY".into()),
                                ],
                                schema_cols.clone(),
                            ));
                        }
                        // Check constraints
                        let checks = self.ctx.catalog.get_check_constraints(ts.oid);
                        for (i, chk) in checks.iter().enumerate() {
                            let cname = chk.name.clone()
                                .unwrap_or_else(|| format!("{}_check{}", tbl, i));
                            rows.push(Row::new(
                                vec![
                                    Value::Text("icedb".into()),
                                    Value::Text("public".into()),
                                    Value::Text(cname),
                                    Value::Text("icedb".into()),
                                    Value::Text("public".into()),
                                    Value::Text(tbl.clone()),
                                    Value::Text("CHECK".into()),
                                ],
                                schema_cols.clone(),
                            ));
                        }
                    }
                }
                Ok(rows)
            }

            // ── information_schema.key_column_usage ───────────────────────
            ("information_schema", "key_column_usage") => {
                let schema_cols: Vec<(String, DataType)> = vec![
                    ("constraint_catalog".into(), DataType::Text),
                    ("constraint_schema".into(), DataType::Text),
                    ("constraint_name".into(), DataType::Text),
                    ("table_catalog".into(), DataType::Text),
                    ("table_schema".into(), DataType::Text),
                    ("table_name".into(), DataType::Text),
                    ("column_name".into(), DataType::Text),
                    ("ordinal_position".into(), DataType::Int4),
                ];
                let tables = self.ctx.catalog.list_tables("public")
                    .map_err(SqlError::Catalog)?;
                let mut rows = Vec::new();
                for tbl in &tables {
                    if let Ok(ts) = self.ctx.catalog.get_table("public", tbl) {
                        if let Some(uc) = self.ctx.catalog.get_unique_constraints(ts.oid) {
                            if let Some(ref pk) = uc.primary_key {
                                let cname = format!("{}_pkey", tbl);
                                rows.push(Row::new(
                                    vec![
                                        Value::Text("icedb".into()),
                                        Value::Text("public".into()),
                                        Value::Text(cname),
                                        Value::Text("icedb".into()),
                                        Value::Text("public".into()),
                                        Value::Text(tbl.clone()),
                                        Value::Text(pk.clone()),
                                        Value::Int4(1),
                                    ],
                                    schema_cols.clone(),
                                ));
                            }
                        }
                        let fks = self.ctx.catalog.get_foreign_keys(ts.oid);
                        for fk in &fks {
                            let cname = format!("{}_{}_fkey", tbl, fk.local_col);
                            rows.push(Row::new(
                                vec![
                                    Value::Text("icedb".into()),
                                    Value::Text("public".into()),
                                    Value::Text(cname),
                                    Value::Text("icedb".into()),
                                    Value::Text("public".into()),
                                    Value::Text(tbl.clone()),
                                    Value::Text(fk.local_col.clone()),
                                    Value::Int4(1),
                                ],
                                schema_cols.clone(),
                            ));
                        }
                    }
                }
                Ok(rows)
            }

            // ── information_schema.referential_constraints ─────────────────
            ("information_schema", "referential_constraints") => {
                let schema_cols: Vec<(String, DataType)> = vec![
                    ("constraint_catalog".into(), DataType::Text),
                    ("constraint_schema".into(), DataType::Text),
                    ("constraint_name".into(), DataType::Text),
                    ("unique_constraint_catalog".into(), DataType::Text),
                    ("unique_constraint_schema".into(), DataType::Text),
                    ("unique_constraint_name".into(), DataType::Text),
                    ("match_option".into(), DataType::Text),
                    ("update_rule".into(), DataType::Text),
                    ("delete_rule".into(), DataType::Text),
                ];
                let tables = self.ctx.catalog.list_tables("public")
                    .map_err(SqlError::Catalog)?;
                let mut rows = Vec::new();
                for tbl in &tables {
                    if let Ok(ts) = self.ctx.catalog.get_table("public", tbl) {
                        let fks = self.ctx.catalog.get_foreign_keys(ts.oid);
                        for fk in &fks {
                            let cname = format!("{}_{}_fkey", tbl, fk.local_col);
                            let ref_cname = format!("{}_pkey", fk.ref_table);
                            let delete_rule = match fk.on_delete {
                                catalog::schema::FkAction::Cascade => "CASCADE",
                                catalog::schema::FkAction::SetNull => "SET NULL",
                                catalog::schema::FkAction::SetDefault => "SET DEFAULT",
                                catalog::schema::FkAction::Restrict => "RESTRICT",
                                catalog::schema::FkAction::NoAction => "NO ACTION",
                            };
                            rows.push(Row::new(
                                vec![
                                    Value::Text("icedb".into()),
                                    Value::Text("public".into()),
                                    Value::Text(cname),
                                    Value::Text("icedb".into()),
                                    Value::Text("public".into()),
                                    Value::Text(ref_cname),
                                    Value::Text("NONE".into()),
                                    Value::Text("NO ACTION".into()),
                                    Value::Text(delete_rule.into()),
                                ],
                                schema_cols.clone(),
                            ));
                        }
                    }
                }
                Ok(rows)
            }

            // ── pg_catalog.pg_tables ───────────────────────────────────────
            ("pg_catalog", "pg_tables") => {
                let schema_cols: Vec<(String, DataType)> = vec![
                    ("schemaname".into(), DataType::Text),
                    ("tablename".into(), DataType::Text),
                    ("tableowner".into(), DataType::Text),
                    ("hasindexes".into(), DataType::Boolean),
                    ("hasrules".into(), DataType::Boolean),
                    ("hastriggers".into(), DataType::Boolean),
                    ("rowsecurity".into(), DataType::Boolean),
                ];
                let tables = self.ctx.catalog.list_tables("public")
                    .map_err(SqlError::Catalog)?;
                let indexes = self.ctx.catalog.list_indexes();
                let mut rows = Vec::new();
                for tbl in tables {
                    let ts_oid = self.ctx.catalog.get_table_oid("public", &tbl).unwrap_or(0);
                    let has_idx = indexes.iter().any(|(oid, _, _)| *oid == ts_oid);
                    rows.push(Row::new(
                        vec![
                            Value::Text("public".into()),
                            Value::Text(tbl),
                            Value::Text("icedb".into()),
                            Value::Bool(has_idx),
                            Value::Bool(false),
                            Value::Bool(false),
                            Value::Bool(false),
                        ],
                        schema_cols.clone(),
                    ));
                }
                Ok(rows)
            }

            // ── pg_catalog.pg_class ────────────────────────────────────────
            ("pg_catalog", "pg_class") => {
                let schema_cols: Vec<(String, DataType)> = vec![
                    ("oid".into(), DataType::Int4),
                    ("relname".into(), DataType::Text),
                    ("relnamespace".into(), DataType::Int4),
                    ("relkind".into(), DataType::Text),
                    ("relnatts".into(), DataType::Int4),
                    ("relpages".into(), DataType::Int4),
                    ("reltuples".into(), DataType::Float8),
                ];
                let class_rows = self.ctx.catalog.list_pg_class_rows()
                    .map_err(SqlError::Catalog)?;
                let mut rows = Vec::new();
                for cr in class_rows {
                    rows.push(Row::new(
                        vec![
                            Value::Int4(cr.oid as i32),
                            Value::Text(cr.relname),
                            Value::Int4(cr.relnamespace as i32),
                            Value::Text((cr.relkind as char).to_string()),
                            Value::Int4(cr.relnatts as i32),
                            Value::Int4(cr.relpages as i32),
                            Value::Float8(cr.reltuples),
                        ],
                        schema_cols.clone(),
                    ));
                }
                Ok(rows)
            }

            // ── pg_catalog.pg_attribute ────────────────────────────────────
            ("pg_catalog", "pg_attribute") => {
                let schema_cols: Vec<(String, DataType)> = vec![
                    ("attrelid".into(), DataType::Int4),
                    ("attname".into(), DataType::Text),
                    ("atttypid".into(), DataType::Int4),
                    ("atttypmod".into(), DataType::Int4),
                    ("attnum".into(), DataType::Int4),
                    ("attnotnull".into(), DataType::Boolean),
                    ("attisdropped".into(), DataType::Boolean),
                    ("atthasdef".into(), DataType::Boolean),
                ];
                let attr_rows = self.ctx.catalog.list_pg_attribute_rows()
                    .map_err(SqlError::Catalog)?;
                let mut rows = Vec::new();
                for ar in attr_rows {
                    rows.push(Row::new(
                        vec![
                            Value::Int4(ar.attrelid as i32),
                            Value::Text(ar.attname),
                            Value::Int4(ar.atttypid as i32),
                            Value::Int4(ar.atttypmod),
                            Value::Int4(ar.attnum as i32),
                            Value::Bool(ar.attnotnull),
                            Value::Bool(ar.attisdropped),
                            Value::Bool(ar.atthasdef),
                        ],
                        schema_cols.clone(),
                    ));
                }
                Ok(rows)
            }

            // ── pg_catalog.pg_namespace ────────────────────────────────────
            ("pg_catalog", "pg_namespace") => {
                let schema_cols: Vec<(String, DataType)> = vec![
                    ("oid".into(), DataType::Int4),
                    ("nspname".into(), DataType::Text),
                    ("nspowner".into(), DataType::Int4),
                ];
                let ns_rows = self.ctx.catalog.list_namespaces()
                    .map_err(SqlError::Catalog)?;
                let mut rows = Vec::new();
                for ns in ns_rows {
                    rows.push(Row::new(
                        vec![
                            Value::Int4(ns.oid as i32),
                            Value::Text(ns.nspname),
                            Value::Int4(ns.nspowner as i32),
                        ],
                        schema_cols.clone(),
                    ));
                }
                Ok(rows)
            }

            // ── pg_catalog.pg_indexes ──────────────────────────────────────
            ("pg_catalog", "pg_indexes") => {
                let schema_cols: Vec<(String, DataType)> = vec![
                    ("schemaname".into(), DataType::Text),
                    ("tablename".into(), DataType::Text),
                    ("indexname".into(), DataType::Text),
                    ("indexdef".into(), DataType::Text),
                ];
                let indexes = self.ctx.catalog.list_indexes();
                let class_rows = self.ctx.catalog.list_pg_class_rows()
                    .map_err(SqlError::Catalog)?;
                let mut rows = Vec::new();
                for (table_oid, col_name, _path) in indexes {
                    let table_name = class_rows.iter()
                        .find(|r| r.oid == table_oid)
                        .map(|r| r.relname.clone())
                        .unwrap_or_else(|| format!("oid_{}", table_oid));
                    let idx_name = format!("{}_{}_idx", table_name, col_name);
                    let idx_def = format!("CREATE INDEX {} ON {} ({})", idx_name, table_name, col_name);
                    rows.push(Row::new(
                        vec![
                            Value::Text("public".into()),
                            Value::Text(table_name),
                            Value::Text(idx_name),
                            Value::Text(idx_def),
                        ],
                        schema_cols.clone(),
                    ));
                }
                Ok(rows)
            }

            // ── pg_catalog.pg_type ─────────────────────────────────────────
            ("pg_catalog", "pg_type") => {
                let schema_cols: Vec<(String, DataType)> = vec![
                    ("oid".into(), DataType::Int4),
                    ("typname".into(), DataType::Text),
                    ("typnamespace".into(), DataType::Int4),
                    ("typlen".into(), DataType::Int4),
                    ("typtype".into(), DataType::Text),
                    ("typcategory".into(), DataType::Text),
                ];
                let pg_catalog_ns_oid = 101i32; // OID_NS_PG_CATALOG
                let types: &[(&str, i32, i32, &str, &str)] = &[
                    ("bool",      16,    1, "b", "B"),
                    ("bytea",     17,   -1, "b", "U"),
                    ("int8",      20,    8, "b", "N"),
                    ("int4",      23,    4, "b", "N"),
                    ("text",      25,   -1, "b", "S"),
                    ("float8",   701,    8, "b", "N"),
                    ("varchar", 1043,   -1, "b", "S"),
                    ("date",    1082,    4, "b", "D"),
                    ("timestamp", 1114,  8, "b", "D"),
                    ("numeric",  1700,  -1, "b", "N"),
                    ("uuid",     2950,  16, "b", "U"),
                ];
                let mut rows = Vec::new();
                for (name, oid, typlen, typtype, typcategory) in types {
                    rows.push(Row::new(
                        vec![
                            Value::Int4(*oid),
                            Value::Text((*name).into()),
                            Value::Int4(pg_catalog_ns_oid),
                            Value::Int4(*typlen),
                            Value::Text((*typtype).into()),
                            Value::Text((*typcategory).into()),
                        ],
                        schema_cols.clone(),
                    ));
                }
                Ok(rows)
            }

            // ── pg_catalog.pg_roles ────────────────────────────────────────
            ("pg_catalog", "pg_roles") => {
                let schema_cols: Vec<(String, DataType)> = vec![
                    ("rolname".into(), DataType::Text),
                    ("rolsuper".into(), DataType::Boolean),
                    ("rolinherit".into(), DataType::Boolean),
                    ("rolcreaterole".into(), DataType::Boolean),
                    ("rolcreatedb".into(), DataType::Boolean),
                    ("rolcanlogin".into(), DataType::Boolean),
                    ("rolreplication".into(), DataType::Boolean),
                    ("rolbypassrls".into(), DataType::Boolean),
                    ("rolconnlimit".into(), DataType::Int4),
                    ("rolpassword".into(), DataType::Text),
                    ("rolvaliduntil".into(), DataType::Text),
                ];
                let roles = self.ctx.catalog.list_roles()
                    .map_err(SqlError::Catalog)?;
                let mut rows = Vec::new();
                for role in roles {
                    rows.push(Row::new(
                        vec![
                            Value::Text(role.rolname),
                            Value::Bool(role.rolsuper),
                            Value::Bool(role.rolinherit),
                            Value::Bool(role.rolcreaterole),
                            Value::Bool(role.rolcreatedb),
                            Value::Bool(role.rolcanlogin),
                            Value::Bool(false), // rolreplication
                            Value::Bool(role.rolbypassrls),
                            Value::Int4(-1),    // rolconnlimit = unlimited
                            Value::Null,        // rolpassword (masked)
                            Value::Null,        // rolvaliduntil
                        ],
                        schema_cols.clone(),
                    ));
                }
                Ok(rows)
            }

            // ── pg_catalog.pg_views ────────────────────────────────────────
            ("pg_catalog", "pg_views") => {
                Ok(vec![]) // No views yet
            }

            // ── pg_catalog.pg_stat_user_tables ────────────────────────────
            ("pg_catalog", "pg_stat_user_tables") => {
                let schema_cols: Vec<(String, DataType)> = vec![
                    ("relid".into(), DataType::Int4),
                    ("schemaname".into(), DataType::Text),
                    ("relname".into(), DataType::Text),
                    ("seq_scan".into(), DataType::Int8),
                    ("idx_scan".into(), DataType::Int8),
                    ("n_tup_ins".into(), DataType::Int8),
                    ("n_tup_upd".into(), DataType::Int8),
                    ("n_tup_del".into(), DataType::Int8),
                    ("n_live_tup".into(), DataType::Int8),
                    ("n_dead_tup".into(), DataType::Int8),
                ];
                let tables = self.ctx.catalog.list_tables("public")
                    .map_err(SqlError::Catalog)?;
                let mut rows = Vec::new();
                for tbl in tables {
                    let oid = self.ctx.catalog.get_table_oid("public", &tbl).unwrap_or(0);
                    rows.push(Row::new(
                        vec![
                            Value::Int4(oid as i32),
                            Value::Text("public".into()),
                            Value::Text(tbl),
                            Value::Int8(0),
                            Value::Int8(0),
                            Value::Int8(0),
                            Value::Int8(0),
                            Value::Int8(0),
                            Value::Int8(0),
                            Value::Int8(0),
                        ],
                        schema_cols.clone(),
                    ));
                }
                Ok(rows)
            }

            ("pg_catalog", "pg_database") => {
                use crate::db_manager::DatabaseRegistry;
                let registry = DatabaseRegistry::new(&self.ctx.data_dir);
                let dbs = registry.list();
                let schema: Vec<(String, catalog::DataType)> = vec![
                    ("datname".to_string(), catalog::DataType::Text),
                    ("datowner".to_string(), catalog::DataType::Text),
                ];
                let rows: Vec<Row> = dbs.into_iter().map(|db| {
                    Row::new(
                        vec![Value::Text(db.name), Value::Text(db.owner)],
                        schema.clone(),
                    )
                }).collect();
                Ok(rows)
            }

            _ => Err(SqlError::NotImplemented(format!(
                "system catalog table: {}.{}",
                catalog_name, table_name
            ))),
        }
    }

    // ── COPY FROM / COPY TO ───────────────────────────────────────────────────

    fn exec_copy_from(
        &self,
        table_name: &str,
        schema_name: &str,
        file_path: &str,
        delimiter: char,
        has_header: bool,
        _quote: char,
    ) -> Result<u64, SqlError> {
        if file_path == "stdin" || file_path.is_empty() {
            return Err(SqlError::Execution(
                "COPY FROM STDIN not supported in embedded mode".to_string(),
            ));
        }

        let ts = self.ctx.catalog.get_table(schema_name, table_name)
            .map_err(SqlError::Catalog)?;

        let content = std::fs::read_to_string(file_path)
            .map_err(|e| SqlError::Execution(format!("COPY FROM: cannot read file '{}': {}", file_path, e)))?;

        let mut lines = content.lines();
        if has_header {
            lines.next(); // skip header
        }

        let mut count = 0u64;
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split(delimiter).collect();
            let mut values: Vec<crate::plan::Expr> = Vec::new();
            for (i, col) in ts.columns.iter().enumerate() {
                let raw = fields.get(i).copied().unwrap_or("").trim().trim_matches('"');
                let val = if raw.eq_ignore_ascii_case("\\N") || raw.is_empty() {
                    Value::Null
                } else {
                    self.parse_value_for_type(raw, &col.data_type)?
                };
                values.push(crate::plan::Expr::Literal(val));
            }
            let columns: Vec<String> = ts.columns.iter().map(|c| c.name.clone()).collect();
            let insert_plan = LogicalPlan::Insert {
                table_name: table_name.to_string(),
                schema: ts.clone(),
                columns,
                source: crate::plan::InsertSource::Values(vec![values]),
                returning: vec![],
                on_conflict: None,
            };
            self.execute(insert_plan)?;
            count += 1;
        }

        Ok(count)
    }

    fn parse_value_for_type(&self, raw: &str, dt: &catalog::DataType) -> Result<Value, SqlError> {
        match dt {
            catalog::DataType::Int4 => {
                raw.parse::<i32>().map(Value::Int4)
                    .map_err(|_| SqlError::TypeError(format!("cannot parse '{}' as INT4", raw)))
            }
            catalog::DataType::Int8 => {
                raw.parse::<i64>().map(Value::Int8)
                    .map_err(|_| SqlError::TypeError(format!("cannot parse '{}' as INT8", raw)))
            }
            catalog::DataType::Float8 => {
                raw.parse::<f64>().map(Value::Float8)
                    .map_err(|_| SqlError::TypeError(format!("cannot parse '{}' as FLOAT8", raw)))
            }
            catalog::DataType::Boolean => {
                match raw.to_lowercase().as_str() {
                    "true" | "t" | "yes" | "1" => Ok(Value::Bool(true)),
                    _ => Ok(Value::Bool(false)),
                }
            }
            catalog::DataType::Date => {
                crate::value::parse_date_str(raw)
                    .map(Value::Date)
                    .ok_or_else(|| SqlError::TypeError(format!("cannot parse '{}' as DATE", raw)))
            }
            catalog::DataType::Timestamp | catalog::DataType::TimestampTz => {
                crate::value::parse_timestamp_str(raw)
                    .map(Value::Timestamp)
                    .ok_or_else(|| SqlError::TypeError(format!("cannot parse '{}' as TIMESTAMP", raw)))
            }
            catalog::DataType::Numeric => Ok(Value::Numeric(raw.to_string())),
            catalog::DataType::Uuid => Ok(Value::Uuid(raw.to_string())),
            _ => Ok(Value::Text(raw.to_string())),
        }
    }

    fn exec_copy_to(
        &self,
        table_name: Option<&str>,
        query: Option<&LogicalPlan>,
        file_path: &str,
        delimiter: char,
        has_header: bool,
    ) -> Result<u64, SqlError> {
        // Get rows either from table scan or query
        let rows = if let Some(plan) = query {
            self.exec_plan(plan)?
        } else if let Some(tbl) = table_name {
            let ts = self.ctx.catalog.get_table("public", tbl)
                .map_err(SqlError::Catalog)?;
            let scan_plan = LogicalPlan::TableScan {
                table_name: tbl.to_string(),
                alias: None,
                schema: ts,
                filter: None,
            };
            self.exec_plan(&scan_plan)?
        } else {
            return Err(SqlError::Execution("COPY TO requires table or query".to_string()));
        };

        if file_path == "stdout" || file_path.is_empty() {
            return Err(SqlError::Execution(
                "COPY TO STDOUT not supported in embedded mode".to_string(),
            ));
        }

        let mut output = String::new();

        // Write header if requested
        if has_header && !rows.is_empty() {
            let header: Vec<String> = rows[0].schema.iter()
                .map(|(name, _)| {
                    // Strip qualifier prefix (e.g. "t.col" -> "col")
                    if let Some(pos) = name.rfind('.') {
                        name[pos+1..].to_string()
                    } else {
                        name.clone()
                    }
                })
                .collect();
            output.push_str(&header.join(&delimiter.to_string()));
            output.push('\n');
        }

        let count = rows.len() as u64;
        for row in &rows {
            let fields: Vec<String> = row.values.iter().zip(row.schema.iter()).map(|(val, _)| {
                match val {
                    Value::Null => String::new(),
                    Value::Text(s) => {
                        // Quote strings that contain delimiter, newline, or double-quote
                        if s.contains(delimiter) || s.contains('\n') || s.contains('"') {
                            format!("\"{}\"", s.replace('"', "\"\""))
                        } else {
                            s.clone()
                        }
                    }
                    other => other.to_string(),
                }
            }).collect();
            output.push_str(&fields.join(&delimiter.to_string()));
            output.push('\n');
        }

        std::fs::write(file_path, &output)
            .map_err(|e| SqlError::Execution(format!("COPY TO: cannot write file '{}': {}", file_path, e)))?;

        Ok(count)
    }
}

/// Convert a Value to a SQL literal string for substitution in SQL function bodies.
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

/// Re-tag a row's schema to include both "alias.col" and "col" entries.
/// Values are duplicated so that both qualified and unqualified lookups work.
fn qualify_row_schema(row: Row, alias: Option<&str>) -> Row {
    let alias = match alias {
        None => return row,
        Some(a) => a,
    };
    let mut new_schema: Vec<(String, catalog::DataType)> = Vec::new();
    let mut new_values: Vec<Value> = Vec::new();

    for ((col_name, col_type), val) in row.schema.iter().zip(row.values.iter()) {
        // qualified: "alias.col"
        new_schema.push((format!("{}.{}", alias, col_name), col_type.clone()));
        new_values.push(val.clone());
        // unqualified: "col"
        new_schema.push((col_name.clone(), col_type.clone()));
        new_values.push(val.clone());
    }
    Row::new(new_values, new_schema)
}

/// Check if two rows match on the given USING columns.
/// Compares the value of each USING column from the left row against the right row,
/// where left has qualified names (alias.col) and right has qualified names too.
/// Falls back to bare column name comparison using the first match from each side.
fn rows_match_using(left: &Row, right: &Row, using_columns: &[String]) -> bool {
    for col in using_columns {
        // Find the value of `col` in the left row: prefer exact "alias.col" match,
        // falling back to any suffix ".col" match.
        let left_val = find_using_col_value(left, col);
        let right_val = find_using_col_value(right, col);
        match (left_val, right_val) {
            (Some(l), Some(r)) => {
                if !matches!(l.partial_cmp(r), Some(std::cmp::Ordering::Equal)) {
                    return false;
                }
            }
            // If either side doesn't have the column, treat as no match
            _ => return false,
        }
    }
    true
}

/// Find the value of a USING column in a row.
/// Prefers "alias.col" qualified matches; returns the first suffix match if no exact match.
/// Unlike Row::get, this picks the FIRST suffix match (since in USING context, the row
/// represents only one table's columns, not the merged result).
fn find_using_col_value<'a>(row: &'a Row, col_name: &str) -> Option<&'a Value> {
    // Try exact match first
    if let Some(idx) = row.schema.iter().position(|(name, _)| name == col_name) {
        return row.values.get(idx);
    }
    // Find the first suffix match "alias.col"
    let suffix = format!(".{}", col_name);
    let idx = row.schema.iter().position(|(name, _)| name.ends_with(&suffix))?;
    row.values.get(idx)
}

/// For JOIN USING, remove duplicate columns from the right side of the merged row.
/// The left side's version of the USING column is kept; right side's duplicate is removed.
fn deduplicate_using_cols(row: Row, using_columns: &[String]) -> Row {
    if using_columns.is_empty() {
        return row;
    }
    // Find the first occurrence of each USING column (from the left side) — keep those.
    // Remove subsequent occurrences (from the right side).
    let mut seen_using: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut new_schema: Vec<(String, catalog::DataType)> = Vec::new();
    let mut new_values: Vec<Value> = Vec::new();

    for ((col_name, col_type), val) in row.schema.iter().zip(row.values.iter()) {
        // Check if this is a USING column we've already included
        let bare_name = if col_name.contains('.') {
            col_name.split('.').next_back().unwrap_or(col_name.as_str())
        } else {
            col_name.as_str()
        };
        if using_columns.contains(&bare_name.to_string()) {
            if seen_using.contains(bare_name) {
                // Skip duplicate
                continue;
            }
            seen_using.insert(bare_name.to_string());
        }
        new_schema.push((col_name.clone(), col_type.clone()));
        new_values.push(val.clone());
    }
    Row::new(new_values, new_schema)
}

fn checked_numeric_op(
    left: Value,
    right: Value,
    op_i32: impl Fn(i32, i32) -> Option<i32>,
    op_i64: impl Fn(i64, i64) -> Option<i64>,
    op_f64: impl Fn(f64, f64) -> f64,
) -> Result<Value, SqlError> {
    match (&left, &right) {
        (Value::Int4(a), Value::Int4(b)) => {
            op_i32(*a, *b)
                .map(Value::Int4)
                .ok_or_else(|| SqlError::NumericOverflow(format!("{a} op {b}")))
        }
        (Value::Int8(a), Value::Int8(b)) => {
            op_i64(*a, *b)
                .map(Value::Int8)
                .ok_or_else(|| SqlError::NumericOverflow(format!("{a} op {b}")))
        }
        (Value::Float8(a), Value::Float8(b)) => Ok(Value::Float8(op_f64(*a, *b))),
        // Implicit int→float coercion when one side is float
        (Value::Int4(a), Value::Float8(b)) => Ok(Value::Float8(op_f64(*a as f64, *b))),
        (Value::Int8(a), Value::Float8(b)) => Ok(Value::Float8(op_f64(*a as f64, *b))),
        (Value::Float8(a), Value::Int4(b)) => Ok(Value::Float8(op_f64(*a, *b as f64))),
        (Value::Float8(a), Value::Int8(b)) => Ok(Value::Float8(op_f64(*a, *b as f64))),
        // Mixed int widths — promote to i64
        (Value::Int4(a), Value::Int8(b)) => {
            op_i64(*a as i64, *b)
                .map(Value::Int8)
                .ok_or_else(|| SqlError::NumericOverflow(format!("{a} op {b}")))
        }
        (Value::Int8(a), Value::Int4(b)) => {
            op_i64(*a, *b as i64)
                .map(Value::Int8)
                .ok_or_else(|| SqlError::NumericOverflow(format!("{a} op {b}")))
        }
        // Any expression involving Numeric is promoted to f64, result stays Numeric.
        // This handles expressions like `balance - 100` where balance is NUMERIC(12,2).
        _ => {
            let lf = numeric_to_f64(&left)
                .ok_or_else(|| SqlError::TypeError(format!(
                    "arithmetic type mismatch: {left:?} vs {right:?}"
                )))?;
            let rf = numeric_to_f64(&right)
                .ok_or_else(|| SqlError::TypeError(format!(
                    "arithmetic type mismatch: {left:?} vs {right:?}"
                )))?;
            let result = op_f64(lf, rf);
            // If either operand was Numeric, keep the result as Numeric.
            // Otherwise (both are exotic types that happen to reach here), use Float8.
            if matches!(&left, Value::Numeric(_)) || matches!(&right, Value::Numeric(_)) {
                Ok(Value::Numeric(format_numeric(result)))
            } else {
                Ok(Value::Float8(result))
            }
        }
    }
}

/// Try to convert any numeric Value variant to f64.
fn numeric_to_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Int4(i) => Some(*i as f64),
        Value::Int8(i) => Some(*i as f64),
        Value::Float8(f) => Some(*f),
        Value::Numeric(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

/// Format a f64 as a decimal string suitable for storage in a Numeric column.
/// Produces the shortest representation that round-trips (no trailing ".0" for integers,
/// but preserves fractional digits when present).
fn format_numeric(f: f64) -> String {
    // Use Rust's default Display, which gives shortest round-trip representation.
    // E.g. 900.0 → "900", 13.49 → "13.49", 17.590000000000003 → handled by rounding below.
    // Round to 10 significant decimal places to suppress f64 noise.
    let s = format!("{:.10}", f);
    // Strip trailing zeros after decimal point.
    if s.contains('.') {
        let s = s.trim_end_matches('0');
        let s = s.trim_end_matches('.');
        s.to_string()
    } else {
        s
    }
}

/// Returns true if the plan contains an Aggregate node anywhere in its tree.
fn plan_contains_aggregate(plan: &LogicalPlan) -> bool {
    match plan {
        LogicalPlan::Aggregate { .. } => true,
        LogicalPlan::Project { input, .. }
        | LogicalPlan::Filter { input, .. }
        | LogicalPlan::Sort { input, .. }
        | LogicalPlan::Limit { input, .. }
        | LogicalPlan::Window { input, .. } => plan_contains_aggregate(input),
        LogicalPlan::Join { left, right, .. } => {
            plan_contains_aggregate(left) || plan_contains_aggregate(right)
        }
        LogicalPlan::SetOp { left, right, .. } => {
            plan_contains_aggregate(left) || plan_contains_aggregate(right)
        }
        LogicalPlan::Cte { inner, .. } => plan_contains_aggregate(inner),
        _ => false,
    }
}

fn deduplicate_rows(rows: Vec<Row>) -> Vec<Row> {
    let mut seen: Vec<Vec<Value>> = Vec::new();
    let mut result = Vec::new();
    for row in rows {
        if !seen.contains(&row.values) {
            seen.push(row.values.clone());
            result.push(row);
        }
    }
    result
}

fn merge_rows(left: &Row, right: &Row) -> Row {
    let mut values = left.values.clone();
    values.extend_from_slice(&right.values);
    let mut schema = left.schema.clone();
    schema.extend_from_slice(&right.schema);
    Row::new(values, schema)
}

fn left_pad_row(lr: &Row, right_rows: &[Row]) -> Row {
    let right_schema = if let Some(r) = right_rows.first() {
        r.schema.clone()
    } else {
        vec![]
    };
    let mut values = lr.values.clone();
    let nulls: Vec<Value> = right_schema.iter().map(|_| Value::Null).collect();
    values.extend_from_slice(&nulls);
    let mut schema = lr.schema.clone();
    schema.extend_from_slice(&right_schema);
    Row::new(values, schema)
}

fn right_pad_row(left_rows: &[Row], rr: &Row) -> Row {
    let left_schema = if let Some(l) = left_rows.first() {
        l.schema.clone()
    } else {
        vec![]
    };
    let nulls: Vec<Value> = left_schema.iter().map(|_| Value::Null).collect();
    let mut values = nulls;
    values.extend_from_slice(&rr.values);
    let mut schema = left_schema;
    schema.extend_from_slice(&rr.schema);
    Row::new(values, schema)
}

fn value_to_string(v: &Value) -> String {
    v.to_string()
}

fn like_match(text: &str, pattern: &str) -> bool {
    // Simple LIKE implementation: % matches any sequence, _ matches one char
    like_match_impl(text.as_bytes(), pattern.as_bytes())
}

fn like_match_impl(text: &[u8], pattern: &[u8]) -> bool {
    match pattern.first() {
        None => text.is_empty(),
        Some(&b'%') => {
            for i in 0..=text.len() {
                if like_match_impl(&text[i..], &pattern[1..]) {
                    return true;
                }
            }
            false
        }
        Some(&b'_') => {
            if text.is_empty() {
                false
            } else {
                like_match_impl(&text[1..], &pattern[1..])
            }
        }
        Some(&c) => !text.is_empty() && text[0] == c && like_match_impl(&text[1..], &pattern[1..]),
    }
}

/// Infer the output DataType of an expression given the input row schema.
fn infer_expr_type(expr: &Expr, input_schema: &[(String, catalog::DataType)]) -> catalog::DataType {
    match expr {
        Expr::Column { name, table } => {
            let search_name = if let Some(t) = table {
                format!("{}.{}", t, name)
            } else {
                name.clone()
            };
            input_schema
                .iter()
                .find(|(col_name, _)| {
                    col_name == &search_name
                        || col_name.ends_with(&format!(".{}", name))
                        || col_name == name
                })
                .map(|(_, t)| t.clone())
                .unwrap_or(catalog::DataType::Text)
        }
        Expr::Literal(v) => match v {
            Value::Null => catalog::DataType::Text,
            Value::Bool(_) => catalog::DataType::Boolean,
            Value::Int4(_) => catalog::DataType::Int4,
            Value::Int8(_) => catalog::DataType::Int8,
            Value::Float8(_) => catalog::DataType::Float8,
            Value::Text(_) => catalog::DataType::Text,
            Value::Bytes(_) => catalog::DataType::Bytea,
            Value::Date(_) => catalog::DataType::Date,
            Value::Timestamp(_) => catalog::DataType::Timestamp,
            Value::Numeric(_) => catalog::DataType::Numeric,
            Value::Uuid(_) => catalog::DataType::Uuid,
        },
        Expr::BinaryOp { op, left, .. } => match op {
            BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Lt
            | BinaryOp::Le
            | BinaryOp::Gt
            | BinaryOp::Ge
            | BinaryOp::And
            | BinaryOp::Or => catalog::DataType::Boolean,
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                infer_expr_type(left, input_schema)
            }
            BinaryOp::Concat => catalog::DataType::Text,
        },
        Expr::UnaryOp { expr, .. } => infer_expr_type(expr, input_schema),
        Expr::Cast { data_type, .. } => data_type.clone(),
        Expr::IsNull(_)
        | Expr::IsNotNull(_)
        | Expr::IsDistinctFrom { .. }
        | Expr::IsNotDistinctFrom { .. }
        | Expr::InSubquery { .. }
        | Expr::Exists { .. } => catalog::DataType::Boolean,
        Expr::ScalarSubquery(_) => catalog::DataType::Text,
        Expr::FunctionCall { name, args } => match name.to_lowercase().as_str() {
            "count" => catalog::DataType::Int8,
            "sum" | "avg" => args
                .first()
                .map(|a| infer_expr_type(a, input_schema))
                .unwrap_or(catalog::DataType::Float8),
            "min" | "max" => args
                .first()
                .map(|a| infer_expr_type(a, input_schema))
                .unwrap_or(catalog::DataType::Text),
            "lower" | "upper" | "trim" | "like" => catalog::DataType::Text,
            _ => catalog::DataType::Text,
        },
        Expr::Case { else_clause, when_clauses, .. } => {
            // Infer type from else clause or first result
            if let Some(e) = else_clause {
                infer_expr_type(e, input_schema)
            } else if let Some((_, result)) = when_clauses.first() {
                infer_expr_type(result, input_schema)
            } else {
                catalog::DataType::Text
            }
        }
        Expr::Coalesce(args) => args
            .first()
            .map(|a| infer_expr_type(a, input_schema))
            .unwrap_or(catalog::DataType::Text),
        Expr::NullIf(expr1, _) => infer_expr_type(expr1, input_schema),
    }
}

/// Wrapper for Value that implements Eq and Hash for use as HashMap key.
#[derive(Clone)]
struct OrderableValue(Value);

impl PartialEq for OrderableValue {
    fn eq(&self, other: &Self) -> bool {
        self.0.partial_cmp(&other.0) == Some(std::cmp::Ordering::Equal)
    }
}

impl Eq for OrderableValue {}

impl std::hash::Hash for OrderableValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match &self.0 {
            Value::Null => 0u8.hash(state),
            Value::Bool(b) => {
                1u8.hash(state);
                b.hash(state);
            }
            Value::Int4(i) => {
                2u8.hash(state);
                i.hash(state);
            }
            Value::Int8(i) => {
                3u8.hash(state);
                i.hash(state);
            }
            Value::Float8(f) => {
                4u8.hash(state);
                f.to_bits().hash(state);
            }
            Value::Text(s) => {
                5u8.hash(state);
                s.hash(state);
            }
            Value::Bytes(b) => {
                6u8.hash(state);
                b.hash(state);
            }
            Value::Date(d) => {
                7u8.hash(state);
                d.hash(state);
            }
            Value::Timestamp(t) => {
                8u8.hash(state);
                t.hash(state);
            }
            Value::Numeric(s) | Value::Uuid(s) => {
                9u8.hash(state);
                s.hash(state);
            }
        }
    }
}

/// Get current timestamp as microseconds since Unix epoch.
fn current_timestamp_micros() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

/// Get current date as days since Unix epoch.
fn current_date_days() -> i32 {
    (current_timestamp_micros() / 86_400_000_000) as i32
}

/// Generate a random UUID string.
fn generate_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Simple pseudo-random f64 in [0, 1) using system time nanos.
fn random_f64() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(12345);
    // LCG-based scramble
    let x = nanos.wrapping_mul(1664525).wrapping_add(1013904223);
    (x as f64) / (u32::MAX as f64)
}

fn datatype_name(dt: &catalog::DataType) -> &'static str {
    match dt {
        catalog::DataType::Boolean => "boolean",
        catalog::DataType::Int4 => "integer",
        catalog::DataType::Int8 => "bigint",
        catalog::DataType::Float8 => "double precision",
        catalog::DataType::Text => "text",
        catalog::DataType::VarChar(_) => "character varying",
        catalog::DataType::Bytea => "bytea",
        catalog::DataType::Date => "date",
        catalog::DataType::Timestamp => "timestamp without time zone",
        catalog::DataType::TimestampTz => "timestamp with time zone",
        catalog::DataType::Numeric => "numeric",
        catalog::DataType::Uuid => "uuid",
    }
}

/// Derive a display name for a single expression (used by plan_col_names).
fn expr_col_name(expr: &Expr) -> String {
    match expr {
        Expr::Column { name, .. } => name.clone(),
        Expr::Literal(_) => "?column?".to_string(),
        Expr::FunctionCall { name, .. } => name.to_lowercase(),
        _ => "?column?".to_string(),
    }
}

/// Extract expected output column names and types from a logical plan.
/// Used to populate `ExecutionResult::col_names` and `col_types` for 0-row query results so
/// that the wire protocol can emit a proper RowDescription message.
pub fn plan_col_names(plan: &LogicalPlan, catalog: &Arc<CatalogManager>) -> (Vec<String>, Vec<catalog::DataType>) {
    match plan {
        LogicalPlan::Project { columns, .. } => {
            let names: Vec<String> = columns.iter().map(|(name, _expr)| name.clone()).collect();
            let types: Vec<catalog::DataType> = names.iter().map(|_| catalog::DataType::Text).collect();
            (names, types)
        }
        LogicalPlan::TableScan { table_name, .. } => {
            match catalog.get_table("public", table_name) {
                Ok(schema) => {
                    let names: Vec<String> = schema.columns.iter().map(|c| c.name.clone()).collect();
                    let types: Vec<catalog::DataType> = schema.columns.iter().map(|c| c.data_type.clone()).collect();
                    (names, types)
                }
                Err(_) => (vec![], vec![]),
            }
        }
        LogicalPlan::Filter { input, .. } => plan_col_names(input, catalog),
        LogicalPlan::Aggregate { group_by, aggregates, .. } => {
            let mut names: Vec<String> = group_by.iter().map(expr_col_name).collect();
            for (alias, _func, _expr) in aggregates {
                names.push(alias.clone());
            }
            let types: Vec<catalog::DataType> = names.iter().map(|_| catalog::DataType::Text).collect();
            (names, types)
        }
        LogicalPlan::Sort { input, .. } => plan_col_names(input, catalog),
        LogicalPlan::Limit { input, .. } => plan_col_names(input, catalog),
        LogicalPlan::Join { .. } => (vec![], vec![]),
        _ => (vec![], vec![]),
    }
}

/// Format a LogicalPlan as a human-readable tree, one line per node.
fn format_plan(plan: &LogicalPlan, indent: usize) -> Vec<String> {
    let prefix = "  ".repeat(indent);
    match plan {
        LogicalPlan::TableScan { table_name, filter, .. } => {
            let mut lines = vec![format!("{}Seq Scan on {}", prefix, table_name)];
            if let Some(f) = filter {
                lines.push(format!("{}  Filter: {:?}", prefix, f));
            }
            lines
        }
        LogicalPlan::IndexScan { table_name, index_column, eq_value, range_start, range_end, .. } => {
            let mut lines = vec![format!("{}Index Scan on {} using index on {}", prefix, table_name, index_column)];
            if let Some(v) = eq_value {
                lines.push(format!("{}  Index Cond: {} = {}", prefix, index_column, v));
            } else {
                if let Some(s) = range_start {
                    lines.push(format!("{}  Index Cond: {} >= {}", prefix, index_column, s));
                }
                if let Some(e) = range_end {
                    lines.push(format!("{}  Index Cond: {} < {}", prefix, index_column, e));
                }
            }
            lines
        }
        LogicalPlan::Filter { input, predicate } => {
            let mut lines = vec![format!("{}Filter: {:?}", prefix, predicate)];
            lines.extend(format_plan(input, indent + 1));
            lines
        }
        LogicalPlan::Project { input, columns, distinct } => {
            let col_names: Vec<String> = columns.iter().map(|(name, _)| name.clone()).collect();
            let distinct_str = if *distinct { " (distinct)" } else { "" };
            let mut lines = vec![format!("{}Project{}: [{}]", prefix, distinct_str, col_names.join(", "))];
            lines.extend(format_plan(input, indent + 1));
            lines
        }
        LogicalPlan::Aggregate { input, group_by, aggregates, .. } => {
            let agg_names: Vec<String> = aggregates.iter().map(|(name, _, _)| name.clone()).collect();
            let mut lines = vec![format!("{}Aggregate: [{}]", prefix, agg_names.join(", "))];
            if !group_by.is_empty() {
                lines.push(format!("{}  Group By: {:?}", prefix, group_by));
            }
            lines.extend(format_plan(input, indent + 1));
            lines
        }
        LogicalPlan::Sort { input, keys } => {
            let key_strs: Vec<String> = keys.iter().map(|k| format!("{:?}", k)).collect();
            let mut lines = vec![format!("{}Sort: [{}]", prefix, key_strs.join(", "))];
            lines.extend(format_plan(input, indent + 1));
            lines
        }
        LogicalPlan::Limit { input, limit, offset } => {
            let mut lines = vec![format!("{}Limit: {} offset {}", prefix, limit, offset)];
            lines.extend(format_plan(input, indent + 1));
            lines
        }
        LogicalPlan::Join { left, right, join_type, condition, .. } => {
            let mut lines = vec![format!("{}Hash Join ({:?})", prefix, join_type)];
            lines.push(format!("{}  Cond: {:?}", prefix, condition));
            lines.push(format!("{}  -> Left:", prefix));
            lines.extend(format_plan(left, indent + 2));
            lines.push(format!("{}  -> Right:", prefix));
            lines.extend(format_plan(right, indent + 2));
            lines
        }
        LogicalPlan::Cte { ctes, inner } => {
            let mut lines = vec![format!("{}CTE", prefix)];
            for (name, cte_plan) in ctes {
                lines.push(format!("{}  {} AS:", prefix, name));
                lines.extend(format_plan(cte_plan, indent + 2));
            }
            lines.push(format!("{}Inner:", prefix));
            lines.extend(format_plan(inner, indent + 1));
            lines
        }
        other => {
            vec![format!("{}{:?}", prefix, other).chars().take(120).collect()]
        }
    }
}

/// Extract (left_key_expr, right_key_expr) from a hash join equality condition.
/// For `col_a = col_b`, returns the left and right column expressions.
fn extract_hash_join_keys(condition: &Expr) -> (Option<Expr>, Option<Expr>) {
    match condition {
        Expr::BinaryOp { left, op: BinaryOp::Eq, right } => {
            (Some(*left.clone()), Some(*right.clone()))
        }
        Expr::BinaryOp { left, op: BinaryOp::And, .. } => {
            // For AND conditions, just use the leftmost equality for hashing
            extract_hash_join_keys(left)
        }
        _ => (None, None),
    }
}
