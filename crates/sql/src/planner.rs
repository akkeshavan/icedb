use std::cell::RefCell;
use std::sync::Arc;

use catalog::manager::CatalogManager;
use catalog::schema::TableSchema;
use sqlparser::ast::{
    self, BinaryOperator, ColumnDef as AstColumnDef, CopyOption, CopySource, CopyTarget,
    CreateFunctionBody, DataType as AstDataType, Expr as AstExpr,
    FunctionArguments, GroupByExpr, Ident, JoinOperator, ObjectName, Query, Select, SelectItem,
    SetExpr, SetOperator, Statement, TableFactor, TableWithJoins, UnaryOperator, Value as AstValue,
};

use crate::error::SqlError;
use crate::plan::{
    AggFunc, AlterTableOp, BinaryOp, Expr, InsertSource, JoinAlgorithm, JoinType, LogicalPlan,
    OnConflict, OnConflictAction, SetOperation, SortKey, UnaryOp, WindowExpr, WindowFunction,
};
use crate::value::Value;

pub struct Planner {
    catalog: Arc<CatalogManager>,
    /// CTE names available in the current query scope (for planning inner queries)
    cte_names: RefCell<Vec<String>>,
}

impl Planner {
    pub fn new(catalog: Arc<CatalogManager>) -> Self {
        Self {
            catalog,
            cte_names: RefCell::new(Vec::new()),
        }
    }

    pub fn plan_statement(&self, stmt: &Statement) -> Result<LogicalPlan, SqlError> {
        match stmt {
            Statement::Query(query) => self.plan_query_with_cte(query),
            Statement::Insert(insert) => {
                // sqlparser 0.53 uses table_name (ObjectName) not table (TableWithJoins)
                let table_with_joins = TableWithJoins {
                    relation: TableFactor::Table {
                        name: insert.table_name.clone(),
                        alias: insert.table_alias.as_ref().map(|a| ast::TableAlias {
                            name: a.clone(),
                            columns: vec![],
                        }),
                        args: None,
                        with_hints: vec![],
                        version: None,
                        partitions: vec![],
                        with_ordinality: false,
                        json_path: None,
                    },
                    joins: vec![],
                };
                self.plan_insert(
                    &table_with_joins,
                    &insert.columns,
                    insert.source.as_deref(),
                    insert.returning.as_ref(),
                    insert.on.as_ref(),
                )
            }
            Statement::Update {
                table,
                assignments,
                selection,
                returning,
                from,
                ..
            } => self.plan_update(table, assignments, selection, returning.as_ref(), from.as_ref()),
            Statement::Delete(delete) => {
                // sqlparser 0.53 uses FromTable enum for delete.from
                let tables = match &delete.from {
                    ast::FromTable::WithFromKeyword(v) => v,
                    ast::FromTable::WithoutKeyword(v) => v,
                };
                let table_name = if let Some(TableWithJoins { relation, .. }) = tables.first() {
                    match relation {
                        TableFactor::Table { name, .. } => name.clone(),
                        _ => {
                            return Err(SqlError::NotImplemented(
                                "complex DELETE target".to_string(),
                            ))
                        }
                    }
                } else {
                    return Err(SqlError::Execution("DELETE missing table".to_string()));
                };
                self.plan_delete(&table_name, &delete.selection, delete.returning.as_ref(), delete.using.as_deref())
            }
            Statement::CreateTable(create) => {
                self.plan_create_table(&create.name, &create.columns, create.if_not_exists, &create.constraints)
            }
            Statement::AlterTable { name, operations, .. } => {
                self.plan_alter_table(name, operations)
            }
            Statement::Drop {
                object_type,
                names,
                if_exists,
                ..
            } => match object_type {
                ast::ObjectType::Table => self.plan_drop_table(names, *if_exists),
                _ => Err(SqlError::NotImplemented(format!("DROP {:?}", object_type))),
            },
            Statement::CreateRole {
                names,
                superuser,
                login,
                password,
                ..
            } => {
                let rolname = names
                    .first()
                    .map(|n| n.to_string())
                    .ok_or_else(|| SqlError::Execution("CREATE ROLE missing name".to_string()))?;
                let rolsuper = superuser.unwrap_or(false);
                let rolcanlogin = login.unwrap_or(false);
                let pw = match password {
                    Some(ast::Password::Password(AstExpr::Value(
                        AstValue::SingleQuotedString(s),
                    ))) => Some(s.clone()),
                    Some(ast::Password::NullPassword) => None,
                    _ => None,
                };

                Ok(LogicalPlan::CreateRole {
                    rolname,
                    rolsuper,
                    rolcanlogin,
                    password: pw,
                })
            }
            Statement::CreateIndex(ci) => {
                // ci.table_name is ObjectName; ci.columns is Vec<OrderByExpr>
                let table_name =
                    ci.table_name
                        .0
                        .last()
                        .map(|i| i.value.clone())
                        .ok_or_else(|| {
                            SqlError::Parse("CREATE INDEX missing table name".to_string())
                        })?;
                // For single-column indexes, use the column name directly.
                // For multi-column indexes, join all column names; this prevents the planner
                // from incorrectly using a multi-column index for single-column equality
                // queries (which would miss duplicate leading-column values).
                if ci.columns.is_empty() {
                    return Err(SqlError::Parse("CREATE INDEX requires at least one column".to_string()));
                }
                let column_name = if ci.columns.len() == 1 {
                    ci.columns[0].expr.to_string()
                } else {
                    ci.columns.iter().map(|c| c.expr.to_string()).collect::<Vec<_>>().join(", ")
                };
                // Extract optional index name (e.g. CREATE INDEX idx_name ON table(col))
                let index_name = ci.name.as_ref().map(|n| n.to_string());
                Ok(LogicalPlan::CreateIndex {
                    schema_name: "public".to_string(),
                    table_name,
                    column_name,
                    index_name,
                })
            }
            Statement::SetVariable { .. }
            | Statement::SetTimeZone { .. } => {
                Ok(LogicalPlan::NoOp { command: "SET".to_string() })
            }
            Statement::ShowVariable { .. }
            | Statement::ShowColumns { .. }
            | Statement::ShowCreate { .. }
            | Statement::ShowTables { .. } => {
                Ok(LogicalPlan::NoOp { command: "SHOW".to_string() })
            }
            Statement::CreateSchema { schema_name, if_not_exists, .. } => {
                let name = match schema_name {
                    ast::SchemaName::Simple(obj_name) => obj_name.0.last()
                        .map(|i| i.value.clone())
                        .ok_or_else(|| SqlError::Parse("CREATE SCHEMA missing name".to_string()))?,
                    ast::SchemaName::NamedAuthorization(obj_name, _) => obj_name.0.last()
                        .map(|i| i.value.clone())
                        .ok_or_else(|| SqlError::Parse("CREATE SCHEMA missing name".to_string()))?,
                    ast::SchemaName::UnnamedAuthorization(ident) => ident.value.clone(),
                };
                Ok(LogicalPlan::CreateSchema { name, if_not_exists: *if_not_exists })
            }
            // Transaction control — accepted as SQL for compatibility; in auto-commit mode
            // each execute() call is already wrapped in its own transaction.
            Statement::StartTransaction { .. } => Ok(LogicalPlan::TransactionControl {
                kind: crate::plan::TransactionControlKind::Begin,
            }),
            Statement::Commit { .. } => Ok(LogicalPlan::TransactionControl {
                kind: crate::plan::TransactionControlKind::Commit,
            }),
            Statement::Rollback { .. } => Ok(LogicalPlan::TransactionControl {
                kind: crate::plan::TransactionControlKind::Rollback,
            }),
            Statement::Grant {
                privileges,
                objects,
                grantees,
                ..
            } => {
                let privs = Self::extract_privilege_names(privileges);
                let cols = Self::extract_privilege_columns(privileges);
                let (schema_name, table_name) = Self::extract_grant_object(objects)?;
                let grantee = grantees
                    .first()
                    .map(|g| g.value.clone())
                    .ok_or_else(|| SqlError::Execution("GRANT missing grantee".to_string()))?;
                Ok(LogicalPlan::Grant {
                    table_name,
                    schema: schema_name,
                    grantee,
                    privileges: privs,
                    columns: cols,
                })
            }
            Statement::Revoke {
                privileges,
                objects,
                grantees,
                ..
            } => {
                let privs = Self::extract_privilege_names(privileges);
                let cols = Self::extract_privilege_columns(privileges);
                let (schema_name, table_name) = Self::extract_grant_object(objects)?;
                let grantee = grantees
                    .first()
                    .map(|g| g.value.clone())
                    .ok_or_else(|| SqlError::Execution("REVOKE missing grantee".to_string()))?;
                Ok(LogicalPlan::Revoke {
                    table_name,
                    schema: schema_name,
                    grantee,
                    privileges: privs,
                    columns: cols,
                })
            }
            Statement::Copy { source, to, target, options, .. } => {
                self.plan_copy(source, *to, target, options)
            }
            Statement::Prepare { name, statement, .. } => {
                let sql = statement.to_string();
                Ok(LogicalPlan::Prepare {
                    name: name.value.clone(),
                    sql,
                })
            }
            Statement::Execute { name, parameters, .. } => {
                let params = parameters.iter().map(|p| {
                    // Evaluate parameters as literal values
                    match p {
                        AstExpr::Value(v) => self.ast_value_to_value(v),
                        other => {
                            // Try converting expression to a value
                            let expr = self.convert_expr_with_schema(other, None)?;
                            match expr {
                                Expr::Literal(val) => Ok(val),
                                _ => Err(SqlError::NotImplemented(
                                    "non-literal EXECUTE parameter".to_string(),
                                )),
                            }
                        }
                    }
                }).collect::<Result<Vec<_>, _>>()?;
                let name_str = name.0.iter().map(|i| i.value.clone()).collect::<Vec<_>>().join(".");
                Ok(LogicalPlan::ExecutePrepared {
                    name: name_str,
                    params,
                })
            }
            Statement::Deallocate { name, .. } => {
                Ok(LogicalPlan::Deallocate {
                    name: name.value.clone(),
                })
            }
            Statement::CreateFunction(cf) => {
                self.plan_create_function(cf)
            }
            Statement::Explain { statement, analyze, .. } => {
                let inner = self.plan_statement(statement)?;
                Ok(LogicalPlan::Explain {
                    analyze: *analyze,
                    plan: Box::new(inner),
                })
            }
            _ => Err(SqlError::NotImplemented(format!(
                "statement type: {}",
                stmt
            ))),
        }
    }

    fn plan_create_function(&self, cf: &ast::CreateFunction) -> Result<LogicalPlan, SqlError> {
        // Extract function name
        let name = cf.name.0.last()
            .map(|i| i.value.clone())
            .ok_or_else(|| SqlError::Parse("CREATE FUNCTION missing name".to_string()))?;

        // Extract parameter list: Vec<(param_name, DataType)>
        let params: Vec<(String, catalog::DataType)> = cf.args.as_ref()
            .map(|args| {
                args.iter().enumerate().map(|(i, arg)| {
                    let param_name = arg.name
                        .as_ref()
                        .map(|n| n.value.clone())
                        .unwrap_or_else(|| format!("${}", i + 1));
                    let dt = self.convert_data_type(&arg.data_type)?;
                    Ok((param_name, dt))
                }).collect::<Result<Vec<_>, SqlError>>()
            })
            .transpose()?
            .unwrap_or_default();

        // Extract return type
        let return_type = cf.return_type.as_ref()
            .map(|dt| self.convert_data_type(dt))
            .transpose()?
            .unwrap_or(catalog::DataType::Text);

        // Extract language
        let language = cf.language.as_ref()
            .map(|l| l.value.to_lowercase())
            .unwrap_or_else(|| "sql".to_string());

        // Extract function body SQL
        let body_sql = match &cf.function_body {
            Some(CreateFunctionBody::AsBeforeOptions(expr)) => {
                // For $$ ... $$ syntax, this becomes a single-quoted string value
                // or the raw expression string
                match expr {
                    AstExpr::Value(AstValue::SingleQuotedString(s))
                    | AstExpr::Value(AstValue::DollarQuotedString(sqlparser::ast::DollarQuotedString { value: s, .. })) => {
                        s.clone()
                    }
                    other => other.to_string(),
                }
            }
            Some(CreateFunctionBody::AsAfterOptions(expr)) => {
                match expr {
                    AstExpr::Value(AstValue::SingleQuotedString(s))
                    | AstExpr::Value(AstValue::DollarQuotedString(sqlparser::ast::DollarQuotedString { value: s, .. })) => {
                        s.clone()
                    }
                    other => other.to_string(),
                }
            }
            Some(CreateFunctionBody::Return(expr)) => expr.to_string(),
            None => return Err(SqlError::Parse("CREATE FUNCTION missing body".to_string())),
        };

        Ok(LogicalPlan::CreateFunction {
            schema: "public".to_string(),
            name,
            params,
            return_type,
            body_sql,
            language,
        })
    }

    fn extract_privilege_names(privileges: &ast::Privileges) -> Vec<String> {
        match privileges {
            ast::Privileges::All { .. } => vec!["ALL".to_string()],
            ast::Privileges::Actions(actions) => actions
                .iter()
                .map(|a| match a {
                    ast::Action::Select { .. } => "SELECT".to_string(),
                    ast::Action::Insert { .. } => "INSERT".to_string(),
                    ast::Action::Update { .. } => "UPDATE".to_string(),
                    ast::Action::Delete => "DELETE".to_string(),
                    other => format!("{other}"),
                })
                .collect(),
        }
    }

    /// Extract column names from column-level privilege actions.
    ///
    /// For `GRANT SELECT (col1, col2) ON t TO r`, the `Action::Select` variant
    /// carries `columns: Some(vec![col1, col2])`. This helper collects all such
    /// column names across all actions in the privilege list, deduplicating them.
    fn extract_privilege_columns(privileges: &ast::Privileges) -> Vec<String> {
        let mut cols: Vec<String> = Vec::new();
        if let ast::Privileges::Actions(actions) = privileges {
            for action in actions {
                let action_cols = match action {
                    ast::Action::Select { columns } => columns.as_deref(),
                    ast::Action::Insert { columns } => columns.as_deref(),
                    ast::Action::Update { columns } => columns.as_deref(),
                    ast::Action::References { columns } => columns.as_deref(),
                    _ => None,
                };
                if let Some(idents) = action_cols {
                    for ident in idents {
                        let name = ident.value.clone();
                        if !cols.contains(&name) {
                            cols.push(name);
                        }
                    }
                }
            }
        }
        cols
    }

