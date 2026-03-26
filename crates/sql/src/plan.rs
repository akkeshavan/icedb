use crate::value::Value;
use catalog::schema::TableSchema;

#[derive(Debug, Clone)]
pub enum Expr {
    Column {
        table: Option<String>,
        name: String,
    },
    Literal(Value),
    BinaryOp {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    IsNull(Box<Expr>),
    IsNotNull(Box<Expr>),
    IsDistinctFrom {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    IsNotDistinctFrom {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Cast {
        expr: Box<Expr>,
        data_type: catalog::DataType,
    },
    FunctionCall {
        name: String,
        args: Vec<Expr>,
    },
    /// IN (subquery) — evaluated at runtime by executing the subplan
    InSubquery {
        expr: Box<Expr>,
        subquery: Box<LogicalPlan>,
        negated: bool,
    },
    /// EXISTS (subquery)
    Exists {
        subquery: Box<LogicalPlan>,
        negated: bool,
    },
    /// Scalar subquery — returns a single value
    ScalarSubquery(Box<LogicalPlan>),
    /// CASE WHEN ... THEN ... [ELSE ...] END
    Case {
        /// For simple CASE: `CASE expr WHEN val THEN result END`
        operand: Option<Box<Expr>>,
        /// (condition/value, result) pairs
        when_clauses: Vec<(Expr, Expr)>,
        else_clause: Option<Box<Expr>>,
    },
    /// COALESCE(expr, ...) — first non-NULL argument
    Coalesce(Vec<Expr>),
    /// NULLIF(expr1, expr2) — NULL if expr1 = expr2, else expr1
    NullIf(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryOp {
    Eq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Add,
    Sub,
    Mul,
    Div,
    Concat,
    Mod,
    JsonGet,     // -> (returns JSON)
    JsonGetText, // ->> (returns text)
}

#[derive(Debug, Clone)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub struct SortKey {
    pub expr: Expr,
    pub ascending: bool,
    pub nulls_first: bool,
}

#[derive(Debug, Clone)]
pub enum SetOperation {
    Union,
    Intersect,
    Except,
}

#[derive(Debug, Clone)]
pub enum LogicalPlan {
    TableScan {
        table_name: String,
        alias: Option<String>,
        schema: TableSchema,
        filter: Option<Expr>,
    },
    Filter {
        input: Box<LogicalPlan>,
        predicate: Expr,
    },
    Project {
        input: Box<LogicalPlan>,
        columns: Vec<(String, Expr)>,
        distinct: bool,
    },
    Join {
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
        join_type: JoinType,
        condition: Expr,
        using_columns: Vec<String>,
        algorithm: JoinAlgorithm,
    },
    Aggregate {
        input: Box<LogicalPlan>,
        group_by: Vec<Expr>,
        aggregates: Vec<(String, AggFunc, Expr)>,
        having: Option<Expr>,
    },
    Sort {
        input: Box<LogicalPlan>,
        keys: Vec<SortKey>,
    },
    Limit {
        input: Box<LogicalPlan>,
        limit: u64,
        offset: u64,
    },
    /// Set operations: UNION / INTERSECT / EXCEPT
    SetOp {
        op: SetOperation,
        all: bool,
        left: Box<LogicalPlan>,
        right: Box<LogicalPlan>,
    },
    /// CTE wrapper — the cte_name can be referenced as a table in the inner plan
    Cte {
        ctes: Vec<(String, Box<LogicalPlan>)>,
        inner: Box<LogicalPlan>,
    },
    /// In-memory table produced by materializing a CTE or subquery
    Values {
        schema: Vec<(String, catalog::DataType)>,
        rows: Vec<Vec<Value>>,
    },
    // DML
    Insert {
        table_name: String,
        schema: TableSchema,
        columns: Vec<String>,
        source: InsertSource,
        returning: Vec<(String, Expr)>,
        on_conflict: Option<OnConflict>,
    },
    Update {
        table_name: String,
        schema: TableSchema,
        assignments: Vec<(String, Expr)>,
        filter: Option<Expr>,
        returning: Vec<(String, Expr)>,
        /// Optional FROM clause for UPDATE ... FROM ... WHERE ...
        from_plan: Option<Box<LogicalPlan>>,
    },
    Delete {
        table_name: String,
        schema: TableSchema,
        filter: Option<Expr>,
        returning: Vec<(String, Expr)>,
        /// Optional USING clause for DELETE ... USING ...
        using_plan: Option<Box<LogicalPlan>>,
    },
    /// Window functions (ROW_NUMBER, RANK, etc.)
    Window {
        input: Box<LogicalPlan>,
        window_exprs: Vec<WindowExpr>,
    },
    /// Recursive CTE (WITH RECURSIVE)
    RecursiveCte {
        name: String,
        /// Optional column aliases declared in the CTE signature, e.g. `series(n)`.
        /// When non-empty, columns of the base-case result are renamed positionally.
        column_aliases: Vec<String>,
        base_query: Box<LogicalPlan>,
        recursive_query: Box<LogicalPlan>,
        /// SEARCH DEPTH FIRST BY <col> SET <order_col>
        search_by_col: Option<String>,
        search_set_col: Option<String>,
        /// CYCLE <col> SET <cycle_col> USING <path_col>
        cycle_col: Option<String>,
        cycle_set_col: Option<String>,
        cycle_path_col: Option<String>,
    },
    // DDL
    CreateTable {
        schema_name: String,
        table_name: String,
        columns: Vec<catalog::schema::ColumnDef>,
        if_not_exists: bool,
        primary_key: Option<String>,
        unique_columns: Vec<String>,
        foreign_keys: Vec<catalog::schema::TableForeignKey>,
        check_constraints: Vec<catalog::schema::CheckConstraint>,
    },
    DropTable {
        schema_name: String,
        table_name: String,
        if_exists: bool,
    },
    CreateRole {
        rolname: String,
        rolsuper: bool,
        rolcanlogin: bool,
        password: Option<String>,
    },
    CreateIndex {
        schema_name: String,
        table_name: String,
        column_name: String,
        index_name: Option<String>,
    },
    AlterTable {
        table_name: String,
        operation: AlterTableOp,
    },
    IndexScan {
        table_name: String,
        schema: TableSchema,
        index_column: String,
        eq_value: Option<crate::value::Value>,
        range_start: Option<crate::value::Value>,
        range_end: Option<crate::value::Value>,
        filter: Option<Expr>,
    },
    /// Transaction control statements — handled at the engine level.
    /// In embedded/auto-commit mode these are accepted as no-ops so that
    /// tutorial SQL runs without error.
    TransactionControl {
        kind: TransactionControlKind,
    },
    /// GRANT privileges ON table TO role
    Grant {
        table_name: String,
        schema: String,
        grantee: String,
        privileges: Vec<String>, // "SELECT", "INSERT", "UPDATE", "DELETE", "ALL"
        /// Column names for column-level grants, e.g. `GRANT SELECT (col1, col2) ON t TO r`
        columns: Vec<String>,
    },
    /// REVOKE privileges ON table FROM role
    Revoke {
        table_name: String,
        schema: String,
        grantee: String,
        privileges: Vec<String>,
        /// Column names for column-level revokes
        columns: Vec<String>,
    },
    /// VACUUM [ANALYZE] [table]
    Vacuum {
        table_name: Option<String>,
        schema: String,
        analyze: bool,
    },
    /// Virtual scan of information_schema or pg_catalog tables
    SystemCatalogScan {
        catalog_name: String,   // "information_schema" or "pg_catalog"
        table_name: String,     // "tables", "columns", "pg_tables", etc.
        filter: Option<Expr>,
    },
    /// COPY table FROM file
    CopyFrom {
        table_name: String,
        schema_name: String,
        file_path: String,
        delimiter: char,
        has_header: bool,
        quote: char,
    },
    /// COPY table/query TO file
    CopyTo {
        table_name: Option<String>,
        query: Option<Box<LogicalPlan>>,
        file_path: String,
        delimiter: char,
        has_header: bool,
    },
    /// PREPARE name AS sql
    Prepare {
        name: String,
        sql: String,
    },
    /// EXECUTE name(params)
    ExecutePrepared {
        name: String,
        params: Vec<Value>,
    },
    /// DEALLOCATE name (or "ALL"/"*" for all)
    Deallocate {
        name: String,
    },
    /// CREATE FUNCTION name(params) RETURNS type LANGUAGE SQL AS $$ body $$
    CreateFunction {
        schema: String,
        name: String,
        params: Vec<(String, catalog::DataType)>,
        return_type: catalog::DataType,
        body_sql: String,
        language: String,
    },
    /// DROP FUNCTION [IF EXISTS] name
    DropFunction {
        schema: String,
        name: String,
        if_exists: bool,
    },
    /// GENERATE_SERIES(start, stop[, step]) table function
    GenerateSeries {
        start: Expr,
        stop: Expr,
        step: Expr,
    },
    /// LATERAL join — subquery may reference columns from the outer plan
    Lateral {
        outer: Box<LogicalPlan>,
        subquery: Box<LogicalPlan>,
        alias: String,
    },
    /// No-op for SET, SHOW, and other accepted-but-ignored statements
    NoOp {
        command: String,
    },
    /// CREATE DATABASE [IF NOT EXISTS] name
    CreateDatabase {
        name: String,
        if_not_exists: bool,
    },
    /// DROP DATABASE [IF EXISTS] name
    DropDatabase {
        name: String,
        if_exists: bool,
    },
    /// CREATE SCHEMA [IF NOT EXISTS] name
    CreateSchema {
        name: String,
        if_not_exists: bool,
    },
    /// EXPLAIN [ANALYZE] <statement>
    Explain {
        analyze: bool,
        plan: Box<LogicalPlan>,
    },
}

#[derive(Debug, Clone)]
pub enum TransactionControlKind {
    Begin,
    Commit,
    Rollback,
    /// SAVEPOINT <name> — session-level; executor treats as acknowledged no-op
    Savepoint,
    /// ROLLBACK TO [SAVEPOINT] <name> — session-level; executor treats as acknowledged no-op
    RollbackToSavepoint,
    /// RELEASE [SAVEPOINT] <name> — session-level; executor treats as acknowledged no-op
    ReleaseSavepoint,
}

#[derive(Debug, Clone)]
pub enum InsertSource {
    Values(Vec<Vec<Expr>>),
    Query(Box<LogicalPlan>),
}

#[derive(Debug, Clone)]
pub struct OnConflict {
    pub target: Vec<String>,
    pub action: OnConflictAction,
}

#[derive(Debug, Clone)]
pub enum OnConflictAction {
    DoNothing,
    DoUpdate { assignments: Vec<(String, Expr)> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinAlgorithm {
    NestedLoop,
    Hash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinType {
    Inner,
    Left,
    Right,
    Full,
    Cross,
}

#[derive(Debug, Clone)]
pub struct WindowExpr {
    pub output_name: String,
    pub function: WindowFunction,
    pub partition_by: Vec<Expr>,
    pub order_by: Vec<SortKey>,
}

#[derive(Debug, Clone)]
pub enum WindowFunction {
    RowNumber,
    Rank,
    DenseRank,
    Sum(Box<Expr>),
    Avg(Box<Expr>),
    Min(Box<Expr>),
    Max(Box<Expr>),
    Count(Box<Expr>),
    Lead { expr: Box<Expr>, offset: i64, default: Option<Box<Expr>> },
    Lag  { expr: Box<Expr>, offset: i64, default: Option<Box<Expr>> },
    FirstValue(Box<Expr>),
    LastValue(Box<Expr>),
    NthValue { expr: Box<Expr>, n: i64 },
    CumeDist,
    PercentRank,
    Ntile(Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum AlterTableOp {
    AddColumn { name: String, data_type: String, nullable: bool },
    DropColumn { name: String },
    RenameColumn { old_name: String, new_name: String },
    RenameTable { new_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggFunc {
    Count,
    CountDistinct,
    Sum,
    Avg,
    Min,
    Max,
    Stddev,
    StddevPop,
    Variance,
    VarPop,
    StringAgg { delimiter: String },
    BoolAnd,
    BoolOr,
    ArrayAgg,
}
