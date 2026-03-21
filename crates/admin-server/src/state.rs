use std::sync::Arc;
use std::time::Instant;

use catalog::manager::CatalogManager;
use sql::QueryEngine;
use wal::writer::WalWriter;

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<QueryEngine>,
    pub catalog: Arc<CatalogManager>,
    pub wal_writer: Arc<WalWriter>,
    pub admin_token: String,
    pub data_dir: String,
    pub port: u16,
    pub start_time: Instant,
}
