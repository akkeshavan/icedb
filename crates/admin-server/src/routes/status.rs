use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};

use crate::auth::RequireAuth;
use crate::state::AppState;

pub async fn get_status(
    _auth: RequireAuth,
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let uptime_secs = state.start_time.elapsed().as_secs();
    let wal_lsn = state.wal_writer.current_lsn();

    let table_count = state
        .catalog
        .list_tables("public")
        .unwrap_or_default()
        .len();

    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": uptime_secs,
        "data_dir": state.data_dir,
        "port": state.port,
        "wal_lsn": wal_lsn,
        "buffer_pool_frames": 256,
        "buffer_pool_dirty": 0,
        "table_count": table_count
    })))
}
