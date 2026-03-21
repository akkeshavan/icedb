use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireAuth;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct QueryRequest {
    pub sql: String,
}

/// POST /api/query — execute arbitrary SQL
pub async fn run_query(
    _auth: RequireAuth,
    State(state): State<AppState>,
    Json(body): Json<QueryRequest>,
) -> (StatusCode, Json<Value>) {
    match state.engine.execute(&body.sql) {
        Ok(result) => {
            // Extract column names from the first row's schema (if any)
            let columns: Vec<String> = result
                .rows
                .first()
                .map(|r| r.schema.iter().map(|(name, _)| name.clone()).collect())
                .unwrap_or_default();

            // Convert rows to JSON arrays
            let rows: Vec<Vec<Value>> = result
                .rows
                .iter()
                .map(|row| {
                    row.values
                        .iter()
                        .map(|v| match v {
                            sql::Value::Null => Value::Null,
                            sql::Value::Bool(b) => json!(b),
                            sql::Value::Int4(i) => json!(i),
                            sql::Value::Int8(i) => json!(i),
                            sql::Value::Float8(f) => json!(f),
                            sql::Value::Text(s) => json!(s),
                            sql::Value::Bytes(b) => {
                                json!(format!("\\x{}", b.iter().map(|byte| format!("{byte:02x}")).collect::<String>()))
                            }
                            sql::Value::Date(_)
                            | sql::Value::Timestamp(_)
                            | sql::Value::Numeric(_)
                            | sql::Value::Uuid(_) => {
                                json!(v.to_string())
                            }
                        })
                        .collect()
                })
                .collect();

            (
                StatusCode::OK,
                Json(json!({
                    "columns": columns,
                    "rows": rows,
                    "rows_affected": result.rows_affected,
                    "command": result.command,
                    "error": null
                })),
            )
        }
        Err(e) => (
            StatusCode::OK, // Return 200 with error field to match API spec
            Json(json!({
                "columns": [],
                "rows": [],
                "rows_affected": 0,
                "command": "",
                "error": e.to_string()
            })),
        ),
    }
}
