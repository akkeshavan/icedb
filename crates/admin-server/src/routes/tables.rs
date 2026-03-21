use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use crate::auth::RequireAuth;
use crate::state::AppState;

/// GET /api/tables/:schema/:table — get table schema (columns, types)
pub async fn get_table_schema(
    _auth: RequireAuth,
    State(state): State<AppState>,
    Path((schema, table)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.catalog.get_table(&schema, &table) {
        Ok(table_schema) => {
            let columns: Vec<Value> = table_schema
                .columns
                .iter()
                .map(|col| {
                    json!({
                        "name": col.name,
                        "type": format!("{:?}", col.data_type),
                        "not_null": col.not_null,
                        "has_default": col.has_default,
                        "attnum": col.attnum
                    })
                })
                .collect();

            Ok(Json(json!({
                "oid": table_schema.oid,
                "name": table_schema.name,
                "schema": schema,
                "columns": columns
            })))
        }
        Err(_) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Table '{}.{}' not found", schema, table) })),
        )),
    }
}

/// GET /api/tables/:schema/:table/indexes — list indexes
pub async fn get_table_indexes(
    _auth: RequireAuth,
    State(state): State<AppState>,
    Path((schema, table)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let table_oid = match state.catalog.get_table_oid(&schema, &table) {
        Ok(oid) => oid,
        Err(_) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("Table '{}.{}' not found", schema, table) })),
            ))
        }
    };

    // Try to find indexes by checking common column names
    let table_schema = state.catalog.get_table(&schema, &table).ok();
    let mut indexes: Vec<Value> = Vec::new();

    if let Some(ts) = table_schema {
        for col in &ts.columns {
            if let Some(path) = state.catalog.get_index_path(table_oid, &col.name) {
                indexes.push(json!({
                    "column": col.name,
                    "type": "btree",
                    "path": path.display().to_string()
                }));
            }
        }
    }

    Ok(Json(json!({
        "table": table,
        "schema": schema,
        "indexes": indexes
    })))
}

/// GET /api/tables/:schema/:table/rowcount — get row count
pub async fn get_table_rowcount(
    _auth: RequireAuth,
    State(state): State<AppState>,
    Path((schema, table)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let sql = if schema == "public" {
        format!("SELECT COUNT(*) FROM {}", table)
    } else {
        format!("SELECT COUNT(*) FROM {}.{}", schema, table)
    };

    match state.engine.execute(&sql) {
        Ok(result) => {
            let count = result
                .rows
                .first()
                .and_then(|r| r.get_by_idx(0))
                .map(|v| v.to_string())
                .unwrap_or_else(|| "0".to_string());

            Ok(Json(json!({
                "table": table,
                "schema": schema,
                "row_count": count
            })))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )),
    }
}

/// DELETE /api/tables/:schema/:table — drop a table
pub async fn drop_table(
    _auth: RequireAuth,
    State(state): State<AppState>,
    Path((schema, table)): Path<(String, String)>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    let sql = if schema == "public" {
        format!("DROP TABLE IF EXISTS {}", table)
    } else {
        format!("DROP TABLE IF EXISTS {}.{}", schema, table)
    };

    match state.engine.execute(&sql) {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )),
    }
}

/// POST /api/tables/:schema/:table/truncate — truncate a table
pub async fn truncate_table(
    _auth: RequireAuth,
    State(state): State<AppState>,
    Path((schema, table)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let sql = if schema == "public" {
        format!("DELETE FROM {}", table)
    } else {
        format!("DELETE FROM {}.{}", schema, table)
    };

    match state.engine.execute(&sql) {
        Ok(result) => Ok(Json(json!({
            "message": format!("Table '{}.{}' truncated", schema, table),
            "rows_deleted": result.rows_affected
        }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )),
    }
}
