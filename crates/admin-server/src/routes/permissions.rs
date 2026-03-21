use axum::Json;
use serde_json::{json, Value};

use crate::auth::RequireAuth;

/// GET /api/permissions — stub endpoint
pub async fn get_permissions(_auth: RequireAuth) -> Json<Value> {
    Json(json!({
        "note": "Table-level ACLs not yet implemented in the engine. Role-level privileges are managed via the Roles API."
    }))
}