    fn extract_grant_object(objects: &ast::GrantObjects) -> Result<(String, String), SqlError> {
        match objects {
            ast::GrantObjects::Tables(tables) => {
                let name = tables
                    .first()
                    .ok_or_else(|| SqlError::Execution("GRANT missing table name".to_string()))?;
                let parts: Vec<String> = name.0.iter().map(|i| i.value.clone()).collect();
                if parts.len() >= 2 {
                    Ok((parts[parts.len() - 2].clone(), parts[parts.len() - 1].clone()))
                } else {
                    Ok(("public".to_string(), parts[0].clone()))
                }
            }
            _ => Err(SqlError::NotImplemented(
                "GRANT/REVOKE on non-table objects not yet supported".to_string(),
            )),
        }
    }

    fn plan_query_with_cte(&self, query: &Query) -> Result<LogicalPlan, SqlError> {
        if let Some(with) = &query.with {
            let recursive = with.recursive;
            // Plan each CTE body, registering each name before planning the next so that
            // later CTEs can reference earlier ones (e.g. author_revenue referencing book_revenue).
            let mut ctes: Vec<(String, Box<LogicalPlan>)> = Vec::new();
            for cte in &with.cte_tables {
                let name = cte.alias.name.value.clone();

                if recursive {
                    // For WITH RECURSIVE, register the name BEFORE planning so the recursive
                    // body can reference it.
                    self.cte_names.borrow_mut().push(name.clone());
                    // Collect column aliases declared in the CTE signature (e.g. `series(n)`).
                    let column_aliases: Vec<String> = cte
                        .alias
                        .columns
                        .iter()
                        .map(|col_def| col_def.name.value.clone())
                        .collect();
                    let cte_plan = self.plan_recursive_cte(&name, column_aliases, &cte.query)?;
                    ctes.push((name.clone(), Box::new(cte_plan)));
                } else {
                    let cte_plan = self.plan_query_with_cte(&cte.query)?;
                    ctes.push((name.clone(), Box::new(cte_plan)));
                    // Register this CTE's name immediately so subsequent CTEs can reference it.
                    self.cte_names.borrow_mut().push(name);
                }
            }
            // Plan the main body — CTEs are materialized at execution time
            let inner = self.plan_query(query)?;
            // Unregister all CTE names after planning
            {
                let mut names = self.cte_names.borrow_mut();
                for (name, _) in &ctes {
                    names.retain(|n| n != name);
                }
            }
            Ok(LogicalPlan::Cte {
                ctes,
                inner: Box::new(inner),
            })
        } else {
            self.plan_query(query)
        }
    }

    /// Plan a recursive CTE. The query body is expected to be a UNION ALL with a base case
    /// and a recursive case that references the CTE name itself.
    fn plan_recursive_cte(
        &self,
        name: &str,
        column_aliases: Vec<String>,
        query: &Query,
    ) -> Result<LogicalPlan, SqlError> {
        // The body should be a UNION ALL: base_case UNION ALL recursive_case
        match query.body.as_ref() {
            SetExpr::SetOperation {
                op: SetOperator::Union,
                left,
                right,
                ..
            } => {
                let base_query = self.plan_set_expr(left)?;
                let recursive_query = self.plan_set_expr(right)?;
                Ok(LogicalPlan::RecursiveCte {
                    name: name.to_string(),
                    column_aliases,
                    base_query: Box::new(base_query),
                    recursive_query: Box::new(recursive_query),
                    // sqlparser-rs 0.53 does not expose SEARCH/CYCLE clauses as structured
                    // AST fields on the Cte struct; these are populated as None here and
                    // can be set by a post-parse rewrite pass when the feature is needed.
                    search_by_col: None,
                    search_set_col: None,
                    cycle_col: None,
                    cycle_set_col: None,
                    cycle_path_col: None,
                })
            }
            // If not UNION ALL, treat it as a plain CTE
            _ => self.plan_query_with_cte(query),
        }
    }

    fn plan_query(&self, query: &Query) -> Result<LogicalPlan, SqlError> {
        let has_order_by = query
            .order_by
            .as_ref()
            .map(|ob| !ob.exprs.is_empty())
            .unwrap_or(false);

        // For SELECT statements with ORDER BY, build the sort before the final projection so
        // that ORDER BY expressions can reference pre-projection column names (e.g. `a.name`)
        // even when the SELECT list aliases them (e.g. `a.name AS author`).
        let plan = if has_order_by {
            if let SetExpr::Select(select) = query.body.as_ref() {
                // Build the plan body without the final Project wrapper
                let (inner_plan, projection) = self.plan_select_body(select)?;

                // Convert ORDER BY keys against the pre-projection schema.
                // For aggregate queries (projection == None), ORDER BY expressions that are
                // aggregate function calls must be rewritten to column references using the
                // aggregate output name, since the aggregation has already been applied.
                let order_by = query.order_by.as_ref().unwrap();
                let agg_output_schema = self.extract_schema_from_plan(&inner_plan);
                // Build a lookup: aggregate AST expression text -> output alias
                // so that ORDER BY count(*) can find the alias "cnt" when SELECT has count(*) AS cnt
                let agg_alias_lookup: Vec<(String, String)> = if projection.is_none() {
                    select.projection.iter().filter_map(|item| {
                        match item {
                            SelectItem::ExprWithAlias { expr, alias } if has_aggregate(expr) => {
                                Some((expr.to_string().to_lowercase(), alias.value.clone()))
                            }
                            SelectItem::UnnamedExpr(expr) if has_aggregate(expr) => {
                                Some((expr.to_string().to_lowercase(), expr_to_name(expr)))
                            }
                            _ => None,
                        }
                    }).collect()
                } else {
                    vec![]
                };
                let keys = order_by
                    .exprs
                    .iter()
                    .map(|o| {
                        let expr = if projection.is_none() && has_aggregate(&o.expr) {
                            // Rewrite aggregate call in ORDER BY to a column reference.
                            // First, try to match by expression text to find the alias.
                            let order_expr_text = o.expr.to_string().to_lowercase();
                            let col_name = agg_alias_lookup.iter()
                                .find(|(agg_text, _)| agg_text == &order_expr_text)
                                .map(|(_, alias)| alias.clone())
                                .unwrap_or_else(|| expr_to_name(&o.expr));
                            Expr::Column { table: None, name: col_name }
                        } else {
                            self.convert_expr_with_schema(&o.expr, agg_output_schema.as_ref())?
                        };
                        let ascending = o.asc.unwrap_or(true);
                        let nulls_first = o.nulls_first.unwrap_or(!ascending);
                        Ok(SortKey {
                            expr,
                            ascending,
                            nulls_first,
                        })
                    })
                    .collect::<Result<Vec<_>, SqlError>>()?;

                // Apply Sort before Project so ORDER BY can see pre-projection columns
                let sorted = LogicalPlan::Sort {
                    input: Box::new(inner_plan),
                    keys,
                };

                // Now apply the projection on top of the sorted data
                let projected = if let Some((columns, dist)) = projection {
                    if dist {
                        // DISTINCT after sort
                        LogicalPlan::Project {
                            input: Box::new(sorted),
                            columns,
                            distinct: dist,
                        }
                    } else {
                        LogicalPlan::Project {
                            input: Box::new(sorted),
                            columns,
                            distinct: dist,
                        }
                    }
                } else {
                    sorted
                };

                // LIMIT / OFFSET / FETCH FIRST
                if query.limit.is_some() || query.offset.is_some() || query.fetch.is_some() {
                    let limit = if let Some(limit_expr) = &query.limit {
                        eval_const_u64(limit_expr)?
                    } else if let Some(fetch) = &query.fetch {
                        if let Some(qty) = &fetch.quantity {
                            eval_const_u64(qty)?
                        } else {
                            1
                        }
                    } else {
                        u64::MAX
                    };
                    let offset = if let Some(offset_clause) = &query.offset {
                        eval_const_u64(&offset_clause.value)?
                    } else {
                        0
                    };
                    return Ok(LogicalPlan::Limit {
                        input: Box::new(projected),
                        limit,
                        offset,
                    });
                }
                return Ok(projected);
            } else {
                // For non-SELECT bodies (SetOp, etc.) ORDER BY is applied after
                let mut plan = self.plan_set_expr(&query.body)?;
                let order_by = query.order_by.as_ref().unwrap();
                let keys = order_by
                    .exprs
                    .iter()
                    .map(|o| {
                        let schema = self.extract_schema_from_plan(&plan);
                        let expr = self.convert_expr_with_schema(&o.expr, schema.as_ref())?;
                        let ascending = o.asc.unwrap_or(true);
                        let nulls_first = o.nulls_first.unwrap_or(!ascending);
                        Ok(SortKey {
                            expr,
                            ascending,
                            nulls_first,
                        })
                    })
                    .collect::<Result<Vec<_>, SqlError>>()?;
                plan = LogicalPlan::Sort {
                    input: Box::new(plan),
                    keys,
                };
                plan
            }
        } else {
            self.plan_set_expr(&query.body)?
        };

        // LIMIT / OFFSET / FETCH FIRST
        let has_fetch = query.fetch.is_some();
        if query.limit.is_some() || query.offset.is_some() || has_fetch {
            let limit = if let Some(limit_expr) = &query.limit {
                eval_const_u64(limit_expr)?
            } else if let Some(fetch) = &query.fetch {
                if let Some(qty) = &fetch.quantity {
                    eval_const_u64(qty)?
                } else {
                    1 // FETCH FIRST ROW ONLY with no count = 1
                }
            } else {
                u64::MAX
            };
            let offset = if let Some(offset_clause) = &query.offset {
                eval_const_u64(&offset_clause.value)?
            } else {
                0
            };
            return Ok(LogicalPlan::Limit {
                input: Box::new(plan),
                limit,
                offset,
            });
        }

        Ok(plan)
    }

    fn plan_set_expr(&self, body: &SetExpr) -> Result<LogicalPlan, SqlError> {
        match body {
            SetExpr::Select(select) => self.plan_select(select),
            SetExpr::SetOperation {
                op,
                set_quantifier,
                left,
                right,
            } => {
                let left_plan = self.plan_set_expr(left)?;
                let right_plan = self.plan_set_expr(right)?;
                let set_op = match op {
                    SetOperator::Union => SetOperation::Union,
                    SetOperator::Intersect => SetOperation::Intersect,
                    SetOperator::Except => SetOperation::Except,
                };
                let all = matches!(
                    set_quantifier,
                    ast::SetQuantifier::All | ast::SetQuantifier::ByName
                );
                Ok(LogicalPlan::SetOp {
                    op: set_op,
                    all,
                    left: Box::new(left_plan),
                    right: Box::new(right_plan),
                })
            }
            SetExpr::Query(inner) => self.plan_query(inner),
            _ => Err(SqlError::NotImplemented(
                "non-SELECT set expression".to_string(),
            )),
        }
    }

    /// Extract window expressions from a SELECT projection.
    fn extract_window_exprs(
        &self,
        projection: &[SelectItem],
        input_plan: &LogicalPlan,
    ) -> Result<Vec<WindowExpr>, SqlError> {
        let schema = self.extract_schema_from_plan(input_plan);
        let mut window_exprs = Vec::new();
        for item in projection {
            let (expr_ast, alias) = match item {
                SelectItem::UnnamedExpr(e) => (e, expr_to_name(e)),
                SelectItem::ExprWithAlias { expr, alias } => (expr, alias.value.clone()),
                _ => continue,
            };
            if has_window_function(expr_ast) {
                if let AstExpr::Function(f) = expr_ast {
                    let func_name = f.name.to_string().to_lowercase();
                    // Parse partition_by and order_by from the OVER clause
                    let (partition_by, order_by) = if let Some(over) = &f.over {
                        match over {
                            ast::WindowType::WindowSpec(spec) => {
                                let pb: Result<Vec<Expr>, _> = spec.partition_by.iter()
                                    .map(|e| self.convert_expr_with_schema(e, schema.as_ref()))
                                    .collect();
                                let ob: Result<Vec<SortKey>, SqlError> = spec.order_by.iter()
                                    .map(|o| {
                                        let expr = self.convert_expr_with_schema(&o.expr, schema.as_ref())?;
                                        Ok(SortKey {
                                            expr,
                                            ascending: o.asc.unwrap_or(true),
                                            nulls_first: o.nulls_first.unwrap_or(false),
                                        })
                                    })
                                    .collect();
                                (pb?, ob?)
                            }
                            _ => (vec![], vec![]),
                        }
                    } else {
                        (vec![], vec![])
                    };

                    // Parse the window function
                    let win_func = match func_name.as_str() {
                        "row_number" => WindowFunction::RowNumber,
                        "rank" => WindowFunction::Rank,
                        "dense_rank" => WindowFunction::DenseRank,
                        "cume_dist" => WindowFunction::CumeDist,
                        "percent_rank" => WindowFunction::PercentRank,
                        "ntile" => {
                            let arg = self.get_window_func_arg(f, schema.as_ref())?;
                            WindowFunction::Ntile(Box::new(arg))
                        }
                        "sum" => {
                            let arg = self.get_window_func_arg(f, schema.as_ref())?;
                            WindowFunction::Sum(Box::new(arg))
                        }
                        "avg" => {
                            let arg = self.get_window_func_arg(f, schema.as_ref())?;
                            WindowFunction::Avg(Box::new(arg))
                        }
                        "min" => {
                            let arg = self.get_window_func_arg(f, schema.as_ref())?;
                            WindowFunction::Min(Box::new(arg))
                        }
                        "max" => {
                            let arg = self.get_window_func_arg(f, schema.as_ref())?;
                            WindowFunction::Max(Box::new(arg))
                        }
                        "count" => {
                            let arg = self.get_window_func_arg(f, schema.as_ref())?;
                            WindowFunction::Count(Box::new(arg))
                        }
                        "lead" => {
                            let arg = self.get_window_func_arg(f, schema.as_ref())?;
                            let offset = self.get_window_func_int_arg(f, 1).unwrap_or(1);
                            let default = self.get_window_func_optional_arg(f, 2, schema.as_ref())?;
                            WindowFunction::Lead { expr: Box::new(arg), offset, default: default.map(Box::new) }
                        }
                        "lag" => {
                            let arg = self.get_window_func_arg(f, schema.as_ref())?;
                            let offset = self.get_window_func_int_arg(f, 1).unwrap_or(1);
                            let default = self.get_window_func_optional_arg(f, 2, schema.as_ref())?;
                            WindowFunction::Lag { expr: Box::new(arg), offset, default: default.map(Box::new) }
                        }
                        "first_value" => {
                            let arg = self.get_window_func_arg(f, schema.as_ref())?;
                            WindowFunction::FirstValue(Box::new(arg))
                        }
                        "last_value" => {
                            let arg = self.get_window_func_arg(f, schema.as_ref())?;
                            WindowFunction::LastValue(Box::new(arg))
                        }
                        "nth_value" => {
                            let arg = self.get_window_func_arg(f, schema.as_ref())?;
                            let n = self.get_window_func_int_arg(f, 1).unwrap_or(1);
                            WindowFunction::NthValue { expr: Box::new(arg), n }
                        }
                        _ => return Err(SqlError::NotImplemented(format!("window function: {}", func_name))),
                    };

                    window_exprs.push(WindowExpr {
                        output_name: alias,
                        function: win_func,
                        partition_by,
                        order_by,
                    });
                }
            }
        }
        Ok(window_exprs)
    }

