use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};

use crate::state::AppState;

/// Extractor that verifies the `Authorization: Bearer <token>` header.
/// Returns 401 if the header is missing or the token does not match.
pub struct RequireAuth;

#[axum::async_trait]
impl FromRequestParts<AppState> for RequireAuth {
    type Rejection = (StatusCode, axum::Json<serde_json::Value>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok());

        let token = auth_header.and_then(|v| v.strip_prefix("Bearer "));

        match token {
            Some(t) if t == state.admin_token => Ok(RequireAuth),
            _ => Err((
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({
                    "error": "Unauthorized: missing or invalid admin token"
                })),
            )),
        }
    }
}
