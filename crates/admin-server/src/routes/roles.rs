use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::RequireAuth;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct RoleResponse {
    pub oid: u32,
    pub rolname: String,
    pub rolsuper: bool,
    pub rolinherit: bool,
    pub rolcreaterole: bool,
    pub rolcreatedb: bool,
    pub rolcanlogin: bool,
    pub rolbypassrls: bool,
    pub has_password: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateRoleRequest {
    pub name: String,
    pub password: Option<String>,
    pub rolsuper: Option<bool>,
    #[allow(dead_code)]
    pub rolcreatedb: Option<bool>,
    #[allow(dead_code)]
    pub rolcreaterole: Option<bool>,
    pub rolcanlogin: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRoleRequest {
    pub password: Option<String>,
    pub rolsuper: Option<bool>,
    pub rolcanlogin: Option<bool>,
}

/// GET /api/roles — list all roles
pub async fn list_roles(
    _auth: RequireAuth,
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Use the engine to run SELECT from pg_authid via SQL
    // Since scan_all_authid_rows is private, we use get_role via SQL
    // We'll use the engine to query: SELECT rolname FROM pg_authid
    // For now, use the available public API: list known roles via SQL engine
    let result = state.engine.execute("SELECT rolname FROM pg_authid");

    match result {
        Ok(exec_result) => {
            let roles: Vec<Value> = exec_result
                .rows
                .iter()
                .map(|row| {
                    let rolname = row
                        .get_by_idx(0)
                        .map(|v| v.to_string())
                        .unwrap_or_default();
                    // Try to get role details
                    if let Ok(role) = state.catalog.get_role(&rolname) {
                        json!({
                            "oid": role.oid,
                            "rolname": role.rolname,
                            "rolsuper": role.rolsuper,
                            "rolinherit": role.rolinherit,
                            "rolcreaterole": role.rolcreaterole,
                            "rolcreatedb": role.rolcreatedb,
                            "rolcanlogin": role.rolcanlogin,
                            "rolbypassrls": role.rolbypassrls,
                            "has_password": role.rolpassword.is_some()
                        })
                    } else {
                        json!({ "rolname": rolname })
                    }
                })
                .collect();
            Ok(Json(json!({ "roles": roles })))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )),
    }
}

/// GET /api/roles/:name — get a single role
pub async fn get_role(
    _auth: RequireAuth,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match state.catalog.get_role(&name) {
        Ok(role) => Ok(Json(json!({
            "oid": role.oid,
            "rolname": role.rolname,
            "rolsuper": role.rolsuper,
            "rolinherit": role.rolinherit,
            "rolcreaterole": role.rolcreaterole,
            "rolcreatedb": role.rolcreatedb,
            "rolcanlogin": role.rolcanlogin,
            "rolbypassrls": role.rolbypassrls,
            "has_password": role.rolpassword.is_some()
        }))),
        Err(_) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Role '{}' not found", name) })),
        )),
    }
}

/// POST /api/roles — create a role
pub async fn create_role(
    _auth: RequireAuth,
    State(state): State<AppState>,
    Json(body): Json<CreateRoleRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let rolsuper = body.rolsuper.unwrap_or(false);
    let rolcanlogin = body.rolcanlogin.unwrap_or(true);

    // Hash password if provided
    let password_verifier = body.password.as_deref().filter(|p| !p.is_empty()).map(|p| {
        auth::scram::hash_password(p)
    });

    let xid = state.catalog.txn_manager_begin();
    match state
        .catalog
        .create_role(xid, &body.name, rolsuper, rolcanlogin, password_verifier)
    {
        Ok(oid) => {
            let _ = state.catalog.txn_manager_commit(xid);
            Ok((
                StatusCode::CREATED,
                Json(json!({
                    "oid": oid,
                    "rolname": body.name,
                    "rolsuper": rolsuper,
                    "rolcanlogin": rolcanlogin
                })),
            ))
        }
        Err(e) => {
            let _ = state.catalog.txn_manager_abort(xid);
            Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            ))
        }
    }
}

/// PUT /api/roles/:name — update a role (password only for now)
pub async fn update_role(
    _auth: RequireAuth,
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<UpdateRoleRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Verify role exists
    match state.catalog.get_role(&name) {
        Ok(_) => {}
        Err(_) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("Role '{}' not found", name) })),
            ))
        }
    }

    // Build and run ALTER ROLE SQL
    let mut sql_parts: Vec<String> = Vec::new();

    if let Some(pw) = &body.password {
        if !pw.is_empty() {
            let escaped = pw.replace('\'', "''");
            sql_parts.push(format!("PASSWORD '{}'", escaped));
        }
    }
    if let Some(super_flag) = body.rolsuper {
        if super_flag {
            sql_parts.push("SUPERUSER".to_string());
        } else {
            sql_parts.push("NOSUPERUSER".to_string());
        }
    }
    if let Some(login_flag) = body.rolcanlogin {
        if login_flag {
            sql_parts.push("LOGIN".to_string());
        } else {
            sql_parts.push("NOLOGIN".to_string());
        }
    }

    if sql_parts.is_empty() {
        return Ok(Json(json!({ "message": "Nothing to update" })));
    }

    let sql = format!("ALTER ROLE {} {}", name, sql_parts.join(" "));
    match state.engine.execute(&sql) {
        Ok(_) => Ok(Json(json!({ "message": format!("Role '{}' updated", name) }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )),
    }
}

/// DELETE /api/roles/:name — delete a role
pub async fn delete_role(
    _auth: RequireAuth,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    // Check role exists
    match state.catalog.get_role(&name) {
        Ok(_) => {}
        Err(_) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("Role '{}' not found", name) })),
            ))
        }
    }

    // Safety: do not delete the last superuser
    let superuser_count = count_superusers(&state);
    let role = state.catalog.get_role(&name).unwrap();
    if role.rolsuper && superuser_count <= 1 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Cannot delete the last superuser" })),
        ));
    }

    let sql = format!("DROP ROLE {}", name);
    match state.engine.execute(&sql) {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )),
    }
}

fn count_superusers(state: &AppState) -> usize {
    // Query pg_authid for superusers
    match state.engine.execute("SELECT rolname FROM pg_authid WHERE rolsuper = true") {
        Ok(result) => result.rows.len(),
        Err(_) => {
            // Fallback: assume there's at least one (the bootstrap superuser)
            1
        }
    }
}