    fn get_window_func_int_arg(&self, f: &ast::Function, idx: usize) -> Option<i64> {
        match &f.args {
            FunctionArguments::List(list) => {
                list.args.get(idx).and_then(|a| match a {
                    ast::FunctionArg::Unnamed(ast::FunctionArgExpr::Expr(
                        AstExpr::Value(AstValue::Number(n, _))
                    )) => n.parse::<i64>().ok(),
                    _ => None,
                })
            }
            _ => None,
        }
    }

    fn get_window_func_optional_arg(
        &self,
        f: &ast::Function,
        idx: usize,
        schema: Option<&Vec<(String, catalog::DataType)>>,
    ) -> Result<Option<Expr>, SqlError> {
        match &f.args {
            FunctionArguments::List(list) => {
                match list.args.get(idx) {
                    Some(ast::FunctionArg::Unnamed(ast::FunctionArgExpr::Expr(e))) => {
                        Ok(Some(self.convert_expr_with_schema(e, schema)?))
                    }
                    _ => Ok(None),
                }
            }
            _ => Ok(None),
        }
    }

    fn get_window_func_arg(
        &self,
        f: &ast::Function,
        schema: Option<&Vec<(String, catalog::DataType)>>,
    ) -> Result<Expr, SqlError> {
        match &f.args {
            FunctionArguments::List(list) => {
                match list.args.first() {
                    Some(ast::FunctionArg::Unnamed(ast::FunctionArgExpr::Expr(e))) => {
                        self.convert_expr_with_schema(e, schema)
                    }
                    Some(ast::FunctionArg::Unnamed(ast::FunctionArgExpr::Wildcard)) => {
                        Ok(Expr::Literal(Value::Int4(1)))
                    }
                    _ => Ok(Expr::Literal(Value::Int4(1))),
                }
            }
            _ => Ok(Expr::Literal(Value::Int4(1))),
        }
    }

    /// Build the SELECT body plan and return the inner plan (before final projection) plus
    /// the projection definition `(columns, distinct)`.  When the SELECT has aggregates the
    /// projection is already folded into the returned plan (Aggregate + Project) and `None` is
    /// returned for the projection tuple.  For a plain SELECT without aggregates, the caller
    /// gets the pre-projection plan and can insert a Sort node before wrapping in Project.
    #[allow(clippy::type_complexity)]
    fn plan_select_body(
        &self,
        select: &Select,
    ) -> Result<(LogicalPlan, Option<(Vec<(String, Expr)>, bool)>), SqlError> {
        let distinct = matches!(&select.distinct, Some(ast::Distinct::Distinct));

        // SELECT without FROM
        if select.from.is_empty() {
            let columns = self.build_projection_no_from(&select.projection)?;
            let inner = LogicalPlan::TableScan {
                table_name: "__dual__".to_string(),
                alias: None,
                schema: TableSchema {
                    oid: 0,
                    name: "__dual__".to_string(),
                    namespace_oid: 0,
                    columns: vec![],
                    foreign_keys: vec![],
                    check_constraints: vec![],
                },
                filter: None,
            };
            return Ok((inner, Some((columns, distinct))));
        }

        // Multi-table FROM (comma-separated) => cross join chain
        let mut plan = if select.from.len() > 1 {
            let mut plan = self.plan_table_with_joins(&select.from[0])?;
            for from in &select.from[1..] {
                // Check for LATERAL: single-relation FROM item with lateral=true
                if from.joins.is_empty() {
                    if let Some((subquery, alias)) = self.try_plan_lateral_factor(&from.relation)? {
                        plan = LogicalPlan::Lateral {
                            outer: Box::new(plan),
                            subquery: Box::new(subquery),
                            alias,
                        };
                        continue;
                    }
                }
                let right = self.plan_table_with_joins(from)?;
                plan = LogicalPlan::Join {
                    left: Box::new(plan),
                    right: Box::new(right),
                    join_type: JoinType::Cross,
                    condition: Expr::Literal(Value::Bool(true)),
                    using_columns: vec![],
                    algorithm: JoinAlgorithm::NestedLoop,
                };
            }
            plan
        } else {
            self.plan_table_with_joins(&select.from[0])?
        };

        // WHERE clause
        if let Some(selection) = &select.selection {
            let schema = self.extract_schema_from_plan(&plan);
            let predicate = self.convert_expr_with_schema(selection, schema.as_ref())?;
            if let Some(index_plan) = self.try_index_scan(&plan, &predicate) {
                plan = index_plan;
            } else {
                plan = LogicalPlan::Filter {
                    input: Box::new(plan),
                    predicate,
                };
            }
        }

        // GROUP BY and aggregates
        let has_agg = self.projection_has_aggregates(&select.projection);
        let group_by_exprs = self.extract_group_by(&select.group_by, &plan)?;

        if has_agg || !group_by_exprs.is_empty() {
            let mut aggregates = self.extract_aggregates(&select.projection, &plan)?;
            // Also collect aggregate functions referenced only in HAVING (not in projection)
            if let Some(having_ast) = &select.having {
                let extra = self.extract_having_aggregates(having_ast, &aggregates, &plan)?;
                aggregates.extend(extra);
            }
            let select_exprs = self.build_select_expr_list(&select.projection, &plan)?;
            self.validate_aggregate_select(&select_exprs, &group_by_exprs, &aggregates)?;
            let having = if let Some(having_ast) = &select.having {
                Some(self.convert_having_expr(having_ast, &aggregates)?)
            } else {
                None
            };
            plan = LogicalPlan::Aggregate {
                input: Box::new(plan),
                group_by: group_by_exprs,
                aggregates,
                having,
            };
            let agg_output_schema = self.extract_schema_from_plan(&plan);
            let projection_columns =
                self.build_post_agg_projection(&select.projection, agg_output_schema.as_ref())?;
            if !self.is_all_columns(&select.projection) {
                plan = LogicalPlan::Project {
                    input: Box::new(plan),
                    columns: projection_columns,
                    distinct,
                };
            }
            // For aggregate queries, projection is already applied — return no separate projection
            Ok((plan, None))
        } else {
            // Check if any SELECT items have window functions
            let has_window = select.projection.iter().any(|item| match item {
                SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => has_window_function(e),
                _ => false,
            });

            if has_window {
                // Extract window expressions and wrap input plan
                let window_exprs = self.extract_window_exprs(&select.projection, &plan)?;
                let window_plan = LogicalPlan::Window {
                    input: Box::new(plan),
                    window_exprs,
                };
                // Build projection that references the window output columns
                let schema = self.extract_schema_from_plan(&window_plan);
                let projection_columns =
                    self.build_projection(&select.projection, schema.as_ref(), &window_plan)?;
                Ok((window_plan, Some((projection_columns, distinct))))
            } else {
                // Return the pre-projection plan and the projection definition separately
                let schema = self.extract_schema_from_plan(&plan);
                let projection_columns =
                    self.build_projection(&select.projection, schema.as_ref(), &plan)?;
                Ok((plan, Some((projection_columns, distinct))))
            }
        }
    }

    fn plan_select(&self, select: &Select) -> Result<LogicalPlan, SqlError> {
        let distinct = matches!(
            &select.distinct,
            Some(ast::Distinct::Distinct)
        );

        // Determine what tables are being scanned
        if select.from.is_empty() {
            // SELECT without FROM (e.g., SELECT 1)
            // Build a single-row plan
            let columns = self.build_projection_no_from(&select.projection)?;
            // Create a trivial plan: we'll handle this as a Project over a dummy scan
            return Ok(LogicalPlan::Project {
                input: Box::new(LogicalPlan::TableScan {
                    table_name: "__dual__".to_string(),
                    alias: None,
                    schema: TableSchema {
                        oid: 0,
                        name: "__dual__".to_string(),
                        namespace_oid: 0,
                        columns: vec![],
                        foreign_keys: vec![],
                        check_constraints: vec![],
                    },
                    filter: None,
                }),
                columns,
                distinct,
            });
        }

        // Multi-table FROM (comma-separated) => cross join chain
        let mut plan = if select.from.len() > 1 {
            let mut plan = self.plan_table_with_joins(&select.from[0])?;
            for from in &select.from[1..] {
                // Check for LATERAL: single-relation FROM item with lateral=true
                if from.joins.is_empty() {
                    if let Some((subquery, alias)) = self.try_plan_lateral_factor(&from.relation)? {
                        plan = LogicalPlan::Lateral {
                            outer: Box::new(plan),
                            subquery: Box::new(subquery),
                            alias,
                        };
                        continue;
                    }
                }
                let right = self.plan_table_with_joins(from)?;
                // Cross join: condition = true
                plan = LogicalPlan::Join {
                    left: Box::new(plan),
                    right: Box::new(right),
                    join_type: JoinType::Cross,
                    condition: Expr::Literal(Value::Bool(true)),
                    using_columns: vec![],
                    algorithm: JoinAlgorithm::NestedLoop,
                };
            }
            plan
        } else {
            self.plan_table_with_joins(&select.from[0])?
        };

        // WHERE clause
        if let Some(selection) = &select.selection {
            let schema = self.extract_schema_from_plan(&plan);
            let predicate = self.convert_expr_with_schema(selection, schema.as_ref())?;

            // Try to convert Filter(TableScan) into IndexScan if a matching index exists.
            if let Some(index_plan) = self.try_index_scan(&plan, &predicate) {
                plan = index_plan;
            } else {
                plan = LogicalPlan::Filter {
                    input: Box::new(plan),
                    predicate,
                };
            }
        }

        // GROUP BY and aggregates
        let has_agg = self.projection_has_aggregates(&select.projection);
        let group_by_exprs = self.extract_group_by(&select.group_by, &plan)?;

        if has_agg || !group_by_exprs.is_empty() {
            let mut aggregates = self.extract_aggregates(&select.projection, &plan)?;

            // Also collect aggregate functions referenced only in HAVING (not in projection)
            if let Some(having_ast) = &select.having {
                let extra = self.extract_having_aggregates(having_ast, &aggregates, &plan)?;
                aggregates.extend(extra);
            }

            // Validate GROUP BY / aggregate consistency
            let select_exprs = self.build_select_expr_list(&select.projection, &plan)?;
            self.validate_aggregate_select(&select_exprs, &group_by_exprs, &aggregates)?;

            // Convert HAVING clause — resolve aggregate function calls to their output aliases
            let having = if let Some(having_ast) = &select.having {
                Some(self.convert_having_expr(having_ast, &aggregates)?)
            } else {
                None
            };

            plan = LogicalPlan::Aggregate {
                input: Box::new(plan),
                group_by: group_by_exprs,
                aggregates,
                having,
            };
            // After aggregate, build projection using column references to aggregate outputs
            // The aggregate plan produces columns by their alias names
            let agg_output_schema = self.extract_schema_from_plan(&plan);
            let projection_columns =
                self.build_post_agg_projection(&select.projection, agg_output_schema.as_ref())?;
            // Only add Project if needed (not a simple SELECT * FROM agg)
            if !self.is_all_columns(&select.projection) {
                plan = LogicalPlan::Project {
                    input: Box::new(plan),
                    columns: projection_columns,
                    distinct,
                };
            }
        } else {
            // Check if any SELECT items have window functions
            let has_window = select.projection.iter().any(|item| match item {
                SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => has_window_function(e),
                _ => false,
            });

            if has_window {
                // Extract window expressions and wrap input plan
                let window_exprs = self.extract_window_exprs(&select.projection, &plan)?;
                plan = LogicalPlan::Window {
                    input: Box::new(plan),
                    window_exprs,
                };
            }

            // SELECT list (projection) for non-aggregate queries
            let schema = self.extract_schema_from_plan(&plan);
            let projection_columns =
                self.build_projection(&select.projection, schema.as_ref(), &plan)?;
            plan = LogicalPlan::Project {
                input: Box::new(plan),
                columns: projection_columns,
                distinct,
            };
        }

        Ok(plan)
    }

