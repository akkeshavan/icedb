use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use storage::heap::HeapFile;
use storage::tuple::TUPLE_HEADER_SIZE;
use txn::manager::TransactionManager;
use txn::transaction::IsolationLevel;
use txn::xid::Xid;
use wal::writer::WalWriter;

use crate::error::CatalogError;
use crate::oids::*;
use crate::pg_attribute::PgAttributeRow;
use crate::pg_authid::PgAuthidRow;
use crate::pg_class::PgClassRow;
use crate::pg_namespace::PgNamespaceRow;
use crate::schema::{ColumnDef, TableSchema};
use crate::types::DataType;

// ── ACL (Access Control List) types ──────────────────────────────────────────

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TableAcl {
    pub grants: Vec<AclEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AclEntry {
    pub grantee: String, // role name, or "*" for PUBLIC
    pub privileges: Vec<AclPrivilege>,
    /// Column-level grants: column_name → list of privileges
    #[serde(default)]
    pub col_grants: HashMap<String, Vec<AclPrivilege>>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AclPrivilege {
    Select,
    Insert,
    Update,
    Delete,
    All,
}

/// Unique/Primary Key constraint information for a table.
#[derive(Debug, Clone)]
pub struct UniqueConstraints {
    pub unique_columns: Vec<String>,
    pub primary_key: Option<String>,
}

/// Per-column statistics gathered by ANALYZE.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ColumnStats {
    pub null_frac: f64,           // fraction of rows that are NULL (0.0–1.0)
    pub n_distinct: f64,          // number of distinct non-null values
    pub most_common_vals: Vec<String>, // string representations of top values
    pub most_common_freqs: Vec<f64>,   // parallel frequencies for mcv list
}

pub struct CatalogManager {
    data_dir: PathBuf,
    #[allow(dead_code)]
    wal_writer: Arc<WalWriter>,
    txn_manager: Arc<TransactionManager>,
    /// Heap files for the catalog tables, keyed by catalog name
    catalog_heaps: HashMap<&'static str, Mutex<HeapFile>>,
    /// In-memory cache of table schemas: OID → TableSchema
    schema_cache: RwLock<HashMap<u32, TableSchema>>,
    /// In-memory cache of (namespace_oid, table_name) → table_oid
    name_cache: RwLock<HashMap<(u32, String), u32>>,
    /// In-memory cache of namespace name → namespace OID
    ns_cache: RwLock<HashMap<String, u32>>,
    /// OID counter for user objects
    next_oid: AtomicU32,
    /// In-memory index registry: (table_oid, column_name) → index file path
    index_registry: RwLock<HashMap<(u32, String), PathBuf>>,
    /// In-memory unique/pk constraint registry: table_oid → UniqueConstraints
    unique_constraints: RwLock<HashMap<u32, UniqueConstraints>>,
    /// In-memory sequence counters: "schema_table_col" → next_value
    sequences: Mutex<HashMap<String, i64>>,
    /// Foreign key constraints: table_oid → list of FK constraints
    fk_registry: RwLock<HashMap<u32, Vec<crate::schema::TableForeignKey>>>,
    /// Check constraints: table_oid → list of check constraints
    check_registry: RwLock<HashMap<u32, Vec<crate::schema::CheckConstraint>>>,
    /// Table ACLs: "schema.table" → TableAcl
    table_acls: Mutex<HashMap<String, TableAcl>>,
    /// Pub/sub channel subscribers: channel_name → list of senders
    notify_subscribers: Mutex<HashMap<String, Vec<std::sync::mpsc::Sender<String>>>>,
    /// Autovacuum tracking: table_name → last vacuum instant
    last_vacuum: Mutex<HashMap<String, std::time::Instant>>,
    /// Function registry: "schema.name" → FunctionDef
    functions: Mutex<HashMap<String, crate::schema::FunctionDef>>,
    /// Statistics gathered by ANALYZE: table_name → column_name → ColumnStats
    table_stats: Mutex<HashMap<String, HashMap<String, ColumnStats>>>,
}

impl CatalogManager {
    /// Open (or create) the catalog at `data_dir`.
    pub fn open(
        data_dir: &Path,
        wal_writer: Arc<WalWriter>,
        txn_manager: Arc<TransactionManager>,
    ) -> Result<Self, CatalogError> {
        std::fs::create_dir_all(data_dir)?;

        let pg_class_path = data_dir.join("catalog_pg_class.heap");
        let needs_bootstrap = !pg_class_path.exists();

        // Open all catalog heap files
        let mut catalog_heaps: HashMap<&'static str, Mutex<HeapFile>> = HashMap::new();
        for name in &["pg_class", "pg_attribute", "pg_authid", "pg_namespace"] {
            let path = data_dir.join(format!("catalog_{name}.heap"));
            let heap = HeapFile::open(&path)?;
            catalog_heaps.insert(name, Mutex::new(heap));
        }

        // Load sequences from disk
        let sequences = Self::load_sequences_from_disk(data_dir)?;

        // Load ACLs from disk
        let table_acls = Self::load_acls_from_disk(data_dir)?;

        let mgr = CatalogManager {
            data_dir: data_dir.to_path_buf(),
            wal_writer,
            txn_manager,
            catalog_heaps,
            schema_cache: RwLock::new(HashMap::new()),
            name_cache: RwLock::new(HashMap::new()),
            ns_cache: RwLock::new(HashMap::new()),
            next_oid: AtomicU32::new(USER_OID_START),
            index_registry: RwLock::new(HashMap::new()),
            unique_constraints: RwLock::new(HashMap::new()),
            sequences: Mutex::new(sequences),
            fk_registry: RwLock::new(HashMap::new()),
            check_registry: RwLock::new(HashMap::new()),
            table_acls: Mutex::new(table_acls),
            notify_subscribers: Mutex::new(HashMap::new()),
            last_vacuum: Mutex::new(HashMap::new()),
            functions: Mutex::new(HashMap::new()),
            table_stats: Mutex::new(HashMap::new()),
        };

        // Load persisted statistics
        let stats_dir = data_dir.join("stats");
        let mut loaded_stats: HashMap<String, HashMap<String, ColumnStats>> = HashMap::new();
        if stats_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&stats_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("json") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                if let Ok(stats) = serde_json::from_str::<HashMap<String, ColumnStats>>(&content) {
                                    loaded_stats.insert(stem.to_string(), stats);
                                }
                            }
                        }
                    }
                }
            }
        }
        *mgr.table_stats.lock() = loaded_stats;

        if needs_bootstrap {
            mgr.bootstrap()?;
        } else {
            mgr.load_from_disk()?;
        }

        Ok(mgr)
    }

    /// Bootstrap a fresh catalog from scratch.
    fn bootstrap(&self) -> Result<(), CatalogError> {
        // Use a bootstrap XID
        let boot_xid: Xid = 1;
        // Manually mark XID 1 as committed by beginning a transaction and committing it
        // We use the real txn manager to get a fresh XID and commit it
        let xid = self.txn_manager.begin(IsolationLevel::ReadCommitted);

        // Insert default namespaces
        let public_ns = PgNamespaceRow {
            oid: OID_NS_PUBLIC,
            nspname: "public".to_string(),
            nspowner: OID_ROLE_SUPERUSER,
        };
        let pg_catalog_ns = PgNamespaceRow {
            oid: OID_NS_PG_CATALOG,
            nspname: "pg_catalog".to_string(),
            nspowner: OID_ROLE_SUPERUSER,
        };
        self.insert_namespace_row(xid, &public_ns)?;
        self.insert_namespace_row(xid, &pg_catalog_ns)?;

        // Register system tables in pg_class
        let sys_tables: &[(&str, u32, u16)] = &[
            ("pg_class", OID_PG_CLASS, 8),
            ("pg_attribute", OID_PG_ATTRIBUTE, 8),
            ("pg_authid", OID_PG_AUTHID, 9),
            ("pg_namespace", OID_PG_NAMESPACE, 3),
        ];

        for (name, oid, natts) in sys_tables {
            let row = PgClassRow {
                oid: *oid,
                relname: name.to_string(),
                relnamespace: OID_NS_PG_CATALOG,
                reltype: 0,
                relkind: b'r',
                relnatts: *natts,
                relpages: 0,
                reltuples: 0.0,
            };
            self.insert_pg_class_row(xid, &row)?;
        }

        // Insert pg_class column definitions for each system table
        // pg_class columns
        let pg_class_cols: &[(&str, u32, i32)] = &[
            ("oid", OID_TYPE_INT4, -1),
            ("relname", OID_TYPE_TEXT, -1),
            ("relnamespace", OID_TYPE_INT4, -1),
            ("reltype", OID_TYPE_INT4, -1),
            ("relkind", OID_TYPE_INT4, -1),
            ("relnatts", OID_TYPE_INT4, -1),
            ("relpages", OID_TYPE_INT4, -1),
            ("reltuples", OID_TYPE_FLOAT8, -1),
        ];
        for (i, (name, typid, typmod)) in pg_class_cols.iter().enumerate() {
            let row = PgAttributeRow {
                attrelid: OID_PG_CLASS,
                attname: name.to_string(),
                atttypid: *typid,
                atttypmod: *typmod,
                attnum: (i + 1) as i16,
                attnotnull: true,
                atthasdef: false,
                attisdropped: false,
            };
            self.insert_pg_attribute_row(xid, &row)?;
        }

        // pg_attribute columns
        let pg_attr_cols: &[(&str, u32, i32)] = &[
            ("attrelid", OID_TYPE_INT4, -1),
            ("attname", OID_TYPE_TEXT, -1),
            ("atttypid", OID_TYPE_INT4, -1),
            ("atttypmod", OID_TYPE_INT4, -1),
            ("attnum", OID_TYPE_INT4, -1),
            ("attnotnull", OID_TYPE_BOOL, -1),
            ("atthasdef", OID_TYPE_BOOL, -1),
            ("attisdropped", OID_TYPE_BOOL, -1),
        ];
        for (i, (name, typid, typmod)) in pg_attr_cols.iter().enumerate() {
            let row = PgAttributeRow {
                attrelid: OID_PG_ATTRIBUTE,
                attname: name.to_string(),
                atttypid: *typid,
                atttypmod: *typmod,
                attnum: (i + 1) as i16,
                attnotnull: true,
                atthasdef: false,
                attisdropped: false,
            };
            self.insert_pg_attribute_row(xid, &row)?;
        }

        // pg_authid columns
        let pg_authid_cols: &[(&str, u32, i32)] = &[
            ("oid", OID_TYPE_INT4, -1),
            ("rolname", OID_TYPE_TEXT, -1),
            ("rolsuper", OID_TYPE_BOOL, -1),
            ("rolinherit", OID_TYPE_BOOL, -1),
            ("rolcreaterole", OID_TYPE_BOOL, -1),
            ("rolcreatedb", OID_TYPE_BOOL, -1),
            ("rolcanlogin", OID_TYPE_BOOL, -1),
            ("rolbypassrls", OID_TYPE_BOOL, -1),
            ("rolpassword", OID_TYPE_TEXT, -1),
        ];
        for (i, (name, typid, typmod)) in pg_authid_cols.iter().enumerate() {
            let row = PgAttributeRow {
                attrelid: OID_PG_AUTHID,
                attname: name.to_string(),
                atttypid: *typid,
                atttypmod: *typmod,
                attnum: (i + 1) as i16,
                attnotnull: false,
                atthasdef: false,
                attisdropped: false,
            };
            self.insert_pg_attribute_row(xid, &row)?;
        }

        // pg_namespace columns
        let pg_ns_cols: &[(&str, u32, i32)] = &[
            ("oid", OID_TYPE_INT4, -1),
            ("nspname", OID_TYPE_TEXT, -1),
            ("nspowner", OID_TYPE_INT4, -1),
        ];
        for (i, (name, typid, typmod)) in pg_ns_cols.iter().enumerate() {
            let row = PgAttributeRow {
                attrelid: OID_PG_NAMESPACE,
                attname: name.to_string(),
                atttypid: *typid,
                atttypmod: *typmod,
                attnum: (i + 1) as i16,
                attnotnull: true,
                atthasdef: false,
                attisdropped: false,
            };
            self.insert_pg_attribute_row(xid, &row)?;
        }

        // Insert superuser role
        let superuser = PgAuthidRow {
            oid: OID_ROLE_SUPERUSER,
            rolname: "postgres".to_string(),
            rolsuper: true,
            rolinherit: true,
            rolcreaterole: true,
            rolcreatedb: true,
            rolcanlogin: true,
            rolbypassrls: true,
            rolpassword: None,
        };
        self.insert_pg_authid_row(xid, &superuser)?;

        // Commit the bootstrap transaction
        self.txn_manager.commit(xid)?;

        // Populate caches
        {
            let mut ns_cache = self.ns_cache.write();
            ns_cache.insert("public".to_string(), OID_NS_PUBLIC);
            ns_cache.insert("pg_catalog".to_string(), OID_NS_PG_CATALOG);
        }

        let _ = boot_xid; // suppress unused warning
        Ok(())
    }

    /// Load catalog state from disk into caches.
    fn load_from_disk(&self) -> Result<(), CatalogError> {
        // Load namespaces
        let ns_rows = self.scan_all_namespace_rows()?;
        {
            let mut ns_cache = self.ns_cache.write();
            let mut max_oid = USER_OID_START;
            for row in &ns_rows {
                ns_cache.insert(row.nspname.clone(), row.oid);
                if row.oid >= max_oid {
                    max_oid = row.oid + 1;
                }
            }
        }

        // Load pg_class rows
        let class_rows = self.scan_all_pg_class_rows()?;
        // Load pg_attribute rows
        let attr_rows = self.scan_all_pg_attribute_rows()?;

        // Group attributes by table OID
        let mut attrs_by_table: HashMap<u32, Vec<PgAttributeRow>> = HashMap::new();
        for row in attr_rows {
            attrs_by_table.entry(row.attrelid).or_default().push(row);
        }

        let mut max_oid = USER_OID_START;
        let mut schema_cache = self.schema_cache.write();
        let mut name_cache = self.name_cache.write();

        for class_row in &class_rows {
            if class_row.oid >= max_oid {
                max_oid = class_row.oid + 1;
            }
            // Only cache user tables (not system tables)
            if class_row.oid < USER_OID_START {
                continue;
            }

            let mut columns: Vec<ColumnDef> = Vec::new();
            if let Some(attr_rows) = attrs_by_table.get(&class_row.oid) {
                let mut sorted = attr_rows.clone();
                sorted.sort_by_key(|a| a.attnum);
                for attr in sorted {
                    if attr.attisdropped {
                        continue;
                    }
                    let data_type =
                        DataType::from_oid(attr.atttypid, attr.atttypmod).ok_or_else(|| {
                            CatalogError::CorruptCatalog(format!(
                                "unknown type OID {} for column {}",
                                attr.atttypid, attr.attname
                            ))
                        })?;
                    // Determine serial flag: check if a sequence file exists for this column.
                    // The sequence key uses the schema name, table name, and column name.
                    let schema_name = if class_row.relnamespace == OID_NS_PUBLIC { "public" } else { "pg_catalog" };
                    let seq_key = Self::sequence_key(schema_name, &class_row.relname, &attr.attname);
                    let serial = {
                        let seqs = self.sequences.lock();
                        seqs.contains_key(&seq_key)
                    };
                    columns.push(ColumnDef {
                        name: attr.attname.clone(),
                        data_type,
                        not_null: attr.attnotnull,
                        has_default: attr.atthasdef || serial,
                        default_expr: None,
                        attnum: attr.attnum as u16,
                        serial,
                        references: None,
                    });
                }
            }

            let schema = TableSchema {
                oid: class_row.oid,
                name: class_row.relname.clone(),
                namespace_oid: class_row.relnamespace,
                columns,
                foreign_keys: Vec::new(),
                check_constraints: Vec::new(),
            };

            schema_cache.insert(class_row.oid, schema);
            name_cache.insert(
                (class_row.relnamespace, class_row.relname.clone()),
                class_row.oid,
            );
        }

        self.next_oid.store(max_oid, Ordering::SeqCst);

        Ok(())
    }

    /// Allocate a new unique OID.
    pub fn allocate_oid(&self) -> u32 {
        self.next_oid.fetch_add(1, Ordering::SeqCst)
    }

    fn table_heap_path(&self, oid: u32) -> PathBuf {
        self.data_dir.join(format!("{oid}.heap"))
    }

    // ── Low-level insert helpers ──────────────────────────────────────────────

    fn insert_pg_class_row(&self, xid: Xid, row: &PgClassRow) -> Result<(), CatalogError> {
        let data = row.encode();
        let heap = self.catalog_heaps.get("pg_class").unwrap();
        let mut heap = heap.lock();
        self.txn_manager.insert_tuple(xid, &mut heap, &data)?;
        Ok(())
    }

    fn insert_pg_attribute_row(&self, xid: Xid, row: &PgAttributeRow) -> Result<(), CatalogError> {
        let data = row.encode();
        let heap = self.catalog_heaps.get("pg_attribute").unwrap();
        let mut heap = heap.lock();
        self.txn_manager.insert_tuple(xid, &mut heap, &data)?;
        Ok(())
    }

    fn insert_pg_authid_row(&self, xid: Xid, row: &PgAuthidRow) -> Result<(), CatalogError> {
        let data = row.encode();
        let heap = self.catalog_heaps.get("pg_authid").unwrap();
        let mut heap = heap.lock();
        self.txn_manager.insert_tuple(xid, &mut heap, &data)?;
        Ok(())
    }

    fn insert_namespace_row(&self, xid: Xid, row: &PgNamespaceRow) -> Result<(), CatalogError> {
        let data = row.encode();
        let heap = self.catalog_heaps.get("pg_namespace").unwrap();
        let mut heap = heap.lock();
        self.txn_manager.insert_tuple(xid, &mut heap, &data)?;
        Ok(())
    }

    // ── Low-level scan helpers ────────────────────────────────────────────────

    /// Scan a catalog heap file for all live (non-dead, non-xmax-deleted) tuples.
    ///
    /// Unlike `scan_visible_tuples`, this does NOT use MVCC visibility — it
    /// reads any non-dead slot whose `t_xmax == 0` (no deleter). This is safe
    /// for catalog loading because the catalog only holds committed data (all
    /// catalog writes are committed before the CatalogManager returns to callers).
    ///
    /// Returns `(TID, user_data_bytes)` pairs.
    fn scan_raw_catalog_tuples(
        heap: &mut HeapFile,
    ) -> Result<Vec<(storage::tid::TID, Vec<u8>)>, CatalogError> {
        let num_pages = heap.num_pages();
        let mut results = Vec::new();
        for page_no in 0..num_pages {
            let page = heap.read_page(page_no)?;
            let num_slots = page.num_slots();
            for slot in 0..num_slots {
                let bytes = match page.get_tuple(slot) {
                    Ok(b) => b,
                    Err(_) => continue, // dead slot
                };
                if bytes.len() < TUPLE_HEADER_SIZE {
                    continue;
                }
                // Decode the tuple header using the proper struct method.
                let header = match storage::tuple::TupleHeader::decode_from(bytes) {
                    Ok(h) => h,
                    Err(_) => continue,
                };
                // If t_xmax != 0, the tuple is logically deleted.
                if header.t_xmax != 0 {
                    continue;
                }
                // User data starts at t_hoff.
                let hoff = header.t_hoff as usize;
                if bytes.len() < hoff {
                    continue;
                }
                let tid = storage::tid::TID::new(page_no, slot);
                results.push((tid, bytes[hoff..].to_vec()));
            }
        }
        Ok(results)
    }

    fn scan_all_pg_class_rows(&self) -> Result<Vec<PgClassRow>, CatalogError> {
        let heap = self.catalog_heaps.get("pg_class").unwrap();
        let mut heap = heap.lock();
        let raw = Self::scan_raw_catalog_tuples(&mut heap)?;
        raw.into_iter()
            .map(|(_, d)| PgClassRow::decode(&d))
            .collect::<Result<Vec<_>, _>>()
    }

    fn scan_all_pg_attribute_rows(&self) -> Result<Vec<PgAttributeRow>, CatalogError> {
        let heap = self.catalog_heaps.get("pg_attribute").unwrap();
        let mut heap = heap.lock();
        let raw = Self::scan_raw_catalog_tuples(&mut heap)?;
        raw.into_iter()
            .map(|(_, d)| PgAttributeRow::decode(&d))
            .collect::<Result<Vec<_>, _>>()
    }

    fn scan_all_namespace_rows(&self) -> Result<Vec<PgNamespaceRow>, CatalogError> {
        let heap = self.catalog_heaps.get("pg_namespace").unwrap();
        let mut heap = heap.lock();
        let raw = Self::scan_raw_catalog_tuples(&mut heap)?;
        raw.into_iter()
            .map(|(_, d)| PgNamespaceRow::decode(&d))
            .collect::<Result<Vec<_>, _>>()
    }

    fn scan_all_authid_rows(&self) -> Result<Vec<PgAuthidRow>, CatalogError> {
        let heap = self.catalog_heaps.get("pg_authid").unwrap();
        let mut heap = heap.lock();
        let raw = Self::scan_raw_catalog_tuples(&mut heap)?;
        raw.into_iter()
            .map(|(_, d)| PgAuthidRow::decode(&d))
            .collect::<Result<Vec<_>, _>>()
    }

    // ── Namespace lookup ──────────────────────────────────────────────────────

    fn resolve_namespace(&self, schema_name: &str) -> Result<u32, CatalogError> {
        {
            let ns_cache = self.ns_cache.read();
            if let Some(&oid) = ns_cache.get(schema_name) {
                return Ok(oid);
            }
        }
        // Re-scan from disk
        let rows = self.scan_all_namespace_rows()?;
        let mut ns_cache = self.ns_cache.write();
        for row in &rows {
            ns_cache.insert(row.nspname.clone(), row.oid);
        }
        ns_cache
            .get(schema_name)
            .copied()
            .ok_or_else(|| CatalogError::SchemaNotFound(schema_name.to_string()))
    }

    // ── DDL operations ────────────────────────────────────────────────────────

    /// Create a new table. Returns the new table's OID.
    pub fn create_table(
        &self,
        xid: Xid,
        schema_name: &str,
        table_name: &str,
        columns: Vec<ColumnDef>,
    ) -> Result<u32, CatalogError> {
        let namespace_oid = self.resolve_namespace(schema_name)?;

        // Check for duplicate
        {
            let name_cache = self.name_cache.read();
            if name_cache.contains_key(&(namespace_oid, table_name.to_string())) {
                return Err(CatalogError::DuplicateTable(table_name.to_string()));
            }
        }

        let oid = self.allocate_oid();

        // Create the heap file for the new table
        let heap_path = self.table_heap_path(oid);
        HeapFile::open(&heap_path)?;

        // Insert into pg_class
        let class_row = PgClassRow {
            oid,
            relname: table_name.to_string(),
            relnamespace: namespace_oid,
            reltype: 0,
            relkind: b'r',
            relnatts: columns.len() as u16,
            relpages: 0,
            reltuples: 0.0,
        };
        self.insert_pg_class_row(xid, &class_row)?;

        // Insert into pg_attribute (one row per column)
        let mut col_defs_with_attnum = columns.clone();
        for (i, col) in col_defs_with_attnum.iter_mut().enumerate() {
            col.attnum = (i + 1) as u16;
        }

        for col in &col_defs_with_attnum {
            let attr_row = PgAttributeRow {
                attrelid: oid,
                attname: col.name.clone(),
                atttypid: col.data_type.oid(),
                atttypmod: col.data_type.typmod(),
                attnum: col.attnum as i16,
                attnotnull: col.not_null,
                atthasdef: col.has_default,
                attisdropped: false,
            };
            self.insert_pg_attribute_row(xid, &attr_row)?;
        }

        // Update caches
        let schema = TableSchema {
            oid,
            name: table_name.to_string(),
            namespace_oid,
            columns: col_defs_with_attnum,
            foreign_keys: Vec::new(),
            check_constraints: Vec::new(),
        };
        {
            let mut schema_cache = self.schema_cache.write();
            schema_cache.insert(oid, schema);
        }
        {
            let mut name_cache = self.name_cache.write();
            name_cache.insert((namespace_oid, table_name.to_string()), oid);
        }

        Ok(oid)
    }

    /// Drop a table (logical delete — does not remove the heap file).
    pub fn drop_table(
        &self,
        xid: Xid,
        schema_name: &str,
        table_name: &str,
    ) -> Result<(), CatalogError> {
        let namespace_oid = self.resolve_namespace(schema_name)?;

        let table_oid = {
            let name_cache = self.name_cache.read();
            name_cache
                .get(&(namespace_oid, table_name.to_string()))
                .copied()
                .ok_or_else(|| CatalogError::TableNotFound(table_name.to_string()))?
        };

        // Soft-delete matching rows in pg_class
        self.delete_pg_class_rows_for(xid, table_oid)?;

        // Soft-delete matching rows in pg_attribute
        self.delete_pg_attribute_rows_for(xid, table_oid)?;

        // Remove from caches
        {
            let mut schema_cache = self.schema_cache.write();
            schema_cache.remove(&table_oid);
        }
        {
            let mut name_cache = self.name_cache.write();
            name_cache.remove(&(namespace_oid, table_name.to_string()));
        }

        Ok(())
    }

    fn delete_pg_class_rows_for(&self, xid: Xid, table_oid: u32) -> Result<(), CatalogError> {
        let heap = self.catalog_heaps.get("pg_class").unwrap();
        let mut heap_guard = heap.lock();

        let raw = Self::scan_raw_catalog_tuples(&mut heap_guard)?;
        let tids_to_delete: Vec<storage::tid::TID> = raw
            .into_iter()
            .filter_map(|(tid, data)| {
                PgClassRow::decode(&data)
                    .ok()
                    .filter(|row| row.oid == table_oid)
                    .map(|_| tid)
            })
            .collect();

        for tid in tids_to_delete {
            self.txn_manager.delete_tuple(xid, &mut heap_guard, tid)?;
        }
        Ok(())
    }

    fn delete_pg_attribute_rows_for(&self, xid: Xid, table_oid: u32) -> Result<(), CatalogError> {
        let heap = self.catalog_heaps.get("pg_attribute").unwrap();
        let mut heap_guard = heap.lock();

        let raw = Self::scan_raw_catalog_tuples(&mut heap_guard)?;
        let tids_to_delete: Vec<storage::tid::TID> = raw
            .into_iter()
            .filter_map(|(tid, data)| {
                PgAttributeRow::decode(&data)
                    .ok()
                    .filter(|row| row.attrelid == table_oid)
                    .map(|_| tid)
            })
            .collect();

        for tid in tids_to_delete {
            self.txn_manager.delete_tuple(xid, &mut heap_guard, tid)?;
        }
        Ok(())
    }

    /// Look up a table by schema and name. Uses cache, falls back to disk.
    pub fn get_table(
        &self,
        schema_name: &str,
        table_name: &str,
    ) -> Result<TableSchema, CatalogError> {
        let namespace_oid = self.resolve_namespace(schema_name)?;

        // Check name_cache first to get the OID
        let maybe_oid = {
            let name_cache = self.name_cache.read();
            name_cache
                .get(&(namespace_oid, table_name.to_string()))
                .copied()
        };

        if let Some(oid) = maybe_oid {
            let schema_cache = self.schema_cache.read();
            if let Some(schema) = schema_cache.get(&oid) {
                return Ok(schema.clone());
            }
        }

        // Scan from disk
        let class_rows = self.scan_all_pg_class_rows()?;
        let attr_rows = self.scan_all_pg_attribute_rows()?;

        let class_row = class_rows
            .iter()
            .find(|r| r.relnamespace == namespace_oid && r.relname == table_name)
            .ok_or_else(|| CatalogError::TableNotFound(table_name.to_string()))?;

        let mut col_attrs: Vec<&PgAttributeRow> = attr_rows
            .iter()
            .filter(|a| a.attrelid == class_row.oid && !a.attisdropped)
            .collect();
        col_attrs.sort_by_key(|a| a.attnum);

        let mut columns = Vec::new();
        for attr in col_attrs {
            let data_type = DataType::from_oid(attr.atttypid, attr.atttypmod).ok_or_else(|| {
                CatalogError::CorruptCatalog(format!(
                    "unknown type OID {} for column {}",
                    attr.atttypid, attr.attname
                ))
            })?;
            let seq_key = Self::sequence_key(schema_name, table_name, &attr.attname);
            let serial = {
                let seqs = self.sequences.lock();
                seqs.contains_key(&seq_key)
            };
            columns.push(ColumnDef {
                name: attr.attname.clone(),
                data_type,
                not_null: attr.attnotnull,
                has_default: attr.atthasdef || serial,
                default_expr: None,
                attnum: attr.attnum as u16,
                serial,
                references: None,
            });
        }

        let schema = TableSchema {
            oid: class_row.oid,
            name: class_row.relname.clone(),
            namespace_oid,
            columns,
            foreign_keys: Vec::new(),
            check_constraints: Vec::new(),
        };

        // Update caches
        {
            let mut sc = self.schema_cache.write();
            sc.insert(schema.oid, schema.clone());
        }
        {
            let mut nc = self.name_cache.write();
            nc.insert((namespace_oid, table_name.to_string()), schema.oid);
        }

        Ok(schema)
    }

    /// List all table names in a schema.
    pub fn list_tables(&self, schema_name: &str) -> Result<Vec<String>, CatalogError> {
        let namespace_oid = self.resolve_namespace(schema_name)?;

        let class_rows = self.scan_all_pg_class_rows()?;
        let tables: Vec<String> = class_rows
            .into_iter()
            .filter(|r| r.relnamespace == namespace_oid && r.relkind == b'r')
            .map(|r| r.relname)
            .collect();

        Ok(tables)
    }

    // ── Role management ───────────────────────────────────────────────────────

    /// Create a new role.
    pub fn create_role(
        &self,
        xid: Xid,
        rolname: &str,
        rolsuper: bool,
        rolcanlogin: bool,
        password_verifier: Option<String>,
    ) -> Result<u32, CatalogError> {
        // Check for duplicate
        let existing = self.scan_all_authid_rows()?;
        if existing.iter().any(|r| r.rolname == rolname) {
            return Err(CatalogError::DuplicateRole(rolname.to_string()));
        }

        let oid = self.allocate_oid();
        let row = PgAuthidRow {
            oid,
            rolname: rolname.to_string(),
            rolsuper,
            rolinherit: false,
            rolcreaterole: rolsuper,
            rolcreatedb: rolsuper,
            rolcanlogin,
            rolbypassrls: false,
            rolpassword: password_verifier,
        };
        self.insert_pg_authid_row(xid, &row)?;
        Ok(oid)
    }

    /// Get a role by name.
    pub fn get_role(&self, rolname: &str) -> Result<PgAuthidRow, CatalogError> {
        let rows = self.scan_all_authid_rows()?;
        rows.into_iter()
            .find(|r| r.rolname == rolname)
            .ok_or_else(|| CatalogError::RoleNotFound(rolname.to_string()))
    }

    /// List all roles.
    pub fn list_roles(&self) -> Result<Vec<PgAuthidRow>, CatalogError> {
        self.scan_all_authid_rows()
    }

    /// List all namespaces (schemas).
    pub fn list_namespaces(&self) -> Result<Vec<PgNamespaceRow>, CatalogError> {
        self.scan_all_namespace_rows()
    }

    /// List all pg_class rows (tables, indexes, etc.).
    pub fn list_pg_class_rows(&self) -> Result<Vec<PgClassRow>, CatalogError> {
        self.scan_all_pg_class_rows()
    }

    /// List all pg_attribute rows.
    pub fn list_pg_attribute_rows(&self) -> Result<Vec<PgAttributeRow>, CatalogError> {
        self.scan_all_pg_attribute_rows()
    }

    /// List all indexes: returns (table_oid, column_name, path) triples.
    pub fn list_indexes(&self) -> Vec<(u32, String, std::path::PathBuf)> {
        let registry = self.index_registry.read();
        registry.iter().map(|((oid, col), path)| (*oid, col.clone(), path.clone())).collect()
    }

    /// Get the data directory.
    pub fn data_dir(&self) -> &std::path::Path {
        &self.data_dir
    }

    /// Authenticate a user: check password matches stored verifier.
    pub fn authenticate(&self, rolname: &str, password: &str) -> Result<bool, CatalogError> {
        let role = self.get_role(rolname)?;
        match &role.rolpassword {
            Some(stored) => Ok(stored == password),
            None => Ok(false),
        }
    }

    /// Open (or get) the heap file for a user table by OID.
    pub fn open_table_heap(&self, oid: u32) -> Result<HeapFile, CatalogError> {
        let path = self.table_heap_path(oid);
        let heap = HeapFile::open(&path)?;
        Ok(heap)
    }

    // ── Test/public helpers ───────────────────────────────────────────────────

    /// Public wrapper for namespace resolution (used in tests).
    pub fn resolve_namespace_pub(&self, schema_name: &str) -> Result<u32, CatalogError> {
        self.resolve_namespace(schema_name)
    }

    /// Begin a new ReadCommitted transaction.
    pub fn txn_manager_begin(&self) -> Xid {
        self.txn_manager.begin(IsolationLevel::ReadCommitted)
    }

    /// Commit a transaction.
    pub fn txn_manager_commit(&self, xid: Xid) -> Result<(), CatalogError> {
        self.txn_manager.commit(xid).map_err(CatalogError::Txn)
    }

    /// Abort a transaction.
    pub fn txn_manager_abort(&self, xid: Xid) -> Result<(), CatalogError> {
        self.txn_manager.abort(xid).map_err(CatalogError::Txn)
    }

    // ── Index registry ────────────────────────────────────────────────────────

    /// Get the OID of a table by schema and name.
    pub fn get_table_oid(&self, schema_name: &str, table_name: &str) -> Result<u32, CatalogError> {
        let ns_oid = self.resolve_namespace(schema_name)?;
        let name_cache = self.name_cache.read();
        name_cache
            .get(&(ns_oid, table_name.to_string()))
            .copied()
            .ok_or_else(|| CatalogError::TableNotFound(table_name.to_string()))
    }

    /// Register an index for a table column and return the file path.
    /// The path is `<data_dir>/idx_<table_oid>_<column_name>.btree`.
    pub fn create_index_entry(&self, table_oid: u32, column_name: &str) -> PathBuf {
        let path = self
            .data_dir
            .join(format!("idx_{}_{}.btree", table_oid, column_name));
        let mut registry = self.index_registry.write();
        registry.insert((table_oid, column_name.to_string()), path.clone());
        path
    }

    /// Look up the path of an existing index, if any.
    pub fn get_index_path(&self, table_oid: u32, column_name: &str) -> Option<PathBuf> {
        let registry = self.index_registry.read();
        registry.get(&(table_oid, column_name.to_string())).cloned()
    }

    // ── Unique / PK constraint registry ────────────────────────────────────────

    /// Register unique/pk constraints for a table.
    pub fn set_unique_constraints(
        &self,
        table_oid: u32,
        unique_columns: Vec<String>,
        primary_key: Option<String>,
    ) {
        let mut reg = self.unique_constraints.write();
        reg.insert(table_oid, UniqueConstraints { unique_columns, primary_key });
    }

    /// Get unique/pk constraints for a table, if any.
    pub fn get_unique_constraints(&self, table_oid: u32) -> Option<UniqueConstraints> {
        let reg = self.unique_constraints.read();
        reg.get(&table_oid).cloned()
    }

    // ── Sequence support ──────────────────────────────────────────────────────

    fn sequence_key(schema: &str, table: &str, col: &str) -> String {
        format!("{schema}_{table}_{col}")
    }

    fn sequence_file_path(&self, key: &str) -> PathBuf {
        let seq_dir = self.data_dir.join("sequences");
        seq_dir.join(format!("{key}.seq"))
    }

    /// Load all sequences from the sequences directory on startup.
    fn load_sequences_from_disk(data_dir: &Path) -> Result<HashMap<String, i64>, CatalogError> {
        let seq_dir = data_dir.join("sequences");
        let mut map = HashMap::new();
        if !seq_dir.exists() {
            return Ok(map);
        }
        if let Ok(entries) = std::fs::read_dir(&seq_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("seq") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            if let Ok(val) = content.trim().parse::<i64>() {
                                map.insert(stem.to_string(), val);
                            }
                        }
                    }
                }
            }
        }
        Ok(map)
    }

    /// Create a new sequence starting at `start`.
    pub fn create_sequence(&self, schema: &str, table: &str, col: &str, start: i64) -> Result<(), CatalogError> {
        let key = Self::sequence_key(schema, table, col);
        // Create sequences directory if needed
        let seq_dir = self.data_dir.join("sequences");
        std::fs::create_dir_all(&seq_dir)?;
        let path = self.sequence_file_path(&key);
        std::fs::write(&path, format!("{start}"))?;
        let mut seqs = self.sequences.lock();
        seqs.insert(key, start);
        Ok(())
    }

    /// Check if a sequence exists.
    pub fn sequence_exists(&self, schema: &str, table: &str, col: &str) -> bool {
        let key = Self::sequence_key(schema, table, col);
        let seqs = self.sequences.lock();
        seqs.contains_key(&key)
    }

    /// Get the next value from a sequence, atomically incrementing it.
    pub fn next_sequence_val(&self, schema: &str, table: &str, col: &str) -> Result<i64, CatalogError> {
        let key = Self::sequence_key(schema, table, col);
        let mut seqs = self.sequences.lock();
        let current = seqs.entry(key.clone()).or_insert(1);
        let val = *current;
        *current += 1;
        // Persist to disk
        let path = self.sequence_file_path(&key);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, format!("{}", *current));
        Ok(val)
    }

    // ── ALTER TABLE operations ─────────────────────────────────────────────────

    /// Add a column to an existing table (schema only; existing rows will return NULL for new col).
    pub fn alter_table_add_column(
        &self,
        xid: Xid,
        schema_name: &str,
        table_name: &str,
        col_name: &str,
    ) -> Result<(), CatalogError> {
        let namespace_oid = self.resolve_namespace(schema_name)?;
        let table_oid = {
            let nc = self.name_cache.read();
            nc.get(&(namespace_oid, table_name.to_string())).copied()
                .ok_or_else(|| CatalogError::TableNotFound(table_name.to_string()))?
        };

        // Get current schema
        let mut schema = {
            let sc = self.schema_cache.read();
            sc.get(&table_oid).cloned()
                .ok_or_else(|| CatalogError::TableNotFound(table_name.to_string()))?
        };

        // Add new column
        let next_attnum = (schema.columns.len() + 1) as u16;
        let new_col = ColumnDef {
            name: col_name.to_string(),
            data_type: DataType::Text,
            not_null: false,
            has_default: false,
            default_expr: None,
            attnum: next_attnum,
            serial: false,
            references: None,
        };

        // Insert into pg_attribute
        let attr_row = PgAttributeRow {
            attrelid: table_oid,
            attname: col_name.to_string(),
            atttypid: DataType::Text.oid(),
            atttypmod: DataType::Text.typmod(),
            attnum: next_attnum as i16,
            attnotnull: false,
            atthasdef: false,
            attisdropped: false,
        };
        self.insert_pg_attribute_row(xid, &attr_row)?;

        // Update caches
        schema.columns.push(new_col);
        let mut sc = self.schema_cache.write();
        sc.insert(table_oid, schema);

        Ok(())
    }

    /// Drop a column from an existing table (marks as dropped in pg_attribute).
    pub fn alter_table_drop_column(
        &self,
        xid: Xid,
        schema_name: &str,
        table_name: &str,
        col_name: &str,
    ) -> Result<(), CatalogError> {
        let namespace_oid = self.resolve_namespace(schema_name)?;
        let table_oid = {
            let nc = self.name_cache.read();
            nc.get(&(namespace_oid, table_name.to_string())).copied()
                .ok_or_else(|| CatalogError::TableNotFound(table_name.to_string()))?
        };

        // Mark column as dropped in pg_attribute heap
        {
            let heap = self.catalog_heaps.get("pg_attribute").unwrap();
            let mut heap_guard = heap.lock();
            let raw = Self::scan_raw_catalog_tuples(&mut heap_guard)?;
            for (tid, data) in raw {
                if let Ok(mut attr) = PgAttributeRow::decode(&data) {
                    if attr.attrelid == table_oid && attr.attname == col_name {
                        attr.attisdropped = true;
                        self.txn_manager.update_tuple(xid, &mut heap_guard, tid, &attr.encode())?;
                        break;
                    }
                }
            }
        }

        // Update schema cache
        let mut sc = self.schema_cache.write();
        if let Some(schema) = sc.get_mut(&table_oid) {
            schema.columns.retain(|c| c.name != col_name);
        }

        Ok(())
    }

    /// Rename a column in an existing table.
    pub fn alter_table_rename_column(
        &self,
        xid: Xid,
        schema_name: &str,
        table_name: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<(), CatalogError> {
        let namespace_oid = self.resolve_namespace(schema_name)?;
        let table_oid = {
            let nc = self.name_cache.read();
            nc.get(&(namespace_oid, table_name.to_string())).copied()
                .ok_or_else(|| CatalogError::TableNotFound(table_name.to_string()))?
        };

        // Update in pg_attribute heap
        {
            let heap = self.catalog_heaps.get("pg_attribute").unwrap();
            let mut heap_guard = heap.lock();
            let raw = Self::scan_raw_catalog_tuples(&mut heap_guard)?;
            for (tid, data) in raw {
                if let Ok(mut attr) = PgAttributeRow::decode(&data) {
                    if attr.attrelid == table_oid && attr.attname == old_name {
                        attr.attname = new_name.to_string();
                        self.txn_manager.update_tuple(xid, &mut heap_guard, tid, &attr.encode())?;
                        break;
                    }
                }
            }
        }

        // Update schema cache
        let mut sc = self.schema_cache.write();
        if let Some(schema) = sc.get_mut(&table_oid) {
            for col in &mut schema.columns {
                if col.name == old_name {
                    col.name = new_name.to_string();
                    break;
                }
            }
        }

        Ok(())
    }

    // ── Foreign key / check constraint registry ───────────────────────────────

    /// Register foreign key constraints for a table.
    pub fn set_foreign_keys(
        &self,
        table_oid: u32,
        fks: Vec<crate::schema::TableForeignKey>,
    ) {
        let mut reg = self.fk_registry.write();
        reg.insert(table_oid, fks);
    }

    /// Get foreign key constraints for a table.
    pub fn get_foreign_keys(
        &self,
        table_oid: u32,
    ) -> Vec<crate::schema::TableForeignKey> {
        let reg = self.fk_registry.read();
        reg.get(&table_oid).cloned().unwrap_or_default()
    }

    /// Find all tables that have an FK pointing to (ref_table, ref_col).
    /// Returns a list of (table_name, TableForeignKey) pairs.
    pub fn find_referencing_tables(
        &self,
        _schema: &str,
        ref_table: &str,
        ref_col: &str,
    ) -> Vec<(String, crate::schema::TableForeignKey)> {
        let fk_reg = self.fk_registry.read();
        let schema_cache = self.schema_cache.read();
        let mut result = Vec::new();
        for (oid, fks) in fk_reg.iter() {
            for fk in fks {
                if fk.ref_table == ref_table && fk.ref_col == ref_col {
                    // Find the table name from schema cache
                    if let Some(schema) = schema_cache.get(oid) {
                        result.push((schema.name.clone(), fk.clone()));
                    }
                }
            }
        }
        result
    }

    /// Register check constraints for a table.
    pub fn set_check_constraints(
        &self,
        table_oid: u32,
        checks: Vec<crate::schema::CheckConstraint>,
    ) {
        let mut reg = self.check_registry.write();
        reg.insert(table_oid, checks);
    }

    /// Get check constraints for a table.
    pub fn get_check_constraints(
        &self,
        table_oid: u32,
    ) -> Vec<crate::schema::CheckConstraint> {
        let reg = self.check_registry.read();
        reg.get(&table_oid).cloned().unwrap_or_default()
    }

    /// Rename a table.
    pub fn alter_table_rename_table(
        &self,
        xid: Xid,
        schema_name: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<(), CatalogError> {
        let namespace_oid = self.resolve_namespace(schema_name)?;
        let table_oid = {
            let nc = self.name_cache.read();
            nc.get(&(namespace_oid, old_name.to_string())).copied()
                .ok_or_else(|| CatalogError::TableNotFound(old_name.to_string()))?
        };

        // Update in pg_class heap
        {
            let heap = self.catalog_heaps.get("pg_class").unwrap();
            let mut heap_guard = heap.lock();
            let raw = Self::scan_raw_catalog_tuples(&mut heap_guard)?;
            for (tid, data) in raw {
                if let Ok(mut cls) = PgClassRow::decode(&data) {
                    if cls.oid == table_oid {
                        cls.relname = new_name.to_string();
                        self.txn_manager.update_tuple(xid, &mut heap_guard, tid, &cls.encode())?;
                        break;
                    }
                }
            }
        }

        // Update caches
        {
            let mut nc = self.name_cache.write();
            nc.remove(&(namespace_oid, old_name.to_string()));
            nc.insert((namespace_oid, new_name.to_string()), table_oid);
        }
        {
            let mut sc = self.schema_cache.write();
            if let Some(schema) = sc.get_mut(&table_oid) {
                schema.name = new_name.to_string();
            }
        }

        Ok(())
    }

    // ── ACL persistence ───────────────────────────────────────────────────────

    fn acl_dir(data_dir: &Path) -> PathBuf {
        data_dir.join("acls")
    }

    fn acl_file_path(data_dir: &Path, schema: &str, table: &str) -> PathBuf {
        Self::acl_dir(data_dir).join(format!("{}_{}.acl", schema, table))
    }

    fn load_acls_from_disk(data_dir: &Path) -> Result<HashMap<String, TableAcl>, CatalogError> {
        let mut map = HashMap::new();
        let acl_dir = Self::acl_dir(data_dir);
        if !acl_dir.exists() {
            return Ok(map);
        }
        if let Ok(entries) = std::fs::read_dir(&acl_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("acl") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            if let Ok(acl) = serde_json::from_str::<TableAcl>(&content) {
                                // stem is "schema_table" but key is "schema.table"
                                // We can't reliably split on _ if names may contain _, so store using stem
                                // We store using "schema.table" format but the file uses "schema_table"
                                // The stem won't contain the dot, so we need to track it differently.
                                // Simple approach: store with stem as the key too.
                                // Actually we store key as "schema.table" and file as "schema_table.acl"
                                // We reconstruct by looking at what file contains
                                let _ = stem; // stem is "schema_table"
                                let key = stem.replacen('_', ".", 1); // "schema.table"
                                map.insert(key, acl);
                            }
                        }
                    }
                }
            }
        }
        Ok(map)
    }

    fn persist_acl(&self, schema: &str, table: &str, acl: &TableAcl) -> Result<(), CatalogError> {
        let acl_dir = Self::acl_dir(&self.data_dir);
        std::fs::create_dir_all(&acl_dir)?;
        let path = Self::acl_file_path(&self.data_dir, schema, table);
        let content = serde_json::to_string(acl)
            .map_err(|e| CatalogError::Serde(e.to_string()))?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    // ── ACL management ────────────────────────────────────────────────────────

    /// Grant privileges on a table to a grantee.
    pub fn grant_table(
        &self,
        schema: &str,
        table: &str,
        grantee: &str,
        privileges: Vec<AclPrivilege>,
    ) -> Result<(), CatalogError> {
        let key = format!("{}.{}", schema, table);
        let mut acls = self.table_acls.lock();
        let acl = acls.entry(key).or_default();

        // Find or create entry for grantee
        if let Some(entry) = acl.grants.iter_mut().find(|e| e.grantee == grantee) {
            for priv_ in &privileges {
                if !entry.privileges.contains(priv_) {
                    entry.privileges.push(priv_.clone());
                }
            }
        } else {
            acl.grants.push(AclEntry {
                grantee: grantee.to_string(),
                privileges,
                col_grants: HashMap::new(),
            });
        }

        let acl_clone = acl.clone();
        drop(acls);
        self.persist_acl(schema, table, &acl_clone)?;
        Ok(())
    }

    /// Revoke privileges on a table from a grantee.
    pub fn revoke_table(
        &self,
        schema: &str,
        table: &str,
        grantee: &str,
        privileges: Vec<AclPrivilege>,
    ) -> Result<(), CatalogError> {
        let key = format!("{}.{}", schema, table);
        let mut acls = self.table_acls.lock();
        if let Some(acl) = acls.get_mut(&key) {
            if let Some(entry) = acl.grants.iter_mut().find(|e| e.grantee == grantee) {
                entry.privileges.retain(|p| !privileges.contains(p));
            }
            let acl_clone = acl.clone();
            drop(acls);
            self.persist_acl(schema, table, &acl_clone)?;
        }
        Ok(())
    }

    /// Check if a role has a specific privilege on a table.
    ///
    /// Returns true if:
    /// - role is superuser (checked by caller), OR
    /// - role has ALL privilege, OR
    /// - role has the specific privilege, OR
    /// - PUBLIC ("*") has the privilege
    pub fn check_table_privilege(
        &self,
        schema: &str,
        table: &str,
        role: &str,
        privilege: AclPrivilege,
    ) -> bool {
        let key = format!("{}.{}", schema, table);
        let acls = self.table_acls.lock();
        let acl = match acls.get(&key) {
            Some(a) => a,
            None => return false,
        };

        for entry in &acl.grants {
            if (entry.grantee == role || entry.grantee == "*")
                && (entry.privileges.contains(&AclPrivilege::All)
                    || entry.privileges.contains(&privilege))
            {
                return true;
            }
        }
        false
    }

    /// Grant column-level privileges: `GRANT SELECT (col1, col2) ON table TO grantee`
    pub fn grant_column_privileges(
        &self,
        schema: &str,
        table: &str,
        grantee: &str,
        columns: &[String],
        privileges: &[AclPrivilege],
    ) -> Result<(), CatalogError> {
        let key = format!("{}.{}", schema, table);
        let mut acls = self.table_acls.lock();
        let acl = acls.entry(key).or_default();

        // Find or create entry for grantee
        if acl.grants.iter().all(|e| e.grantee != grantee) {
            acl.grants.push(AclEntry {
                grantee: grantee.to_string(),
                privileges: vec![],
                col_grants: HashMap::new(),
            });
        }
        let entry = acl.grants.iter_mut().find(|e| e.grantee == grantee).unwrap();

        for col in columns {
            let col_privs = entry.col_grants.entry(col.clone()).or_default();
            for priv_ in privileges {
                if !col_privs.contains(priv_) {
                    col_privs.push(priv_.clone());
                }
            }
        }

        let acl_clone = acl.clone();
        drop(acls);
        self.persist_acl(schema, table, &acl_clone)?;
        Ok(())
    }

    /// Revoke column-level privileges: `REVOKE SELECT (col1) ON table FROM grantee`
    pub fn revoke_column_privileges(
        &self,
        schema: &str,
        table: &str,
        grantee: &str,
        columns: &[String],
        privileges: &[AclPrivilege],
    ) -> Result<(), CatalogError> {
        let key = format!("{}.{}", schema, table);
        let mut acls = self.table_acls.lock();
        if let Some(acl) = acls.get_mut(&key) {
            if let Some(entry) = acl.grants.iter_mut().find(|e| e.grantee == grantee) {
                for col in columns {
                    if let Some(col_privs) = entry.col_grants.get_mut(col) {
                        col_privs.retain(|p| !privileges.contains(p));
                    }
                }
            }
            let acl_clone = acl.clone();
            drop(acls);
            self.persist_acl(schema, table, &acl_clone)?;
        }
        Ok(())
    }

    /// Check whether grantee has a column-level privilege.
    ///
    /// Returns true if the grantee has the privilege at the table level (which
    /// implies column-level access) OR has an explicit column-level grant.
    pub fn check_column_privilege(
        &self,
        schema: &str,
        table: &str,
        column: &str,
        grantee: &str,
        privilege: AclPrivilege,
    ) -> bool {
        // Table-level privilege implies column-level access
        if self.check_table_privilege(schema, table, grantee, privilege.clone()) {
            return true;
        }
        let key = format!("{}.{}", schema, table);
        let acls = self.table_acls.lock();
        let acl = match acls.get(&key) {
            Some(a) => a,
            None => return false,
        };
        acl.grants.iter().any(|e| {
            (e.grantee == grantee || e.grantee == "*")
                && e.col_grants
                    .get(column)
                    .map(|privs| privs.contains(&AclPrivilege::All) || privs.contains(&privilege))
                    .unwrap_or(false)
        })
    }

    // ── LISTEN / NOTIFY ──────────────────────────────────────────────────────

    /// Subscribe to a notification channel. Returns a receiver that will get
    /// messages when NOTIFY is called on the same channel.
    pub fn listen(&self, channel: &str) -> std::sync::mpsc::Receiver<String> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.notify_subscribers
            .lock()
            .entry(channel.to_string())
            .or_default()
            .push(tx);
        rx
    }

    /// Unsubscribe from a channel (or all channels if `channel` is None).
    pub fn unlisten(&self, channel: Option<&str>) {
        let mut subs = self.notify_subscribers.lock();
        match channel {
            Some(ch) => { subs.remove(ch); }
            None => { subs.clear(); }
        }
    }

    /// Send a notification to all subscribers of `channel`.
    pub fn notify(&self, channel: &str, payload: &str) {
        let mut subs = self.notify_subscribers.lock();
        if let Some(senders) = subs.get_mut(channel) {
            senders.retain(|tx| tx.send(payload.to_string()).is_ok());
        }
    }

    // ── Autovacuum tracking ───────────────────────────────────────────────────

    /// Record that `table_name` was just vacuumed.
    pub fn record_vacuum(&self, table_name: &str) {
        self.last_vacuum
            .lock()
            .insert(table_name.to_string(), std::time::Instant::now());
    }

    /// Returns the names of tables in `schema` that have not been vacuumed
    /// within the last `stale_secs` seconds.
    pub fn tables_needing_vacuum(&self, schema: &str, stale_secs: u64) -> Vec<String> {
        let last = self.last_vacuum.lock();
        let cutoff = std::time::Duration::from_secs(stale_secs);
        let tables = match self.list_tables(schema) {
            Ok(t) => t,
            Err(_) => return vec![],
        };
        tables
            .into_iter()
            .filter(|name| {
                match last.get(name.as_str()) {
                    None => true, // never vacuumed
                    Some(instant) => instant.elapsed() > cutoff,
                }
            })
            .collect()
    }

    // ── Function registry ─────────────────────────────────────────────────────

    /// Register a SQL-language function.
    pub fn create_function(&self, func: crate::schema::FunctionDef) -> Result<(), CatalogError> {
        let key = format!("{}.{}", func.schema, func.name);
        self.functions.lock().insert(key, func);
        Ok(())
    }

    /// Look up a function by schema and name.
    pub fn get_function(&self, schema: &str, name: &str) -> Option<crate::schema::FunctionDef> {
        let key = format!("{}.{}", schema, name);
        self.functions.lock().get(&key).cloned()
    }

    /// Drop a function by schema and name.
    pub fn drop_function(&self, schema: &str, name: &str) -> Result<(), CatalogError> {
        let key = format!("{}.{}", schema, name);
        self.functions.lock().remove(&key)
            .ok_or_else(|| CatalogError::TableNotFound(format!("function {}.{}", schema, name)))?;
        Ok(())
    }

    /// List all functions in a schema.
    pub fn list_functions(&self, schema: &str) -> Vec<crate::schema::FunctionDef> {
        let prefix = format!("{}.", schema);
        self.functions
            .lock()
            .values()
            .filter(|f| f.schema == schema || format!("{}.{}", f.schema, f.name).starts_with(&prefix))
            .cloned()
            .collect()
    }

    // ── Column statistics (ANALYZE) ───────────────────────────────────────────

    /// Store statistics for all columns of a table (called by ANALYZE).
    pub fn store_table_stats(
        &self,
        table_name: &str,
        col_stats: HashMap<String, ColumnStats>,
    ) {
        self.table_stats
            .lock()
            .insert(table_name.to_string(), col_stats.clone());
        // Persist to disk
        let path = self.data_dir.join("stats").join(format!("{}.json", table_name));
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&col_stats) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// Get statistics for a specific column, if available.
    pub fn get_column_stats(&self, table_name: &str, col_name: &str) -> Option<ColumnStats> {
        self.table_stats
            .lock()
            .get(table_name)
            .and_then(|t| t.get(col_name))
            .cloned()
    }

    // ── Index helpers ─────────────────────────────────────────────────────────

    /// Check whether an index exists on the given column of `table_oid`.
    pub fn has_index_on_column(&self, table_oid: u32, col_name: &str) -> bool {
        let reg = self.index_registry.read();
        reg.contains_key(&(table_oid, col_name.to_string()))
    }

    // ── Namespace management ──────────────────────────────────────────────────

    /// Create a new schema (namespace). If it already exists and `if_not_exists` is true,
    /// returns Ok(()) silently. Otherwise returns AlreadyExists error.
    pub fn create_namespace(&self, name: &str, if_not_exists: bool) -> Result<(), CatalogError> {
        {
            let ns = self.ns_cache.read();
            if ns.contains_key(name) {
                if if_not_exists {
                    return Ok(());
                }
                return Err(CatalogError::AlreadyExists(format!("schema {}", name)));
            }
        }
        let xid = self.txn_manager.begin(IsolationLevel::ReadCommitted);
        let oid = self.allocate_oid();
        let row = PgNamespaceRow {
            oid,
            nspname: name.to_string(),
            nspowner: 0,
        };
        self.insert_namespace_row(xid, &row)?;
        self.txn_manager.commit(xid)?;
        self.ns_cache.write().insert(name.to_string(), oid);
        Ok(())
    }
}
