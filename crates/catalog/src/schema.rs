use crate::types::DataType;

/// A SQL-language stored function definition.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub schema: String,
    pub params: Vec<(String, DataType)>, // (param_name, type)
    pub return_type: DataType,
    pub body_sql: String, // the SQL body
    pub language: String, // "sql" (we only support "sql" for now)
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub enum FkAction {
    #[default]
    NoAction,
    Cascade,
    SetNull,
    SetDefault,
    Restrict,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ForeignKey {
    pub ref_table: String,
    pub ref_col: String,
    pub on_delete: FkAction,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TableForeignKey {
    pub local_col: String,
    pub ref_table: String,
    pub ref_col: String,
    pub on_delete: FkAction,
    #[serde(default)]
    pub on_update: FkAction,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckConstraint {
    pub name: Option<String>,
    pub expr: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: DataType,
    pub not_null: bool,
    pub has_default: bool,
    pub default_expr: Option<String>,
    pub attnum: u16, // 1-based column number
    /// True if this column is auto-increment (SERIAL / BIGSERIAL / GENERATED ALWAYS AS IDENTITY)
    #[serde(default)]
    pub serial: bool,
    /// Optional FK constraint on this column
    #[serde(default)]
    pub references: Option<ForeignKey>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TableSchema {
    pub oid: u32,
    pub name: String,
    pub namespace_oid: u32,
    pub columns: Vec<ColumnDef>,
    /// Table-level foreign key constraints
    #[serde(default)]
    pub foreign_keys: Vec<TableForeignKey>,
    /// CHECK constraints
    #[serde(default)]
    pub check_constraints: Vec<CheckConstraint>,
}

impl TableSchema {
    pub fn column_by_name(&self, name: &str) -> Option<&ColumnDef> {
        self.columns.iter().find(|c| c.name == name)
    }

    pub fn column_by_attnum(&self, attnum: u16) -> Option<&ColumnDef> {
        self.columns.iter().find(|c| c.attnum == attnum)
    }
}
