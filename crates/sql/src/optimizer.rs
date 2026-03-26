use std::sync::Arc;

use catalog::manager::CatalogManager;

use crate::plan::{BinaryOp, Expr, LogicalPlan};
use crate::value::Value;

/// A simple cost-based optimizer that rewrites `Filter(TableScan)` into
/// `IndexScan` when an equality predicate on an indexed column is detected.
pub struct Optimizer {
    catalog: Arc<CatalogManager>,
}

impl Optimizer {
    pub fn new(catalog: Arc<CatalogManager>) -> Self {
        Self { catalog }
    }

    /// Recursively optimize a logical plan.
    pub fn optimize(&self, plan: LogicalPlan) -> LogicalPlan {
        match plan {
            // The key optimization: Filter over TableScan → IndexScan if possible
            LogicalPlan::Filter { input, predicate } => {
                let optimized_input = self.optimize(*input);
                // Try to push the filter into an IndexScan
                if let Some(index_plan) = self.try_index_scan(&optimized_input, &predicate) {
                    index_plan
                } else {
                    LogicalPlan::Filter {
                        input: Box::new(optimized_input),
                        predicate,
                    }
                }
            }

            // Recursively optimize child plans
            LogicalPlan::Project {
                input,
                columns,
                distinct,
            } => LogicalPlan::Project {
                input: Box::new(self.optimize(*input)),
                columns,
                distinct,
            },
            LogicalPlan::Sort { input, keys } => LogicalPlan::Sort {
                input: Box::new(self.optimize(*input)),
                keys,
            },
            LogicalPlan::Limit {
                input,
                limit,
                offset,
            } => LogicalPlan::Limit {
                input: Box::new(self.optimize(*input)),
                limit,
                offset,
            },
            LogicalPlan::Aggregate {
                input,
                group_by,
                aggregates,
                having,
                grouping_sets,
            } => LogicalPlan::Aggregate {
                input: Box::new(self.optimize(*input)),
                group_by,
                aggregates,
                having,
                grouping_sets,
            },
            LogicalPlan::Join {
                left,
                right,
                join_type,
                condition,
                using_columns,
                algorithm,
            } => LogicalPlan::Join {
                left: Box::new(self.optimize(*left)),
                right: Box::new(self.optimize(*right)),
                join_type,
                condition,
                using_columns,
                algorithm,
            },
            LogicalPlan::SetOp {
                op,
                all,
                left,
                right,
            } => LogicalPlan::SetOp {
                op,
                all,
                left: Box::new(self.optimize(*left)),
                right: Box::new(self.optimize(*right)),
            },
            LogicalPlan::Cte { ctes, inner } => {
                let optimized_ctes = ctes
                    .into_iter()
                    .map(|(name, plan)| (name, Box::new(self.optimize(*plan))))
                    .collect();
                LogicalPlan::Cte {
                    ctes: optimized_ctes,
                    inner: Box::new(self.optimize(*inner)),
                }
            }
            LogicalPlan::Window { input, window_exprs } => LogicalPlan::Window {
                input: Box::new(self.optimize(*input)),
                window_exprs,
            },
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
            } => LogicalPlan::RecursiveCte {
                name,
                column_aliases,
                base_query: Box::new(self.optimize(*base_query)),
                recursive_query: Box::new(self.optimize(*recursive_query)),
                search_by_col,
                search_set_col,
                cycle_col,
                cycle_set_col,
                cycle_path_col,
            },

            // All other plans pass through unchanged
            other => other,
        }
    }

    /// If `plan` is a `TableScan` and `predicate` is an equality or selective
    /// range predicate on an indexed column, rewrite to `IndexScan`.
    /// Returns `None` if the rewrite is not applicable.
    fn try_index_scan(&self, plan: &LogicalPlan, predicate: &Expr) -> Option<LogicalPlan> {
        let (table_name, schema, existing_filter) = match plan {
            LogicalPlan::TableScan {
                table_name,
                schema,
                filter,
                ..
            } => (table_name.clone(), schema.clone(), filter.clone()),
            _ => return None,
        };

        let table_oid = self.catalog.get_table_oid("public", &table_name).ok()?;

        // Try equality first (always use index for equality on indexed column)
        if let Some((col_name, eq_val)) = Self::extract_eq_predicate(predicate) {
            if self.catalog.has_index_on_column(table_oid, &col_name) {
                return Some(LogicalPlan::IndexScan {
                    table_name,
                    schema,
                    index_column: col_name,
                    eq_value: Some(eq_val),
                    range_start: None,
                    range_end: None,
                    filter: existing_filter,
                });
            }
        }

        // Try range predicate — only use index when selectivity is below 20%
        if let Some((col_name, range_start, range_end)) =
            Self::extract_range_predicate(predicate)
        {
            if self.catalog.has_index_on_column(table_oid, &col_name) {
                let sel = self.estimate_selectivity(&table_name, predicate);
                if sel < 0.20 {
                    return Some(LogicalPlan::IndexScan {
                        table_name,
                        schema,
                        index_column: col_name,
                        eq_value: None,
                        range_start,
                        range_end,
                        filter: existing_filter,
                    });
                }
            }
        }

        None
    }

    /// Estimate the fraction of rows matching `predicate` using column statistics.
    /// Returns a value in [0.0, 1.0]; lower = more selective.
    fn estimate_selectivity(&self, table_name: &str, predicate: &Expr) -> f64 {
        match predicate {
            Expr::BinaryOp { left, op, right } => {
                let col_name = match (left.as_ref(), right.as_ref()) {
                    (Expr::Column { name, .. }, _) => name.clone(),
                    (_, Expr::Column { name, .. }) => name.clone(),
                    _ => return 0.5,
                };
                let stats = match self.catalog.get_column_stats(table_name, &col_name) {
                    Some(s) => s,
                    None => return 0.5,
                };
                match op {
                    BinaryOp::Eq => {
                        // If value is in MCV list, use its recorded frequency
                        let lit_str = match (left.as_ref(), right.as_ref()) {
                            (_, Expr::Literal(v)) | (Expr::Literal(v), _) => {
                                v.to_string()
                            }
                            _ => {
                                return 1.0 / stats.n_distinct.max(1.0);
                            }
                        };
                        for (val, freq) in stats
                            .most_common_vals
                            .iter()
                            .zip(stats.most_common_freqs.iter())
                        {
                            if val == &lit_str {
                                return *freq;
                            }
                        }
                        // Not in MCV: uniform distribution over remaining distinct values
                        1.0 / stats.n_distinct.max(1.0)
                    }
                    BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                        // Range predicate: assume ~30% selectivity (no histogram yet)
                        0.3
                    }
                    _ => 0.5,
                }
            }
            _ => 0.5,
        }
    }

    /// Extract `(column_name, value)` from `col = lit` or `lit = col` equality.
    fn extract_eq_predicate(expr: &Expr) -> Option<(String, Value)> {
        match expr {
            Expr::BinaryOp {
                left,
                op: BinaryOp::Eq,
                right,
            } => match (left.as_ref(), right.as_ref()) {
                (Expr::Column { name, .. }, Expr::Literal(v)) => {
                    Some((name.clone(), v.clone()))
                }
                (Expr::Literal(v), Expr::Column { name, .. }) => {
                    Some((name.clone(), v.clone()))
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// Extract `(column_name, range_start, range_end)` from a range predicate
    /// (`col > lit`, `col >= lit`, `col < lit`, `col <= lit`, and symmetric forms).
    fn extract_range_predicate(
        expr: &Expr,
    ) -> Option<(String, Option<Value>, Option<Value>)> {
        match expr {
            Expr::BinaryOp { left, op, right } => {
                match (left.as_ref(), op, right.as_ref()) {
                    // col > lit  |  col >= lit  → range_start = lit
                    (
                        Expr::Column { name, .. },
                        BinaryOp::Gt | BinaryOp::Ge,
                        Expr::Literal(v),
                    ) => Some((name.clone(), Some(v.clone()), None)),
                    // col < lit  |  col <= lit  → range_end = lit
                    (
                        Expr::Column { name, .. },
                        BinaryOp::Lt | BinaryOp::Le,
                        Expr::Literal(v),
                    ) => Some((name.clone(), None, Some(v.clone()))),
                    // lit < col  |  lit <= col  → range_start = lit
                    (
                        Expr::Literal(v),
                        BinaryOp::Lt | BinaryOp::Le,
                        Expr::Column { name, .. },
                    ) => Some((name.clone(), Some(v.clone()), None)),
                    // lit > col  |  lit >= col  → range_end = lit
                    (
                        Expr::Literal(v),
                        BinaryOp::Gt | BinaryOp::Ge,
                        Expr::Column { name, .. },
                    ) => Some((name.clone(), None, Some(v.clone()))),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}
