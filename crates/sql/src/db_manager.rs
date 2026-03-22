//! DatabaseManager — manages multiple QueryEngine instances (one per database).
//!
//! Layout on disk:
//!   {data_dir}/                        ← default "icedb" database
//!   {data_dir}/databases/{name}/       ← additional databases
//!   {data_dir}/pg_database.json        ← registry of all databases

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::engine::QueryEngine;

// ── Registry ─────────────────────────────────────────────────────────────────

/// One row in pg_database.json.
pub struct DatabaseEntry {
    pub name: String,
    pub owner: String,
}

/// Persistent registry stored in `{data_dir}/pg_database.json`.
pub struct DatabaseRegistry {
    path: PathBuf,
}

impl DatabaseRegistry {
    pub fn new(data_dir: &Path) -> Self {
        Self { path: data_dir.join("pg_database.json") }
    }

    /// Read all registered databases. Returns an empty list if file doesn't exist.
    pub fn list(&self) -> Vec<DatabaseEntry> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(s) => s,
            Err(_) => return vec![DatabaseEntry { name: "icedb".into(), owner: "icedb".into() }],
        };
        // Parse JSON array: [{"name":"...","owner":"..."},...]
        let value: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::json!([]));
        let arr = match value.as_array() {
            Some(a) => a.clone(),
            None => return vec![DatabaseEntry { name: "icedb".into(), owner: "icedb".into() }],
        };
        let mut entries: Vec<DatabaseEntry> = arr.iter().filter_map(|v| {
            let name = v.get("name")?.as_str()?.to_string();
            let owner = v.get("owner").and_then(|o| o.as_str()).unwrap_or("icedb").to_string();
            Some(DatabaseEntry { name, owner })
        }).collect();
        // Always ensure "icedb" is present
        if !entries.iter().any(|e| e.name == "icedb") {
            entries.insert(0, DatabaseEntry { name: "icedb".into(), owner: "icedb".into() });
        }
        entries
    }

    pub fn database_exists(&self, name: &str) -> bool {
        self.list().iter().any(|e| e.name == name)
    }

    pub fn register(&self, name: &str, owner: &str) -> std::io::Result<()> {
        let mut entries = self.list();
        if !entries.iter().any(|e| e.name == name) {
            entries.push(DatabaseEntry { name: name.to_string(), owner: owner.to_string() });
        }
        self.save(&entries)
    }

    pub fn unregister(&self, name: &str) -> std::io::Result<()> {
        let entries: Vec<_> = self.list().into_iter().filter(|e| e.name != name).collect();
        self.save(&entries)
    }

    fn save(&self, entries: &[DatabaseEntry]) -> std::io::Result<()> {
        let arr: serde_json::Value = serde_json::Value::Array(
            entries.iter().map(|e| serde_json::json!({"name": e.name, "owner": e.owner})).collect()
        );
        std::fs::write(&self.path, serde_json::to_string_pretty(&arr).unwrap())
    }
}

// ── Directory helper ──────────────────────────────────────────────────────────

/// Return the data directory for a database.
/// "icedb" → `data_dir` directly (backward-compat).
/// Others → `data_dir/databases/{name}`.
pub fn database_dir(data_dir: &Path, name: &str) -> PathBuf {
    if name == "icedb" {
        data_dir.to_path_buf()
    } else {
        data_dir.join("databases").join(name)
    }
}

// ── DatabaseManager ───────────────────────────────────────────────────────────

/// Manages multiple `QueryEngine` instances, one per database.
/// Engines are created lazily and cached.
pub struct DatabaseManager {
    data_dir: PathBuf,
    engines: Mutex<HashMap<String, Arc<QueryEngine>>>,
}

impl DatabaseManager {
    /// Create a new manager. The "icedb" default engine should already be created by the
    /// caller and registered via `register_engine`.
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            engines: Mutex::new(HashMap::new()),
        }
    }

    /// Register a pre-built engine (used by server startup for the default db).
    pub fn register_engine(&self, name: &str, engine: Arc<QueryEngine>) {
        self.engines.lock().insert(name.to_string(), engine);
    }

    /// Get or open the engine for the named database.
    /// Returns an error if the database does not exist.
    pub fn get_or_open(&self, db_name: &str) -> Result<Arc<QueryEngine>, crate::error::SqlError> {
        // Check cache first
        {
            let cache = self.engines.lock();
            if let Some(e) = cache.get(db_name) {
                return Ok(Arc::clone(e));
            }
        }

        // Validate database exists
        let registry = DatabaseRegistry::new(&self.data_dir);
        if !registry.database_exists(db_name) {
            return Err(crate::error::SqlError::Execution(format!(
                "database \"{}\" does not exist", db_name
            )));
        }

        // Open it
        let dir = database_dir(&self.data_dir, db_name);
        let engine = open_engine(&dir, db_name)?;
        let engine = Arc::new(engine);
        self.engines.lock().insert(db_name.to_string(), Arc::clone(&engine));
        Ok(engine)
    }

    /// Return the root data directory.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

/// Open (or bootstrap) a QueryEngine for the given directory.
pub fn open_engine(dir: &Path, db_name: &str) -> Result<QueryEngine, crate::error::SqlError> {
    std::fs::create_dir_all(dir).map_err(|e| {
        crate::error::SqlError::Execution(format!("cannot create db dir: {}", e))
    })?;
    let wal = Arc::new(
        wal::WalWriter::open(dir).map_err(|e| {
            crate::error::SqlError::Execution(format!("WAL open failed: {}", e))
        })?
    );
    let txn = Arc::new(txn::TransactionManager::new_with_wal_recovery(Arc::clone(&wal), dir));
    let cat = Arc::new(
        catalog::CatalogManager::open(dir, Arc::clone(&wal), Arc::clone(&txn)).map_err(|e| {
            crate::error::SqlError::Execution(format!("catalog open failed: {}", e))
        })?
    );
    Ok(QueryEngine::new_with_db(txn, cat, dir.to_path_buf(), db_name.to_string()))
}
