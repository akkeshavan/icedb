use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use crate::auth::RequireAuth;
use crate::state::AppState;

/// GET /api/schemas — list all schemas
pub async fn list_schemas(
    _auth: RequireAuth,
    State(_state): State<AppState>,
) -> Json<Value> {
    // icedb currently uses a single "public" schema
    Json(json!({
        "schemas": ["public", "pg_catalog"]
    }))
}

/// GET /api/schemas/:schema/tables — list tables in a schema
pub async fn list_tables_in_schema(
    _auth: RequireAuth,
    State(state): State<AppState>,
    Path(schema): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.catalog.list_tables(&schema) {
        Ok(tables) => Ok(Json(json!({
            "schema": schema,
            "tables": tables,
            "count": tables.len()
        }))),
        Err(e) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": e.to_string() })),
        )),
    }
}