    fn plan_table_with_joins(&self, from: &TableWithJoins) -> Result<LogicalPlan, SqlError> {
        let mut plan = self.plan_table_factor(&from.relation)?;

        for join in &from.joins {
            // Check if the join relation is a LATERAL derived subquery
            if let Some((subquery, alias)) = self.try_plan_lateral_factor(&join.relation)? {
                plan = LogicalPlan::Lateral {
                    outer: Box::new(plan),
                    subquery: Box::new(subquery),
                    alias,
                };
                continue;
            }
            let right = self.plan_table_factor(&join.relation)?;
            let (join_type, condition, using_columns) = match &join.join_operator {
                JoinOperator::Inner(constraint) => {
                    let (cond, using) =
                        self.extract_join_condition_with_using(constraint, &plan, &right)?;
                    (JoinType::Inner, cond, using)
                }
                JoinOperator::LeftOuter(constraint) => {
                    let (cond, using) =
                        self.extract_join_condition_with_using(constraint, &plan, &right)?;
                    (JoinType::Left, cond, using)
                }
                JoinOperator::RightOuter(constraint) => {
                    let (cond, using) =
                        self.extract_join_condition_with_using(constraint, &plan, &right)?;
                    (JoinType::Right, cond, using)
                }
                JoinOperator::FullOuter(constraint) => {
                    let (cond, using) =
                        self.extract_join_condition_with_using(constraint, &plan, &right)?;
                    (JoinType::Full, cond, using)
                }
                JoinOperator::CrossJoin => {
                    (JoinType::Cross, Expr::Literal(Value::Bool(true)), vec![])
                }
                _ => return Err(SqlError::NotImplemented("join type".to_string())),
            };
            let algorithm = infer_join_algorithm(&condition);
            plan = LogicalPlan::Join {
                left: Box::new(plan),
                right: Box::new(right),
                join_type,
                condition,
                using_columns,
                algorithm,
            };
        }

        Ok(plan)
    }

    fn extract_join_condition_with_using(
        &self,
        constraint: &ast::JoinConstraint,
        _left: &LogicalPlan,
        _right: &LogicalPlan,
    ) -> Result<(Expr, Vec<String>), SqlError> {
        match constraint {
            ast::JoinConstraint::On(expr) => {
                // Convert without schema for joins (column refs resolved at eval time)
                let cond = self.convert_expr_with_schema(expr, None)?;
                Ok((cond, vec![]))
            }
            ast::JoinConstraint::Using(cols) => {
                // USING(col) => left.col = right.col
                if cols.is_empty() {
                    return Err(SqlError::Execution(
                        "USING requires at least one column".to_string(),
                    ));
                }
                let using_names: Vec<String> = cols.iter().map(|c| c.value.clone()).collect();
                let col_name = cols[0].value.clone();
                let mut cond = Expr::BinaryOp {
                    left: Box::new(Expr::Column {
                        table: None,
                        name: col_name.clone(),
                    }),
                    op: BinaryOp::Eq,
                    right: Box::new(Expr::Column {
                        table: None,
                        name: col_name,
                    }),
                };
                for c in &cols[1..] {
                    let name = c.value.clone();
                    let next = Expr::BinaryOp {
                        left: Box::new(Expr::Column {
                            table: None,
                            name: name.clone(),
                        }),
                        op: BinaryOp::Eq,
                        right: Box::new(Expr::Column { table: None, name }),
                    };
                    cond = Expr::BinaryOp {
                        left: Box::new(cond),
                        op: BinaryOp::And,
                        right: Box::new(next),
                    };
                }
                Ok((cond, using_names))
            }
            ast::JoinConstraint::Natural => {
                Err(SqlError::NotImplemented("NATURAL JOIN".to_string()))
            }
            ast::JoinConstraint::None => Ok((Expr::Literal(Value::Bool(true)), vec![])),
        }
    }

    fn plan_table_factor(&self, factor: &TableFactor) -> Result<LogicalPlan, SqlError> {
        match factor {
            TableFactor::Table { name, alias, args, .. } => {
                let parts: Vec<String> = name.0.iter().map(|i| i.value.clone()).collect();
                let table_name = parts.last().cloned().unwrap_or_default();
                let lower_name = table_name.to_lowercase();

                // Check for table-valued function: GENERATE_SERIES(start, stop[, step])
                if lower_name == "generate_series" {
                    if let Some(func_args) = args {
                        return self.plan_generate_series(&func_args.args);
                    }
                }

                // Multi-part name: check if first part is a system schema
                if parts.len() >= 2 {
                    let schema_part = parts[0].to_lowercase();
                    let tbl = parts[1].to_lowercase();
                    if schema_part == "information_schema" || schema_part == "pg_catalog" {
                        return Ok(LogicalPlan::SystemCatalogScan {
                            catalog_name: schema_part,
                            table_name: tbl,
                            filter: None,
                        });
                    }
                }
                // Single-part: check if it's a known system catalog table name
                if matches!(lower_name.as_str(),
                    "pg_tables" | "pg_class" | "pg_attribute" | "pg_namespace" |
                    "pg_indexes" | "pg_type" | "pg_roles" | "pg_views" |
                    "pg_stat_user_tables"
                ) {
                    return Ok(LogicalPlan::SystemCatalogScan {
                        catalog_name: "pg_catalog".to_string(),
                        table_name: lower_name,
                        filter: None,
                    });
                }
                let schema = self.lookup_table(&table_name)?;
                // The alias is used for qualified column references (e.g. u.id when alias is "u").
                // When no explicit alias is given, use the table name so that table-qualified
                // references like `tablename.col` resolve correctly in JOINs.
                let alias_name = alias
                    .as_ref()
                    .map(|a| a.name.value.clone())
                    .unwrap_or_else(|| table_name.clone());
                Ok(LogicalPlan::TableScan {
                    table_name: table_name.clone(),
                    alias: Some(alias_name),
                    schema,
                    filter: None,
                })
            }
            TableFactor::Derived {
                subquery, alias, lateral,
            } => {
                // LATERAL subqueries need the outer plan available — handled in
                // plan_table_with_joins and the multi-table FROM assembler.
                // Here we just plan the inner subquery; the caller wraps it in Lateral
                // when lateral=true and an outer plan is available.
                let _ = lateral; // used by callers
                let inner = self.plan_query_with_cte(subquery)?;
                // Wrap as a CTE-like plan with alias so columns can be referenced
                let _alias_name = alias.as_ref().map(|a| a.name.value.clone());
                Ok(inner)
            }
            _ => Err(SqlError::NotImplemented("complex table factor".to_string())),
        }
    }

    /// Return `(inner_subquery_plan, alias_string)` for a `TableFactor::Derived`
    /// that has `lateral = true`.  Returns `None` if the factor is not lateral-derived.
    #[allow(dead_code)]
    fn try_plan_lateral_factor(
        &self,
        factor: &TableFactor,
    ) -> Result<Option<(LogicalPlan, String)>, SqlError> {
        if let TableFactor::Derived { lateral: true, subquery, alias } = factor {
            let inner = self.plan_query_with_cte(subquery)?;
            let alias_name = alias
                .as_ref()
                .map(|a| a.name.value.clone())
                .unwrap_or_default();
            Ok(Some((inner, alias_name)))
        } else {
            Ok(None)
        }
    }

    fn plan_generate_series(&self, args: &[ast::FunctionArg]) -> Result<LogicalPlan, SqlError> {
        let exprs: Vec<&AstExpr> = args.iter().filter_map(|a| {
            if let ast::FunctionArg::Unnamed(ast::FunctionArgExpr::Expr(e)) = a {
                Some(e)
            } else {
                None
            }
        }).collect();

        if exprs.len() < 2 {
            return Err(SqlError::Execution(
                "generate_series requires at least 2 arguments".to_string(),
            ));
        }

        let start = self.convert_expr_with_schema(exprs[0], None)?;
        let stop = self.convert_expr_with_schema(exprs[1], None)?;
        let step = if exprs.len() >= 3 {
            self.convert_expr_with_schema(exprs[2], None)?
        } else {
            Expr::Literal(Value::Int8(1))
        };

        Ok(LogicalPlan::GenerateSeries { start, stop, step })
    }

    fn lookup_table(&self, table_name: &str) -> Result<TableSchema, SqlError> {
        // If the table name is a registered CTE, return a placeholder schema.
        // The executor will substitute the real materialized rows at runtime.
        if self.cte_names.borrow().iter().any(|n| n == table_name) {
            return Ok(TableSchema {
                oid: 0,
                name: table_name.to_string(),
                namespace_oid: 0,
                columns: vec![],
                foreign_keys: vec![],
                check_constraints: vec![],
            });
        }
        // Try public schema first
        self.catalog
            .get_table("public", table_name)
            .map_err(|e| match e {
                catalog::error::CatalogError::TableNotFound(_) => {
                    SqlError::TableNotFound(table_name.to_string())
                }
                other => SqlError::Catalog(other),
            })
    }

    /// Try to convert a `Filter(TableScan)` into an `IndexScan` when the
    /// predicate is a simple equality on an indexed column.
    fn try_index_scan(&self, plan: &LogicalPlan, predicate: &Expr) -> Option<LogicalPlan> {
        // Only optimize a direct TableScan (no joins, etc.)
        let (table_name, schema) = match plan {
            LogicalPlan::TableScan {
                table_name,
                schema,
                ..
            } => (table_name, schema),
            _ => return None,
        };

        // Look for: col = literal  or  literal = col
        let (col_name, eq_val) = match predicate {
            Expr::BinaryOp {
                left,
                op: BinaryOp::Eq,
                right,
            } => match (left.as_ref(), right.as_ref()) {
                (Expr::Column { name, .. }, Expr::Literal(v)) => (name.clone(), v.clone()),
                (Expr::Literal(v), Expr::Column { name, .. }) => (name.clone(), v.clone()),
                _ => return None,
            },
            _ => return None,
        };

        // Check if an index exists for this column.
        if let Ok(table_oid) = self.catalog.get_table_oid("public", table_name) {
            if self.catalog.get_index_path(table_oid, &col_name).is_some() {
                return Some(LogicalPlan::IndexScan {
                    table_name: table_name.clone(),
                    schema: schema.clone(),
                    index_column: col_name,
                    eq_value: Some(eq_val),
                    range_start: None,
                    range_end: None,
                    filter: None,
                });
            }
        }
        None
    }

    #[allow(clippy::only_used_in_recursion)]
    fn extract_schema_from_plan(
        &self,
        plan: &LogicalPlan,
    ) -> Option<Vec<(String, catalog::DataType)>> {
        match plan {
            LogicalPlan::TableScan { schema, alias, .. } => {
                // Include both qualified (alias.col) and unqualified (col) entries
                let mut cols = Vec::new();
                for c in &schema.columns {
                    if let Some(a) = alias.as_deref() {
                        cols.push((format!("{}.{}", a, c.name), c.data_type.clone()));
                    }
                    cols.push((c.name.clone(), c.data_type.clone()));
                }
                Some(cols)
            }
            LogicalPlan::IndexScan { schema, .. } => Some(
                schema
                    .columns
                    .iter()
                    .map(|c| (c.name.clone(), c.data_type.clone()))
                    .collect(),
            ),
            LogicalPlan::Filter { input, .. } => self.extract_schema_from_plan(input),
            LogicalPlan::Project { columns, .. } => {
                // We don't know types in general without full type inference
                Some(
                    columns
                        .iter()
                        .map(|(name, _)| (name.clone(), catalog::DataType::Text))
                        .collect(),
                )
            }
            LogicalPlan::Join { left, right, .. } => {
                let mut cols = self.extract_schema_from_plan(left).unwrap_or_default();
                cols.extend(self.extract_schema_from_plan(right).unwrap_or_default());
                Some(cols)
            }
            LogicalPlan::Aggregate { aggregates, .. } => Some(
                aggregates
                    .iter()
                    .map(|(name, _, _)| (name.clone(), catalog::DataType::Int8))
                    .collect(),
            ),
            LogicalPlan::Window { input, window_exprs } => {
                // Window adds new columns after the input schema
                let mut s = self.extract_schema_from_plan(input).unwrap_or_default();
                for we in window_exprs {
                    s.push((we.output_name.clone(), catalog::DataType::Int8));
                }
                Some(s)
            }
            _ => None,
        }
    }

    fn build_projection_no_from(
        &self,
        projection: &[SelectItem],
    ) -> Result<Vec<(String, Expr)>, SqlError> {
        let mut cols = Vec::new();
        for item in projection {
            match item {
                SelectItem::UnnamedExpr(expr) => {
                    let e = self.convert_expr_with_schema(expr, None)?;
                    let name = expr_to_name(expr);
                    cols.push((name, e));
                }
                SelectItem::ExprWithAlias { expr, alias } => {
                    let e = self.convert_expr_with_schema(expr, None)?;
                    cols.push((alias.value.clone(), e));
                }
                _ => return Err(SqlError::NotImplemented("projection item".to_string())),
            }
        }
        Ok(cols)
    }

    /// Build projection columns after an aggregate, using column references to aggregate outputs.
    fn build_post_agg_projection(
        &self,
        projection: &[SelectItem],
        agg_schema: Option<&Vec<(String, catalog::DataType)>>,
    ) -> Result<Vec<(String, Expr)>, SqlError> {
        let mut cols = Vec::new();
        let schema = agg_schema.cloned().unwrap_or_default();

        for item in projection {
            match item {
                SelectItem::Wildcard(_) => {
                    for (name, _) in &schema {
                        cols.push((
                            name.clone(),
                            Expr::Column {
                                table: None,
                                name: name.clone(),
                            },
                        ));
                    }
                }
                SelectItem::UnnamedExpr(expr) => {
                    let name = expr_to_name(expr);
                    // Use column reference to the aggregate output name
                    cols.push((name.clone(), Expr::Column { table: None, name }));
                }
                SelectItem::ExprWithAlias { expr: _, alias } => {
                    // Use the alias as a column reference to the aggregate output
                    cols.push((
                        alias.value.clone(),
                        Expr::Column {
                            table: None,
                            name: alias.value.clone(),
                        },
                    ));
                }
                _ => {
                    return Err(SqlError::NotImplemented(
                        "projection item in aggregate".to_string(),
                    ))
                }
            }
        }
        Ok(cols)
    }

    fn is_all_columns(&self, projection: &[SelectItem]) -> bool {
        projection.iter().all(|p| {
            matches!(
                p,
                SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _)
            )
        })
    }

    fn build_projection(
        &self,
        projection: &[SelectItem],
        schema: Option<&Vec<(String, catalog::DataType)>>,
        plan: &LogicalPlan,
    ) -> Result<Vec<(String, Expr)>, SqlError> {
        let mut cols = Vec::new();
        for item in projection {
            match item {
                SelectItem::Wildcard(_) => {
                    // Expand to all columns — skip qualified "alias.col" entries since the
                    // unqualified "col" entries cover the same values and avoid duplicates.
                    if let Some(s) = schema {
                        for (name, _) in s {
                            if !name.contains('.') {
                                cols.push((
                                    name.clone(),
                                    Expr::Column {
                                        table: None,
                                        name: name.clone(),
                                    },
                                ));
                            }
                        }
                    } else if let Some(s) = self.extract_full_schema_from_plan(plan) {
                        for (name, _) in s {
                            if !name.contains('.') {
                                cols.push((
                                    name.clone(),
                                    Expr::Column {
                                        table: None,
                                        name: name.clone(),
                                    },
                                ));
                            }
                        }
                    }
                }
                SelectItem::QualifiedWildcard(table_obj, _) => {
                    // table.*
                    let table_prefix = table_obj
                        .0
                        .last()
                        .map(|i| i.value.clone())
                        .unwrap_or_default();
                    if let Some(s) = self.extract_full_schema_from_plan(plan) {
                        for (name, _) in s {
                            cols.push((
                                name.clone(),
                                Expr::Column {
                                    table: Some(table_prefix.clone()),
                                    name: name.clone(),
                                },
                            ));
                        }
                    }
                }
                SelectItem::UnnamedExpr(expr) => {
                    let name = expr_to_name(expr);
                    // For window functions, emit a Column placeholder using the output name
                    // that extract_window_exprs assigns (which is expr_to_name = the function name).
                    let e = if has_window_function(expr) {
                        Expr::Column { table: None, name: name.clone() }
                    } else {
                        self.convert_expr_with_schema(expr, schema)?
                    };
                    cols.push((name, e));
                }
                SelectItem::ExprWithAlias { expr, alias } => {
                    // For window functions, emit a Column placeholder using the alias,
                    // which matches the output_name set by extract_window_exprs.
                    let e = if has_window_function(expr) {
                        Expr::Column { table: None, name: alias.value.clone() }
                    } else {
                        self.convert_expr_with_schema(expr, schema)?
                    };
                    cols.push((alias.value.clone(), e));
                }
            }
        }
        Ok(cols)
    }

    #[allow(clippy::only_used_in_recursion)]
    fn extract_full_schema_from_plan(
        &self,
        plan: &LogicalPlan,
    ) -> Option<Vec<(String, catalog::DataType)>> {
        match plan {
            LogicalPlan::TableScan { schema, alias, .. } => {
                let mut cols = Vec::new();
                for c in &schema.columns {
                    if let Some(a) = alias.as_deref() {
                        cols.push((format!("{}.{}", a, c.name), c.data_type.clone()));
                    }
                    cols.push((c.name.clone(), c.data_type.clone()));
                }
                Some(cols)
            }
            LogicalPlan::IndexScan { schema, .. } => Some(
                schema
                    .columns
                    .iter()
                    .map(|c| (c.name.clone(), c.data_type.clone()))
                    .collect(),
            ),
            LogicalPlan::Filter { input, .. } => self.extract_full_schema_from_plan(input),
            LogicalPlan::Join { left, right, .. } => {
                let mut s = self.extract_full_schema_from_plan(left).unwrap_or_default();
                s.extend(
                    self.extract_full_schema_from_plan(right)
                        .unwrap_or_default(),
                );
                Some(s)
            }
            LogicalPlan::Aggregate {
                aggregates,
                group_by,
                input,
                ..
            } => {
                let mut s = Vec::new();
                // group by columns
                if let Some(input_schema) = self.extract_full_schema_from_plan(input) {
                    for (i, _) in group_by.iter().enumerate() {
                        if let Some((name, dt)) = input_schema.get(i) {
                            s.push((name.clone(), dt.clone()));
                        }
                    }
                }
                for (name, _, _) in aggregates {
                    s.push((name.clone(), catalog::DataType::Int8));
                }
                Some(s)
            }
            LogicalPlan::Window { input, window_exprs } => {
                let mut s = self.extract_full_schema_from_plan(input).unwrap_or_default();
                for we in window_exprs {
                    s.push((we.output_name.clone(), catalog::DataType::Int8));
                }
                Some(s)
            }
            _ => None,
        }
    }

    fn projection_has_aggregates(&self, projection: &[SelectItem]) -> bool {
        projection.iter().any(|item| match item {
            SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                has_aggregate(expr)
            }
            _ => false,
        })
    }

    fn extract_group_by(
        &self,
        group_by: &GroupByExpr,
        plan: &LogicalPlan,
    ) -> Result<Vec<Expr>, SqlError> {
        let schema = self.extract_schema_from_plan(plan);
        match group_by {
            GroupByExpr::Expressions(exprs, _) => exprs
                .iter()
                .map(|e| self.convert_expr_with_schema(e, schema.as_ref()))
                .collect(),
            GroupByExpr::All(_) => Ok(vec![]),
        }
    }

    fn extract_aggregates(
        &self,
        projection: &[SelectItem],
        plan: &LogicalPlan,
    ) -> Result<Vec<(String, AggFunc, Expr)>, SqlError> {
        let schema = self.extract_schema_from_plan(plan);
        let mut aggs = Vec::new();
        for item in projection {
            match item {
                SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                    let alias = match item {
                        SelectItem::ExprWithAlias { alias, .. } => alias.value.clone(),
                        _ => expr_to_name(expr),
                    };
                    if let Some((func, arg)) = extract_agg_from_expr(expr) {
                        let arg_expr = self.convert_expr_with_schema(&arg, schema.as_ref())?;
                        aggs.push((alias, func, arg_expr));
                    }
                }
                _ => {}
            }
        }
        Ok(aggs)
    }

    /// Extract aggregate functions referenced in a HAVING clause that are not already in
    /// the aggregates list from the projection. These "hidden" aggregates must be computed
    /// so the HAVING predicate can reference them.
    fn extract_having_aggregates(
        &self,
        having_ast: &AstExpr,
        existing: &[(String, AggFunc, Expr)],
        plan: &LogicalPlan,
    ) -> Result<Vec<(String, AggFunc, Expr)>, SqlError> {
        let schema = self.extract_schema_from_plan(plan);
        let mut extra = Vec::new();
        self.collect_agg_from_expr(having_ast, existing, schema.as_ref(), &mut extra)?;
        Ok(extra)
    }

    fn collect_agg_from_expr(
        &self,
        expr: &AstExpr,
        existing: &[(String, AggFunc, Expr)],
        schema: Option<&Vec<(String, catalog::DataType)>>,
        out: &mut Vec<(String, AggFunc, Expr)>,
    ) -> Result<(), SqlError> {
        if let Some((func, arg_ast)) = extract_agg_from_expr(expr) {
            let alias = expr_to_name(expr);
            // Only add if not already in existing or out
            let already_known = existing.iter().any(|(a, _, _)| a == &alias)
                || out.iter().any(|(a, _, _)| a == &alias);
            if !already_known {
                let arg_expr = self.convert_expr_with_schema(&arg_ast, schema)?;
                out.push((alias, func, arg_expr));
            }
            return Ok(());
        }
        // Recurse into sub-expressions
        match expr {
            AstExpr::BinaryOp { left, right, .. } => {
                self.collect_agg_from_expr(left, existing, schema, out)?;
                self.collect_agg_from_expr(right, existing, schema, out)?;
            }
            AstExpr::UnaryOp { expr, .. } => {
                self.collect_agg_from_expr(expr, existing, schema, out)?;
            }
            AstExpr::Nested(inner) => {
                self.collect_agg_from_expr(inner, existing, schema, out)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn plan_insert(
        &self,
        table: &TableWithJoins,
        columns: &[Ident],
        source: Option<&Query>,
        returning_ast: Option<&Vec<SelectItem>>,
        on_insert: Option<&ast::OnInsert>,
    ) -> Result<LogicalPlan, SqlError> {
        let table_name = match &table.relation {
            TableFactor::Table { name, .. } => {
                name.0.last().map(|i| i.value.clone()).unwrap_or_default()
            }
            _ => {
                return Err(SqlError::NotImplemented(
                    "complex INSERT target".to_string(),
                ))
            }
        };

        let schema = self.lookup_table(&table_name)?;
        let col_names: Vec<String> = columns.iter().map(|i| i.value.clone()).collect();

        let source_plan = if let Some(q) = source {
            match q.body.as_ref() {
                SetExpr::Values(values_clause) => {
                    let mut rows = Vec::new();
                    for row in &values_clause.rows {
                        let exprs = row
                            .iter()
                            .map(|e| self.convert_expr_with_schema(e, None))
                            .collect::<Result<Vec<_>, _>>()?;
                        rows.push(exprs);
                    }
                    InsertSource::Values(rows)
                }
                _ => {
                    // INSERT INTO ... SELECT ...
                    let query_plan = self.plan_query_with_cte(q)?;
                    InsertSource::Query(Box::new(query_plan))
                }
            }
        } else {
            return Err(SqlError::Execution("INSERT missing VALUES".to_string()));
        };

        let schema_cols: Vec<(String, catalog::DataType)> = schema
            .columns
            .iter()
            .map(|c| (c.name.clone(), c.data_type.clone()))
            .collect();
        let returning = self.build_returning(returning_ast, &schema_cols)?;

        // Parse ON CONFLICT clause
        let on_conflict = if let Some(ast::OnInsert::OnConflict(oc)) = on_insert {
            let target = match &oc.conflict_target {
                Some(ast::ConflictTarget::Columns(cols)) => {
                    cols.iter().map(|c| c.value.clone()).collect()
                }
                _ => vec![],
            };
            let action = match &oc.action {
                ast::OnConflictAction::DoNothing => OnConflictAction::DoNothing,
                ast::OnConflictAction::DoUpdate(do_update) => {
                    let assignments = do_update.assignments.iter()
                        .map(|a| {
                            let col_name = a.target.to_string();
                            let expr = self.convert_expr_with_schema(&a.value, Some(&schema_cols))?;
                            Ok((col_name, expr))
                        })
                        .collect::<Result<Vec<_>, SqlError>>()?;
                    OnConflictAction::DoUpdate { assignments }
                }
            };
            Some(OnConflict { target, action })
        } else {
            None
        };

        Ok(LogicalPlan::Insert {
            table_name,
            schema,
            columns: col_names,
            source: source_plan,
            returning,
            on_conflict,
        })
    }

    fn plan_update(
        &self,
        table: &TableWithJoins,
        assignments: &[ast::Assignment],
        selection: &Option<AstExpr>,
        returning_ast: Option<&Vec<SelectItem>>,
        from: Option<&TableWithJoins>,
    ) -> Result<LogicalPlan, SqlError> {
        let table_name = match &table.relation {
            TableFactor::Table { name, .. } => {
                name.0.last().map(|i| i.value.clone()).unwrap_or_default()
            }
            _ => {
                return Err(SqlError::NotImplemented(
                    "complex UPDATE target".to_string(),
                ))
            }
        };

        let schema = self.lookup_table(&table_name)?;
        let schema_cols: Vec<(String, catalog::DataType)> = schema
            .columns
            .iter()
            .map(|c| (c.name.clone(), c.data_type.clone()))
            .collect();

        let asgns: Vec<(String, Expr)> = assignments
            .iter()
            .map(|a| {
                let col_name = a.target.to_string();
                let expr = self.convert_expr_with_schema(&a.value, Some(&schema_cols))?;
                Ok((col_name, expr))
            })
            .collect::<Result<Vec<_>, SqlError>>()?;

        let filter = selection
            .as_ref()
            .map(|e| self.convert_expr_with_schema(e, Some(&schema_cols)))
            .transpose()?;

        let returning = self.build_returning(returning_ast, &schema_cols)?;

        // TODO: UPDATE ... FROM — plan the FROM table and pass as from_plan
        let from_plan = if let Some(from_twj) = from {
            Some(Box::new(self.plan_table_with_joins(from_twj)?))
        } else {
            None
        };

        Ok(LogicalPlan::Update {
            table_name,
            schema,
            assignments: asgns,
            filter,
            returning,
            from_plan,
        })
    }

    fn plan_delete(
        &self,
        table_name: &ObjectName,
        selection: &Option<AstExpr>,
        returning_ast: Option<&Vec<SelectItem>>,
        using: Option<&[TableWithJoins]>,
    ) -> Result<LogicalPlan, SqlError> {
        let name = table_name
            .0
            .last()
            .map(|i| i.value.clone())
            .unwrap_or_default();
        let schema = self.lookup_table(&name)?;
        let schema_cols: Vec<(String, catalog::DataType)> = schema
            .columns
            .iter()
            .map(|c| (c.name.clone(), c.data_type.clone()))
            .collect();

        let filter = selection
            .as_ref()
            .map(|e| self.convert_expr_with_schema(e, Some(&schema_cols)))
            .transpose()?;

        let returning = self.build_returning(returning_ast, &schema_cols)?;

        // TODO: DELETE ... USING — plan the USING tables and pass as using_plan
        let using_plan = if let Some(using_tables) = using {
            if let Some(first) = using_tables.first() {
                Some(Box::new(self.plan_table_with_joins(first)?))
            } else {
                None
            }
        } else {
            None
        };

        Ok(LogicalPlan::Delete {
            table_name: name,
            schema,
            filter,
            returning,
            using_plan,
        })
    }

    fn build_returning(
        &self,
        returning_ast: Option<&Vec<SelectItem>>,
        schema_cols: &[(String, catalog::DataType)],
    ) -> Result<Vec<(String, Expr)>, SqlError> {
        let items = match returning_ast {
            None => return Ok(vec![]),
            Some(items) => items,
        };
        let mut cols = Vec::new();
        for item in items {
            match item {
                SelectItem::Wildcard(_) => {
                    for (name, _) in schema_cols {
                        cols.push((
                            name.clone(),
                            Expr::Column {
                                table: None,
                                name: name.clone(),
                            },
                        ));
                    }
                }
                SelectItem::UnnamedExpr(expr) => {
                    let e = self.convert_expr_with_schema(expr, Some(&schema_cols.to_vec()))?;
                    let name = expr_to_name(expr);
                    cols.push((name, e));
                }
                SelectItem::ExprWithAlias { expr, alias } => {
                    let e = self.convert_expr_with_schema(expr, Some(&schema_cols.to_vec()))?;
                    cols.push((alias.value.clone(), e));
                }
                _ => {
                    return Err(SqlError::NotImplemented(
                        "RETURNING item type".to_string(),
                    ))
                }
            }
        }
        Ok(cols)
    }

    fn plan_create_table(
        &self,
        name: &ObjectName,
        columns: &[AstColumnDef],
        if_not_exists: bool,
        constraints: &[ast::TableConstraint],
    ) -> Result<LogicalPlan, SqlError> {
        // Determine schema name and table name
        let parts: Vec<&str> = name.0.iter().map(|i| i.value.as_str()).collect();
        let (schema_name, table_name) = match parts.len() {
            1 => ("public", parts[0]),
            2 => (parts[0], parts[1]),
            _ => return Err(SqlError::NotImplemented("3-part table name".to_string())),
        };

        let mut col_defs = Vec::new();
        let mut primary_key: Option<String> = None;
        let mut unique_columns: Vec<String> = Vec::new();
        let mut foreign_keys: Vec<catalog::schema::TableForeignKey> = Vec::new();
        let mut check_constraints: Vec<catalog::schema::CheckConstraint> = Vec::new();

        for col in columns {
            // Detect SERIAL / BIGSERIAL by type name
            let type_str = col.data_type.to_string().to_lowercase();
            let is_serial = matches!(type_str.as_str(), "serial" | "bigserial");
            let dtype = if is_serial {
                if type_str == "bigserial" {
                    catalog::DataType::Int8
                } else {
                    catalog::DataType::Int4
                }
            } else {
                self.convert_data_type(&col.data_type)?
            };
            let not_null = col
                .options
                .iter()
                .any(|opt| matches!(&opt.option, ast::ColumnOption::NotNull | ast::ColumnOption::Unique { is_primary: true, .. }));
            let is_pk = col
                .options
                .iter()
                .any(|opt| matches!(&opt.option, ast::ColumnOption::Unique { is_primary: true, .. }));
            let is_unique = col
                .options
                .iter()
                .any(|opt| matches!(&opt.option, ast::ColumnOption::Unique { is_primary: false, .. }));
            // Extract DEFAULT expression
            let default_expr: Option<String> = col.options.iter().find_map(|opt| {
                if let ast::ColumnOption::Default(expr) = &opt.option {
                    Some(expr.to_string())
                } else {
                    None
                }
            });
            let has_default = default_expr.is_some() || is_serial;
            if is_pk {
                primary_key = Some(col.name.value.clone());
                if !unique_columns.contains(&col.name.value) {
                    unique_columns.push(col.name.value.clone());
                }
            } else if is_unique && !unique_columns.contains(&col.name.value) {
                unique_columns.push(col.name.value.clone());
            }

            // Extract column-level FK reference
            let mut col_fk: Option<catalog::schema::ForeignKey> = None;
            for opt in &col.options {
                match &opt.option {
                    ast::ColumnOption::ForeignKey { foreign_table, referred_columns, on_delete, .. } => {
                        let ref_table = foreign_table.0.last()
                            .map(|i| i.value.clone())
                            .unwrap_or_default();
                        let ref_col = referred_columns.first()
                            .map(|i| i.value.clone())
                            .unwrap_or_default();
                        let on_del = match on_delete {
                            Some(ast::ReferentialAction::Cascade) => catalog::schema::FkAction::Cascade,
                            Some(ast::ReferentialAction::SetNull) => catalog::schema::FkAction::SetNull,
                            Some(ast::ReferentialAction::SetDefault) => catalog::schema::FkAction::SetDefault,
                            Some(ast::ReferentialAction::Restrict) => catalog::schema::FkAction::Restrict,
                            _ => catalog::schema::FkAction::NoAction,
                        };
                        // Also register as table-level FK
                        foreign_keys.push(catalog::schema::TableForeignKey {
                            local_col: col.name.value.clone(),
                            ref_table: ref_table.clone(),
                            ref_col: ref_col.clone(),
                            on_delete: match on_delete {
                                Some(ast::ReferentialAction::Cascade) => catalog::schema::FkAction::Cascade,
                                Some(ast::ReferentialAction::SetNull) => catalog::schema::FkAction::SetNull,
                                Some(ast::ReferentialAction::SetDefault) => catalog::schema::FkAction::SetDefault,
                                Some(ast::ReferentialAction::Restrict) => catalog::schema::FkAction::Restrict,
                                _ => catalog::schema::FkAction::NoAction,
                            },
                        });
                        col_fk = Some(catalog::schema::ForeignKey { ref_table, ref_col, on_delete: on_del });
                    }
                    ast::ColumnOption::Check(expr) => {
                        check_constraints.push(catalog::schema::CheckConstraint {
                            name: None,
                            expr: expr.to_string(),
                        });
                    }
                    _ => {}
                }
            }

            col_defs.push(catalog::schema::ColumnDef {
                name: col.name.value.clone(),
                data_type: dtype,
                not_null: not_null || is_pk || is_serial,
                has_default,
                default_expr,
                attnum: 0, // will be set by catalog
                serial: is_serial,
                references: col_fk,
            });
        }

        // Also check table-level constraints
        for constraint in constraints {
            match constraint {
                ast::TableConstraint::PrimaryKey { columns, .. } => {
                    // columns is Vec<Ident> in sqlparser 0.53
                    if let Some(first) = columns.first() {
                        let col_name = first.value.clone();
                        primary_key = Some(col_name.clone());
                        if !unique_columns.contains(&col_name) {
                            unique_columns.push(col_name);
                        }
                    }
                }
                ast::TableConstraint::Unique { columns, .. } => {
                    for col in columns {
                        let col_name = col.value.clone();
                        if !unique_columns.contains(&col_name) {
                            unique_columns.push(col_name);
                        }
                    }
                }
                ast::TableConstraint::ForeignKey { columns, foreign_table, referred_columns, on_delete, .. } => {
                    let ref_table = foreign_table.0.last()
                        .map(|i| i.value.clone())
                        .unwrap_or_default();
                    let ref_col = referred_columns.first()
                        .map(|i| i.value.clone())
                        .unwrap_or_default();
                    for local_col_ident in columns {
                        foreign_keys.push(catalog::schema::TableForeignKey {
                            local_col: local_col_ident.value.clone(),
                            ref_table: ref_table.clone(),
                            ref_col: ref_col.clone(),
                            on_delete: match on_delete {
                                Some(ast::ReferentialAction::Cascade) => catalog::schema::FkAction::Cascade,
                                Some(ast::ReferentialAction::SetNull) => catalog::schema::FkAction::SetNull,
                                Some(ast::ReferentialAction::SetDefault) => catalog::schema::FkAction::SetDefault,
                                Some(ast::ReferentialAction::Restrict) => catalog::schema::FkAction::Restrict,
                                _ => catalog::schema::FkAction::NoAction,
                            },
                        });
                    }
                }
                ast::TableConstraint::Check { name, expr } => {
                    check_constraints.push(catalog::schema::CheckConstraint {
                        name: name.as_ref().map(|n| n.value.clone()),
                        expr: expr.to_string(),
                    });
                }
                _ => {}
            }
        }

        Ok(LogicalPlan::CreateTable {
            schema_name: schema_name.to_string(),
            table_name: table_name.to_string(),
            columns: col_defs,
            if_not_exists,
            primary_key,
            unique_columns,
            foreign_keys,
            check_constraints,
        })
    }

    fn plan_alter_table(
        &self,
        name: &ObjectName,
        operations: &[ast::AlterTableOperation],
    ) -> Result<LogicalPlan, SqlError> {
        let table_name = name.0.last()
            .map(|i| i.value.clone())
            .ok_or_else(|| SqlError::Execution("ALTER TABLE missing table name".to_string()))?;

        if operations.is_empty() {
            return Err(SqlError::Execution("ALTER TABLE: no operations specified".to_string()));
        }

        let operation = match &operations[0] {
            ast::AlterTableOperation::AddColumn { column_def, .. } => {
                let dtype = self.convert_data_type(&column_def.data_type)?;
                let nullable = !column_def.options.iter().any(|o| matches!(&o.option, ast::ColumnOption::NotNull));
                AlterTableOp::AddColumn {
                    name: column_def.name.value.clone(),
                    data_type: format!("{:?}", dtype),
                    nullable,
                }
            }
            ast::AlterTableOperation::DropColumn { column_name, .. } => {
                AlterTableOp::DropColumn {
                    name: column_name.value.clone(),
                }
            }
            ast::AlterTableOperation::RenameColumn { old_column_name, new_column_name } => {
                AlterTableOp::RenameColumn {
                    old_name: old_column_name.value.clone(),
                    new_name: new_column_name.value.clone(),
                }
            }
            ast::AlterTableOperation::RenameTable { table_name: new_name } => {
                AlterTableOp::RenameTable {
                    new_name: new_name.0.last().map(|i| i.value.clone()).unwrap_or_default(),
                }
            }
            other => {
                return Err(SqlError::NotImplemented(format!("ALTER TABLE operation: {:?}", other)));
            }
        };

        Ok(LogicalPlan::AlterTable {
            table_name,
            operation,
        })
    }

    fn plan_drop_table(
        &self,
        names: &[ObjectName],
        if_exists: bool,
    ) -> Result<LogicalPlan, SqlError> {
        if names.is_empty() {
            return Err(SqlError::Execution("DROP TABLE missing name".to_string()));
        }
        let parts: Vec<&str> = names[0].0.iter().map(|i| i.value.as_str()).collect();
        let (schema_name, table_name) = match parts.len() {
            1 => ("public", parts[0]),
            2 => (parts[0], parts[1]),
            _ => return Err(SqlError::NotImplemented("3-part table name".to_string())),
        };

        Ok(LogicalPlan::DropTable {
            schema_name: schema_name.to_string(),
            table_name: table_name.to_string(),
            if_exists,
        })
    }

    fn convert_expr_with_schema(
        &self,
        expr: &AstExpr,
        _schema: Option<&Vec<(String, catalog::DataType)>>,
    ) -> Result<Expr, SqlError> {
        match expr {
            AstExpr::Identifier(ident) => Ok(Expr::Column {
                table: None,
                name: ident.value.clone(),
            }),
            AstExpr::CompoundIdentifier(parts) => match parts.len() {
                1 => Ok(Expr::Column {
                    table: None,
                    name: parts[0].value.clone(),
                }),
                2 => Ok(Expr::Column {
                    table: Some(parts[0].value.clone()),
                    name: parts[1].value.clone(),
                }),
                _ => Err(SqlError::NotImplemented("3-part identifier".to_string())),
            },
            AstExpr::Value(v) => {
                let val = convert_ast_value(v)?;
                Ok(Expr::Literal(val))
            }
            AstExpr::TypedString { data_type, value } => {
                let dtype = self.convert_data_type(data_type)?;
                let val = Value::Text(value.clone()).cast_to(&dtype)?;
                Ok(Expr::Literal(val))
            }
            AstExpr::BinaryOp { left, op, right } => {
                let l = self.convert_expr_with_schema(left, _schema)?;
                let r = self.convert_expr_with_schema(right, _schema)?;
                let plan_op = convert_binary_op(op)?;
                Ok(Expr::BinaryOp {
                    left: Box::new(l),
                    op: plan_op,
                    right: Box::new(r),
                })
            }
            AstExpr::UnaryOp { op, expr } => {
                let e = self.convert_expr_with_schema(expr, _schema)?;
                let plan_op = match op {
                    UnaryOperator::Minus => UnaryOp::Neg,
                    UnaryOperator::Not => UnaryOp::Not,
                    _ => return Err(SqlError::NotImplemented(format!("unary op: {op}"))),
                };
                Ok(Expr::UnaryOp {
                    op: plan_op,
                    expr: Box::new(e),
                })
            }
            AstExpr::IsNull(inner) => {
                let e = self.convert_expr_with_schema(inner, _schema)?;
                Ok(Expr::IsNull(Box::new(e)))
            }
            AstExpr::IsNotNull(inner) => {
                let e = self.convert_expr_with_schema(inner, _schema)?;
                Ok(Expr::IsNotNull(Box::new(e)))
            }
            AstExpr::Cast {
                expr, data_type, ..
            } => {
                let e = self.convert_expr_with_schema(expr, _schema)?;
                let dt = self.convert_data_type(data_type)?;
                Ok(Expr::Cast {
                    expr: Box::new(e),
                    data_type: dt,
                })
            }
            // AstExpr::TryCast does not exist in sqlparser 0.53; TryCast is a CastKind within Cast.
            AstExpr::Nested(inner) => self.convert_expr_with_schema(inner, _schema),
            AstExpr::Function(func) => {
                let func_name = func.name.to_string().to_lowercase();
                // Window functions have an OVER clause — skip them in regular expr conversion;
                // they are handled separately by plan_select_body via window_exprs.
                // When we encounter a window function reference here, emit a placeholder Column ref
                // that will be resolved after the Window node appends the column.
                if func.over.is_some() {
                    // This is a window function call. The output column name is generated
                    // at planning time and added to the row by the Window executor node.
                    // We return a Column reference to the output name.
                    let output_name = window_func_output_name(&func_name, &func.args);
                    return Ok(Expr::Column { table: None, name: output_name });
                }
                match &func.args {
                    FunctionArguments::List(arg_list) => {
                        let args: Vec<Expr> = arg_list
                            .args
                            .iter()
                            .map(|a| match a {
                                ast::FunctionArg::Unnamed(ast::FunctionArgExpr::Expr(e)) => {
                                    self.convert_expr_with_schema(e, _schema)
                                }
                                ast::FunctionArg::Unnamed(ast::FunctionArgExpr::Wildcard) => {
                                    Ok(Expr::Literal(Value::Int4(1))) // COUNT(*) argument
                                }
                                _ => Err(SqlError::NotImplemented("function arg type".to_string())),
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        // Handle COALESCE and NULLIF specially
                        match func_name.as_str() {
                            "coalesce" => Ok(Expr::Coalesce(args)),
                            "nullif" => {
                                if args.len() != 2 {
                                    return Err(SqlError::Execution(
                                        "NULLIF requires exactly 2 arguments".to_string(),
                                    ));
                                }
                                let mut iter = args.into_iter();
                                let a = iter.next().unwrap();
                                let b = iter.next().unwrap();
                                Ok(Expr::NullIf(Box::new(a), Box::new(b)))
                            }
                            _ => Ok(Expr::FunctionCall {
                                name: func_name,
                                args,
                            }),
                        }
                    }
                    FunctionArguments::None => Ok(Expr::FunctionCall {
                        name: func_name,
                        args: vec![],
                    }),
                    _ => Err(SqlError::NotImplemented(
                        "function argument style".to_string(),
                    )),
                }
            }
            AstExpr::Between {
                expr,
                negated,
                low,
                high,
            } => {
                // expr BETWEEN low AND high  =>  expr >= low AND expr <= high
                let e = self.convert_expr_with_schema(expr, _schema)?;
                let lo = self.convert_expr_with_schema(low, _schema)?;
                let hi = self.convert_expr_with_schema(high, _schema)?;
                let ge = Expr::BinaryOp {
                    left: Box::new(e.clone()),
                    op: BinaryOp::Ge,
                    right: Box::new(lo),
                };
                let le = Expr::BinaryOp {
                    left: Box::new(e),
                    op: BinaryOp::Le,
                    right: Box::new(hi),
                };
                let and = Expr::BinaryOp {
                    left: Box::new(ge),
                    op: BinaryOp::And,
                    right: Box::new(le),
                };
                if *negated {
                    Ok(Expr::UnaryOp {
                        op: UnaryOp::Not,
                        expr: Box::new(and),
                    })
                } else {
                    Ok(and)
                }
            }
            AstExpr::Like {
                expr,
                pattern,
                negated,
                ..
            } => {
                // Simple LIKE: convert to FunctionCall "like"
                let e = self.convert_expr_with_schema(expr, _schema)?;
                let p = self.convert_expr_with_schema(pattern, _schema)?;
                let like_expr = Expr::FunctionCall {
                    name: "like".to_string(),
                    args: vec![e, p],
                };
                if *negated {
                    Ok(Expr::UnaryOp {
                        op: UnaryOp::Not,
                        expr: Box::new(like_expr),
                    })
                } else {
                    Ok(like_expr)
                }
            }
            AstExpr::ILike {
                expr,
                pattern,
                negated,
                ..
            } => {
                // ILIKE: case-insensitive LIKE — convert to FunctionCall "ilike"
                let e = self.convert_expr_with_schema(expr, _schema)?;
                let p = self.convert_expr_with_schema(pattern, _schema)?;
                let ilike_expr = Expr::FunctionCall {
                    name: "ilike".to_string(),
                    args: vec![e, p],
                };
                if *negated {
                    Ok(Expr::UnaryOp {
                        op: UnaryOp::Not,
                        expr: Box::new(ilike_expr),
                    })
                } else {
                    Ok(ilike_expr)
                }
            }
            AstExpr::InList {
                expr,
                list,
                negated,
            } => {
                let e = self.convert_expr_with_schema(expr, _schema)?;
                if list.is_empty() {
                    return Ok(Expr::Literal(Value::Bool(*negated)));
                }
                let first = self.convert_expr_with_schema(&list[0], _schema)?;
                let mut result = Expr::BinaryOp {
                    left: Box::new(e.clone()),
                    op: BinaryOp::Eq,
                    right: Box::new(first),
                };
                for item in &list[1..] {
                    let item_expr = self.convert_expr_with_schema(item, _schema)?;
                    let eq = Expr::BinaryOp {
                        left: Box::new(e.clone()),
                        op: BinaryOp::Eq,
                        right: Box::new(item_expr),
                    };
                    result = Expr::BinaryOp {
                        left: Box::new(result),
                        op: BinaryOp::Or,
                        right: Box::new(eq),
                    };
                }
                if *negated {
                    // NOT IN: convert each to NotEq and AND them
                    Ok(Expr::UnaryOp {
                        op: UnaryOp::Not,
                        expr: Box::new(result),
                    })
                } else {
                    Ok(result)
                }
            }
            AstExpr::InSubquery {
                expr,
                subquery,
                negated,
            } => {
                let e = self.convert_expr_with_schema(expr, _schema)?;
                let subplan = self.plan_query_with_cte(subquery)?;
                Ok(Expr::InSubquery {
                    expr: Box::new(e),
                    subquery: Box::new(subplan),
                    negated: *negated,
                })
            }
            AstExpr::Exists { subquery, negated } => {
                let subplan = self.plan_query_with_cte(subquery)?;
                Ok(Expr::Exists {
                    subquery: Box::new(subplan),
                    negated: *negated,
                })
            }
            AstExpr::Subquery(subquery) => {
                let subplan = self.plan_query_with_cte(subquery)?;
                Ok(Expr::ScalarSubquery(Box::new(subplan)))
            }
            AstExpr::IsDistinctFrom(left, right) => {
                let l = self.convert_expr_with_schema(left, _schema)?;
                let r = self.convert_expr_with_schema(right, _schema)?;
                Ok(Expr::IsDistinctFrom {
                    left: Box::new(l),
                    right: Box::new(r),
                })
            }
            AstExpr::IsNotDistinctFrom(left, right) => {
                let l = self.convert_expr_with_schema(left, _schema)?;
                let r = self.convert_expr_with_schema(right, _schema)?;
                Ok(Expr::IsNotDistinctFrom {
                    left: Box::new(l),
                    right: Box::new(r),
                })
            }
            AstExpr::Case {
                operand,
                conditions,
                results,
                else_result,
            } => {
                let plan_operand = if let Some(op) = operand {
                    Some(Box::new(self.convert_expr_with_schema(op, _schema)?))
                } else {
                    None
                };
                let when_clauses: Result<Vec<(Expr, Expr)>, SqlError> = conditions
                    .iter()
                    .zip(results.iter())
                    .map(|(cond, res)| {
                        let c = self.convert_expr_with_schema(cond, _schema)?;
                        let r = self.convert_expr_with_schema(res, _schema)?;
                        Ok((c, r))
                    })
                    .collect();
                let else_clause = if let Some(e) = else_result {
                    Some(Box::new(self.convert_expr_with_schema(e, _schema)?))
                } else {
                    None
                };
                Ok(Expr::Case {
                    operand: plan_operand,
                    when_clauses: when_clauses?,
                    else_clause,
                })
            }
            // sqlparser parses CEIL/FLOOR as dedicated AST nodes
            AstExpr::Ceil { expr, field: _ } => {
                let inner = self.convert_expr_with_schema(expr, _schema)?;
                Ok(Expr::FunctionCall { name: "ceil".to_string(), args: vec![inner] })
            }
            AstExpr::Floor { expr, field: _ } => {
                let inner = self.convert_expr_with_schema(expr, _schema)?;
                Ok(Expr::FunctionCall { name: "floor".to_string(), args: vec![inner] })
            }
            // sqlparser parses TRIM as a dedicated AST node
            AstExpr::Trim { expr, trim_what, trim_characters, trim_where } => {
                let inner = self.convert_expr_with_schema(expr, _schema)?;
                let func_name = match trim_where {
                    Some(ast::TrimWhereField::Leading) => "ltrim",
                    Some(ast::TrimWhereField::Trailing) => "rtrim",
                    _ => "trim",
                };
                let mut args = vec![inner];
                if let Some(chars_expr) = trim_what {
                    let chars = self.convert_expr_with_schema(chars_expr, _schema)?;
                    args.push(chars);
                } else if let Some(chars_list) = trim_characters {
                    if let Some(first) = chars_list.first() {
                        let chars = self.convert_expr_with_schema(first, _schema)?;
                        args.push(chars);
                    }
                }
                Ok(Expr::FunctionCall { name: func_name.to_string(), args })
            }
            // sqlparser parses SUBSTRING(...) as a dedicated AST node
            AstExpr::Substring { expr, substring_from, substring_for, special: _ } => {
                let s = self.convert_expr_with_schema(expr, _schema)?;
                let mut args = vec![s];
                if let Some(from_expr) = substring_from {
                    args.push(self.convert_expr_with_schema(from_expr, _schema)?);
                } else {
                    args.push(Expr::Literal(Value::Int4(1)));
                }
                if let Some(for_expr) = substring_for {
                    args.push(self.convert_expr_with_schema(for_expr, _schema)?);
                }
                Ok(Expr::FunctionCall { name: "substring".to_string(), args })
            }
            // current_user is parsed as a Function call in sqlparser 0.53, no special AstExpr variant.
            // IS TRUE / IS FALSE
            AstExpr::IsTrue(inner) => {
                let e = self.convert_expr_with_schema(inner, _schema)?;
                Ok(Expr::BinaryOp {
                    left: Box::new(e),
                    op: BinaryOp::Eq,
                    right: Box::new(Expr::Literal(Value::Bool(true))),
                })
            }
            AstExpr::IsNotTrue(inner) => {
                let e = self.convert_expr_with_schema(inner, _schema)?;
                Ok(Expr::BinaryOp {
                    left: Box::new(e),
                    op: BinaryOp::NotEq,
                    right: Box::new(Expr::Literal(Value::Bool(true))),
                })
            }
            AstExpr::IsFalse(inner) => {
                let e = self.convert_expr_with_schema(inner, _schema)?;
                Ok(Expr::BinaryOp {
                    left: Box::new(e),
                    op: BinaryOp::Eq,
                    right: Box::new(Expr::Literal(Value::Bool(false))),
                })
            }
            AstExpr::IsNotFalse(inner) => {
                let e = self.convert_expr_with_schema(inner, _schema)?;
                Ok(Expr::BinaryOp {
                    left: Box::new(e),
                    op: BinaryOp::NotEq,
                    right: Box::new(Expr::Literal(Value::Bool(false))),
                })
            }
            // IS UNKNOWN ≡ IS NULL for boolean expressions (NULL is the "unknown" truth value)
            AstExpr::IsUnknown(inner) => {
                let e = self.convert_expr_with_schema(inner, _schema)?;
                Ok(Expr::IsNull(Box::new(e)))
            }
            AstExpr::IsNotUnknown(inner) => {
                let e = self.convert_expr_with_schema(inner, _schema)?;
                Ok(Expr::IsNotNull(Box::new(e)))
            }
            // POSITION(needle IN haystack)
            AstExpr::Position { expr, r#in } => {
                let needle = self.convert_expr_with_schema(expr, _schema)?;
                let haystack = self.convert_expr_with_schema(r#in, _schema)?;
                Ok(Expr::FunctionCall {
                    name: "strpos".to_string(),
                    args: vec![haystack, needle],
                })
            }
            _ => Err(SqlError::NotImplemented(format!("expression: {expr}"))),
        }
    }

    /// Convert a HAVING expression, replacing aggregate function calls with references to
    /// their output alias columns in the aggregate result row.
    fn convert_having_expr(
        &self,
        expr: &AstExpr,
        aggregates: &[(String, AggFunc, Expr)],
    ) -> Result<Expr, SqlError> {
        // If this expr is an aggregate function call, look it up in aggregates by name
        if let AstExpr::Function(f) = expr {
            let func_name = f.name.to_string().to_lowercase();
            if matches!(func_name.as_str(), "count" | "sum" | "avg" | "min" | "max") {
                // Try to find which aggregate this corresponds to by alias
                // The alias in aggregates was computed by expr_to_name at plan time
                let having_alias = expr_to_name(expr);
                // Look for a matching aggregate alias
                if let Some((alias, _, _)) = aggregates
                    .iter()
                    .find(|(a, _, _)| a == &having_alias)
                {
                    return Ok(Expr::Column {
                        table: None,
                        name: alias.clone(),
                    });
                }
                // Fallback: use the function name as column reference
                return Ok(Expr::Column {
                    table: None,
                    name: having_alias,
                });
            }
        }

        // Recursively handle non-aggregate expressions
        match expr {
            AstExpr::BinaryOp { left, op, right } => {
                let l = self.convert_having_expr(left, aggregates)?;
                let r = self.convert_having_expr(right, aggregates)?;
                let plan_op = convert_binary_op(op)?;
                Ok(Expr::BinaryOp {
                    left: Box::new(l),
                    op: plan_op,
                    right: Box::new(r),
                })
            }
            AstExpr::UnaryOp { op, expr } => {
                let e = self.convert_having_expr(expr, aggregates)?;
                let plan_op = match op {
                    UnaryOperator::Minus => UnaryOp::Neg,
                    UnaryOperator::Not => UnaryOp::Not,
                    _ => return Err(SqlError::NotImplemented(format!("unary op in HAVING: {op}"))),
                };
                Ok(Expr::UnaryOp {
                    op: plan_op,
                    expr: Box::new(e),
                })
            }
            AstExpr::Nested(inner) => self.convert_having_expr(inner, aggregates),
            // For non-aggregate parts, fall back to the regular converter
            other => self.convert_expr_with_schema(other, None),
        }
    }

    /// Build a flat list of (alias, Expr) for all SELECT items (non-aggregate items only, for validation).
    fn build_select_expr_list(
        &self,
        projection: &[SelectItem],
        plan: &LogicalPlan,
    ) -> Result<Vec<(String, Expr)>, SqlError> {
        let schema = self.extract_schema_from_plan(plan);
        let mut result = Vec::new();
        for item in projection {
            match item {
                SelectItem::UnnamedExpr(expr) => {
                    let name = expr_to_name(expr);
                    let e = self.convert_expr_with_schema(expr, schema.as_ref())?;
                    result.push((name, e));
                }
                SelectItem::ExprWithAlias { expr, alias } => {
                    let e = self.convert_expr_with_schema(expr, schema.as_ref())?;
                    result.push((alias.value.clone(), e));
                }
                SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(_, _) => {
                    // Wildcards don't need validation
                }
            }
        }
        Ok(result)
    }

    fn validate_aggregate_select(
        &self,
        select_exprs: &[(String, Expr)],
        group_by: &[Expr],
        aggregates: &[(String, AggFunc, Expr)],
    ) -> Result<(), SqlError> {
        // Only enforce when there are aggregates
        if aggregates.is_empty() {
            return Ok(());
        }

        use std::collections::HashSet;
        let agg_aliases: HashSet<&str> = aggregates.iter().map(|(a, _, _)| a.as_str()).collect();

        for (alias, expr) in select_exprs {
            if agg_aliases.contains(alias.as_str()) {
                continue; // This is an aggregate output — OK
            }
            if matches!(expr, Expr::Literal(_)) {
                continue; // Constants are always OK
            }
            // Check if this expression appears in group_by
            let in_group_by = group_by.iter().any(|g| exprs_equivalent(g, expr));
            if !in_group_by {
                return Err(SqlError::Execution(format!(
                    "column \"{}\" must appear in the GROUP BY clause or be used in an aggregate function",
                    alias
                )));
            }
        }
        Ok(())
    }

    fn convert_data_type(&self, dtype: &AstDataType) -> Result<catalog::DataType, SqlError> {
        match dtype {
            AstDataType::Boolean => Ok(catalog::DataType::Boolean),
            AstDataType::Bool => Ok(catalog::DataType::Boolean),
            AstDataType::Int(None) | AstDataType::Integer(None) | AstDataType::Int4(None) => {
                Ok(catalog::DataType::Int4)
            }
            AstDataType::Int4(_) | AstDataType::Int(_) | AstDataType::Integer(_) => {
                Ok(catalog::DataType::Int4)
            }
            AstDataType::BigInt(_) | AstDataType::Int8(_) => Ok(catalog::DataType::Int8),
            AstDataType::Float(_)
            | AstDataType::Double
            | AstDataType::DoublePrecision
            | AstDataType::Float8 => Ok(catalog::DataType::Float8),
            AstDataType::Real | AstDataType::Float4 => Ok(catalog::DataType::Float8),
            AstDataType::Text => Ok(catalog::DataType::Text),
            AstDataType::Varchar(len) => {
                let max_len = len
                    .as_ref()
                    .map(|l| {
                        if let ast::CharacterLength::IntegerLength { length, .. } = l {
                            *length as u32
                        } else {
                            u32::MAX
                        }
                    })
                    .unwrap_or(u32::MAX);
                Ok(catalog::DataType::VarChar(max_len))
            }
            AstDataType::Bytea => Ok(catalog::DataType::Bytea),
            AstDataType::Date => Ok(catalog::DataType::Date),
            AstDataType::Timestamp(_, _) => Ok(catalog::DataType::Timestamp),
            AstDataType::Uuid => Ok(catalog::DataType::Uuid),
            AstDataType::Numeric(_) | AstDataType::Decimal(_) => Ok(catalog::DataType::Numeric),
            AstDataType::Custom(name, _) => match name.to_string().to_lowercase().as_str() {
                "int" | "integer" | "int4" => Ok(catalog::DataType::Int4),
                "bigint" | "int8" => Ok(catalog::DataType::Int8),
                "serial" => Ok(catalog::DataType::Int4),
                "bigserial" => Ok(catalog::DataType::Int8),
                "float" | "float8" | "double" | "double precision" => Ok(catalog::DataType::Float8),
                "text" => Ok(catalog::DataType::Text),
                "bool" | "boolean" => Ok(catalog::DataType::Boolean),
                "bytea" => Ok(catalog::DataType::Bytea),
                "date" => Ok(catalog::DataType::Date),
                "timestamp" | "timestamp without time zone" => Ok(catalog::DataType::Timestamp),
                "timestamptz" | "timestamp with time zone" => Ok(catalog::DataType::TimestampTz),
                "numeric" | "decimal" => Ok(catalog::DataType::Numeric),
                "uuid" => Ok(catalog::DataType::Uuid),
                other => Err(SqlError::TypeError(format!("unknown type: {other}"))),
            },
            _ => Err(SqlError::NotImplemented(format!("data type: {dtype}"))),
        }
    }

    /// Parse a SQL expression string (e.g. from a CHECK constraint) into an `Expr`.
    pub fn expr_from_str(&self, expr_str: &str) -> Result<Expr, SqlError> {
        use sqlparser::dialect::PostgreSqlDialect;
        use sqlparser::parser::Parser;
        let dialect = PostgreSqlDialect {};
        let mut parser = Parser::new(&dialect)
            .try_with_sql(expr_str)
            .map_err(|e| SqlError::Parse(e.to_string()))?;
        let ast_expr = parser
            .parse_expr()
            .map_err(|e| SqlError::Parse(e.to_string()))?;
        self.convert_expr_with_schema(&ast_expr, None)
    }

    fn ast_value_to_value(&self, v: &AstValue) -> Result<Value, SqlError> {
        convert_ast_value(v)
    }

    fn plan_copy(
        &self,
        source: &CopySource,
        to: bool,
        target: &CopyTarget,
        options: &[CopyOption],
    ) -> Result<LogicalPlan, SqlError> {
        // Extract file path
        let file_path = match target {
            CopyTarget::File { filename } => filename.clone(),
            CopyTarget::Stdin => "stdin".to_string(),
            CopyTarget::Stdout => "stdout".to_string(),
            _ => return Err(SqlError::NotImplemented("COPY with program target".to_string())),
        };

        // Extract options
        let mut delimiter = ',';
        let mut has_header = false;
        let mut quote = '"';
        for opt in options {
            match opt {
                CopyOption::Delimiter(c) => delimiter = *c,
                CopyOption::Header(b) => has_header = *b,
                CopyOption::Quote(c) => quote = *c,
                CopyOption::Format(ident) => {
                    // FORMAT CSV is implied; FORMAT TEXT would change delimiter
                    if ident.value.to_lowercase() == "text" {
                        delimiter = '\t';
                    }
                }
                _ => {} // Ignore other options
            }
        }

        if to {
            // COPY TO
            let (table_name, query_plan) = match source {
                CopySource::Table { table_name, .. } => {
                    let tbl = table_name.0.last().map(|i| i.value.clone()).unwrap_or_default();
                    (Some(tbl), None)
                }
                CopySource::Query(q) => {
                    let plan = self.plan_query_with_cte(q)?;
                    (None, Some(Box::new(plan)))
                }
            };
            Ok(LogicalPlan::CopyTo {
                table_name,
                query: query_plan,
                file_path,
                delimiter,
                has_header,
            })
        } else {
            // COPY FROM
            let table_name = match source {
                CopySource::Table { table_name, .. } => {
                    table_name.0.last().map(|i| i.value.clone()).unwrap_or_default()
                }
                CopySource::Query(_) => {
                    return Err(SqlError::NotImplemented("COPY FROM with query source".to_string()));
                }
            };
            Ok(LogicalPlan::CopyFrom {
                table_name,
                schema_name: "public".to_string(),
                file_path,
                delimiter,
                has_header,
                quote,
            })
        }
    }
}

fn exprs_equivalent(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (
            Expr::Column {
                name: n1,
                table: t1,
            },
            Expr::Column {
                name: n2,
                table: t2,
            },
        ) => n1 == n2 && t1 == t2,
        (Expr::Literal(v1), Expr::Literal(v2)) => v1 == v2,
        _ => false,
    }
}

fn eval_const_u64(expr: &AstExpr) -> Result<u64, SqlError> {
    match expr {
        AstExpr::Value(AstValue::Number(n, _)) => n
            .parse::<u64>()
            .map_err(|e| SqlError::TypeError(e.to_string())),
        _ => Err(SqlError::NotImplemented(
            "non-literal LIMIT/OFFSET".to_string(),
        )),
    }
}

fn convert_ast_value(v: &AstValue) -> Result<Value, SqlError> {
    match v {
        AstValue::Number(n, _) => {
            // Try i32 first, then i64, then f64
            if let Ok(i) = n.parse::<i32>() {
                Ok(Value::Int4(i))
            } else if let Ok(i) = n.parse::<i64>() {
                Ok(Value::Int8(i))
            } else if let Ok(f) = n.parse::<f64>() {
                Ok(Value::Float8(f))
            } else {
                Err(SqlError::TypeError(format!("cannot parse number: {n}")))
            }
        }
        AstValue::SingleQuotedString(s) | AstValue::DoubleQuotedString(s) => {
            Ok(Value::Text(s.clone()))
        }
        AstValue::Boolean(b) => Ok(Value::Bool(*b)),
        AstValue::Null => Ok(Value::Null),
        _ => Err(SqlError::NotImplemented(format!("value: {v:?}"))),
    }
}

fn convert_binary_op(op: &BinaryOperator) -> Result<BinaryOp, SqlError> {
    match op {
        BinaryOperator::Eq => Ok(BinaryOp::Eq),
        BinaryOperator::NotEq => Ok(BinaryOp::NotEq),
        BinaryOperator::Lt => Ok(BinaryOp::Lt),
        BinaryOperator::LtEq => Ok(BinaryOp::Le),
        BinaryOperator::Gt => Ok(BinaryOp::Gt),
        BinaryOperator::GtEq => Ok(BinaryOp::Ge),
        BinaryOperator::And => Ok(BinaryOp::And),
        BinaryOperator::Or => Ok(BinaryOp::Or),
        BinaryOperator::Plus => Ok(BinaryOp::Add),
        BinaryOperator::Minus => Ok(BinaryOp::Sub),
        BinaryOperator::Multiply => Ok(BinaryOp::Mul),
        BinaryOperator::Divide => Ok(BinaryOp::Div),
        BinaryOperator::StringConcat => Ok(BinaryOp::Concat),
        BinaryOperator::Modulo => Ok(BinaryOp::Mod),
        _ => Err(SqlError::NotImplemented(format!("binary op: {op}"))),
    }
}

fn expr_to_name(expr: &AstExpr) -> String {
    match expr {
        AstExpr::Identifier(i) => i.value.clone(),
        AstExpr::CompoundIdentifier(parts) => {
            parts.last().map(|i| i.value.clone()).unwrap_or_default()
        }
        AstExpr::Value(v) => v.to_string(),
        AstExpr::Function(f) => f.name.to_string().to_lowercase(),
        _ => "?column?".to_string(),
    }
}

fn has_aggregate(expr: &AstExpr) -> bool {
    match expr {
        AstExpr::Function(f) => {
            // Window functions (with OVER clause) are NOT aggregates in this context
            if f.over.is_some() {
                return false;
            }
            let name = f.name.to_string().to_lowercase();
            matches!(name.as_str(), "count" | "sum" | "avg" | "min" | "max"
                | "stddev" | "stddev_samp" | "stddev_pop" | "std"
                | "variance" | "var_samp" | "var_pop"
                | "string_agg" | "bool_and" | "bool_or" | "every" | "array_agg"
            )
        }
        AstExpr::BinaryOp { left, right, .. } => has_aggregate(left) || has_aggregate(right),
        AstExpr::UnaryOp { expr, .. } => has_aggregate(expr),
        AstExpr::Nested(e) => has_aggregate(e),
        _ => false,
    }
}

fn has_window_function(expr: &AstExpr) -> bool {
    match expr {
        AstExpr::Function(f) => f.over.is_some(),
        AstExpr::BinaryOp { left, right, .. } => has_window_function(left) || has_window_function(right),
        AstExpr::UnaryOp { expr, .. } => has_window_function(expr),
        AstExpr::Nested(e) => has_window_function(e),
        _ => false,
    }
}

/// Generate an output column name for a window function (used as a placeholder Column reference).
fn window_func_output_name(func_name: &str, args: &FunctionArguments) -> String {
    // Use func_name as the default output name; if there's an alias it'll be applied at the SelectItem level
    let arg_name = match args {
        FunctionArguments::List(list) => {
            if let Some(ast::FunctionArg::Unnamed(ast::FunctionArgExpr::Expr(e))) = list.args.first() {
                format!("_{}", expr_to_name(e))
            } else {
                String::new()
            }
        }
        _ => String::new(),
    };
    format!("{}{}", func_name, arg_name)
}

fn extract_agg_from_expr(expr: &AstExpr) -> Option<(AggFunc, AstExpr)> {
    match expr {
        AstExpr::Function(f) => {
            let name = f.name.to_string().to_lowercase();
            // Check for COUNT(DISTINCT ...)
            let is_count_distinct = name == "count" && matches!(&f.args,
                FunctionArguments::List(list) if list.duplicate_treatment == Some(ast::DuplicateTreatment::Distinct)
            );
            let func = if is_count_distinct {
                AggFunc::CountDistinct
            } else {
                match name.as_str() {
                    "count" => AggFunc::Count,
                    "sum" => AggFunc::Sum,
                    "avg" => AggFunc::Avg,
                    "min" => AggFunc::Min,
                    "max" => AggFunc::Max,
                    "stddev" | "stddev_samp" | "std" => AggFunc::Stddev,
                    "stddev_pop" => AggFunc::StddevPop,
                    "variance" | "var_samp" => AggFunc::Variance,
                    "var_pop" => AggFunc::VarPop,
                    "bool_and" | "every" => AggFunc::BoolAnd,
                    "bool_or" => AggFunc::BoolOr,
                    "array_agg" => AggFunc::ArrayAgg,
                    "string_agg" => {
                        // Get delimiter from second argument
                        let delimiter = match &f.args {
                            FunctionArguments::List(list) => {
                                match list.args.get(1) {
                                    Some(ast::FunctionArg::Unnamed(ast::FunctionArgExpr::Expr(
                                        AstExpr::Value(AstValue::SingleQuotedString(s))
                                    ))) => s.clone(),
                                    _ => ",".to_string(),
                                }
                            }
                            _ => ",".to_string(),
                        };
                        AggFunc::StringAgg { delimiter }
                    }
                    _ => return None,
                }
            };
            // Get the first argument
            let arg = match &f.args {
                FunctionArguments::List(list) => match list.args.first() {
                    Some(ast::FunctionArg::Unnamed(ast::FunctionArgExpr::Wildcard)) => {
                        AstExpr::Value(AstValue::Number("1".to_string(), false))
                    }
                    Some(ast::FunctionArg::Unnamed(ast::FunctionArgExpr::Expr(e))) => e.clone(),
                    _ => AstExpr::Value(AstValue::Number("1".to_string(), false)),
                },
                _ => AstExpr::Value(AstValue::Number("1".to_string(), false)),
            };
            Some((func, arg))
        }
        _ => None,
    }
}

/// Determine the join algorithm based on the join condition.
/// Use hash join when the condition is a simple equality (or AND of equalities).
fn infer_join_algorithm(condition: &Expr) -> JoinAlgorithm {
    if is_equality_condition(condition) {
        JoinAlgorithm::Hash
    } else {
        JoinAlgorithm::NestedLoop
    }
}

fn is_equality_condition(expr: &Expr) -> bool {
    match expr {
        Expr::BinaryOp { op: BinaryOp::Eq, left, right } => {
            matches!(left.as_ref(), Expr::Column { .. }) && matches!(right.as_ref(), Expr::Column { .. })
        }
        Expr::BinaryOp { op: BinaryOp::And, left, right } => {
            is_equality_condition(left) && is_equality_condition(right)
        }
        _ => false,
    }
}
