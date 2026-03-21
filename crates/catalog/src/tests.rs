use std::sync::Arc;
use tempfile::tempdir;
use txn::manager::TransactionManager;
use wal::writer::WalWriter;

use crate::error::CatalogError;
use crate::manager::CatalogManager;
use crate::oids::*;
use crate::pg_attribute::PgAttributeRow;
use crate::pg_authid::PgAuthidRow;
use crate::pg_class::PgClassRow;
use crate::schema::ColumnDef;
use crate::types::DataType;

fn make_manager() -> (CatalogManager, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let wal_dir = dir.path().join("wal");
    let data_dir = dir.path().join("data");
    let wal = Arc::new(WalWriter::open(&wal_dir).unwrap());
    let txn = Arc::new(TransactionManager::new(wal.clone()));
    let mgr = CatalogManager::open(&data_dir, wal, txn).unwrap();
    (mgr, dir)
}

#[test]
fn test_pg_class_encode_decode() {
    let row = PgClassRow {
        oid: 42,
        relname: "my_table".to_string(),
        relnamespace: OID_NS_PUBLIC,
        reltype: 0,
        relkind: b'r',
        relnatts: 5,
        relpages: 10,
        reltuples: 100.5,
    };
    let encoded = row.encode();
    let decoded = PgClassRow::decode(&encoded).unwrap();
    assert_eq!(decoded.oid, row.oid);
    assert_eq!(decoded.relname, row.relname);
    assert_eq!(decoded.relnamespace, row.relnamespace);
    assert_eq!(decoded.reltype, row.reltype);
    assert_eq!(decoded.relkind, row.relkind);
    assert_eq!(decoded.relnatts, row.relnatts);
    assert_eq!(decoded.relpages, row.relpages);
    assert!((decoded.reltuples - row.reltuples).abs() < f64::EPSILON);
}

#[test]
fn test_pg_attribute_encode_decode() {
    let row = PgAttributeRow {
        attrelid: 1001,
        attname: "my_column".to_string(),
        atttypid: OID_TYPE_TEXT,
        atttypmod: -1,
        attnum: 3,
        attnotnull: true,
        atthasdef: false,
        attisdropped: false,
    };
    let encoded = row.encode();
    let decoded = PgAttributeRow::decode(&encoded).unwrap();
    assert_eq!(decoded.attrelid, row.attrelid);
    assert_eq!(decoded.attname, row.attname);
    assert_eq!(decoded.atttypid, row.atttypid);
    assert_eq!(decoded.atttypmod, row.atttypmod);
    assert_eq!(decoded.attnum, row.attnum);
    assert_eq!(decoded.attnotnull, row.attnotnull);
    assert_eq!(decoded.atthasdef, row.atthasdef);
    assert_eq!(decoded.attisdropped, row.attisdropped);
}

#[test]
fn test_pg_authid_encode_decode() {
    // Without password
    let row = PgAuthidRow {
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
    let encoded = row.encode();
    let decoded = PgAuthidRow::decode(&encoded).unwrap();
    assert_eq!(decoded.oid, row.oid);
    assert_eq!(decoded.rolname, row.rolname);
    assert_eq!(decoded.rolsuper, row.rolsuper);
    assert_eq!(decoded.rolcanlogin, row.rolcanlogin);
    assert!(decoded.rolpassword.is_none());

    // With password
    let row2 = PgAuthidRow {
        oid: 1001,
        rolname: "alice".to_string(),
        rolsuper: false,
        rolinherit: false,
        rolcreaterole: false,
        rolcreatedb: false,
        rolcanlogin: true,
        rolbypassrls: false,
        rolpassword: Some("scram-sha-256$...".to_string()),
    };
    let encoded2 = row2.encode();
    let decoded2 = PgAuthidRow::decode(&encoded2).unwrap();
    assert_eq!(decoded2.rolpassword, Some("scram-sha-256$...".to_string()));
}

#[test]
fn test_catalog_bootstrap() {
    let (mgr, _dir) = make_manager();

    // "public" schema exists
    mgr.get_table("public", "nonexistent").unwrap_err();

    // pg_catalog schema
    let ns_err = mgr.resolve_namespace_pub("pg_catalog");
    assert!(ns_err.is_ok(), "pg_catalog namespace should exist");

    // Superuser role exists
    let role = mgr.get_role("postgres").unwrap();
    assert!(role.rolsuper);
    assert!(role.rolcanlogin);
    assert_eq!(role.oid, OID_ROLE_SUPERUSER);

    // System tables appear in pg_class
    let sys_tables = mgr.list_tables("pg_catalog").unwrap();
    assert!(sys_tables.contains(&"pg_class".to_string()));
    assert!(sys_tables.contains(&"pg_attribute".to_string()));
    assert!(sys_tables.contains(&"pg_authid".to_string()));
    assert!(sys_tables.contains(&"pg_namespace".to_string()));
}

#[test]
fn test_create_and_get_table() {
    let (mgr, _dir) = make_manager();

    let cols = vec![
        ColumnDef {
            name: "id".to_string(),
            data_type: DataType::Int4,
            not_null: true,
            has_default: false,
            default_expr: None,
            attnum: 1,
            serial: false,
            references: None,
        },
        ColumnDef {
            name: "name".to_string(),
            data_type: DataType::Text,
            not_null: false,
            has_default: false,
            default_expr: None,
            attnum: 2,
            serial: false,
            references: None,
        },
        ColumnDef {
            name: "score".to_string(),
            data_type: DataType::Float8,
            not_null: false,
            has_default: false,
            default_expr: None,
            attnum: 3,
            serial: false,
            references: None,
        },
    ];

    // Note: we use the mgr's internal txn_manager_begin/commit helpers
    let xid = mgr.txn_manager_begin();
    let oid = mgr.create_table(xid, "public", "users", cols).unwrap();
    mgr.txn_manager_commit(xid).unwrap();

    assert!(oid >= USER_OID_START);

    let schema = mgr.get_table("public", "users").unwrap();
    assert_eq!(schema.name, "users");
    assert_eq!(schema.columns.len(), 3);
    assert_eq!(schema.columns[0].name, "id");
    assert_eq!(schema.columns[0].data_type, DataType::Int4);
    assert_eq!(schema.columns[1].name, "name");
    assert_eq!(schema.columns[1].data_type, DataType::Text);
    assert_eq!(schema.columns[2].name, "score");
    assert_eq!(schema.columns[2].data_type, DataType::Float8);
}

#[test]
fn test_duplicate_table_error() {
    let (mgr, _dir) = make_manager();

    let cols = vec![ColumnDef {
        name: "id".to_string(),
        data_type: DataType::Int4,
        not_null: true,
        has_default: false,
        default_expr: None,
        attnum: 1,
        serial: false,
        references: None,
    }];

    let xid = mgr.txn_manager_begin();
    mgr.create_table(xid, "public", "items", cols.clone())
        .unwrap();
    mgr.txn_manager_commit(xid).unwrap();

    let xid2 = mgr.txn_manager_begin();
    let result = mgr.create_table(xid2, "public", "items", cols);
    let _ = mgr.txn_manager_abort(xid2);
    assert!(matches!(result, Err(CatalogError::DuplicateTable(_))));
}

#[test]
fn test_drop_table() {
    let (mgr, _dir) = make_manager();

    let cols = vec![ColumnDef {
        name: "id".to_string(),
        data_type: DataType::Int4,
        not_null: true,
        has_default: false,
        default_expr: None,
        attnum: 1,
        serial: false,
        references: None,
    }];

    let xid = mgr.txn_manager_begin();
    mgr.create_table(xid, "public", "temp_table", cols).unwrap();
    mgr.txn_manager_commit(xid).unwrap();

    // Verify it exists
    mgr.get_table("public", "temp_table").unwrap();

    // Drop it
    let xid2 = mgr.txn_manager_begin();
    mgr.drop_table(xid2, "public", "temp_table").unwrap();
    mgr.txn_manager_commit(xid2).unwrap();

    // Should now be gone
    let result = mgr.get_table("public", "temp_table");
    assert!(matches!(result, Err(CatalogError::TableNotFound(_))));
}

#[test]
fn test_list_tables() {
    let (mgr, _dir) = make_manager();

    let simple_col = || {
        vec![ColumnDef {
            name: "id".to_string(),
            data_type: DataType::Int4,
            not_null: true,
            has_default: false,
            default_expr: None,
            attnum: 1,
            serial: false,
            references: None,
        }]
    };

    let xid = mgr.txn_manager_begin();
    mgr.create_table(xid, "public", "table_a", simple_col())
        .unwrap();
    mgr.create_table(xid, "public", "table_b", simple_col())
        .unwrap();
    mgr.create_table(xid, "public", "table_c", simple_col())
        .unwrap();
    mgr.txn_manager_commit(xid).unwrap();

    let tables = mgr.list_tables("public").unwrap();
    assert!(tables.contains(&"table_a".to_string()));
    assert!(tables.contains(&"table_b".to_string()));
    assert!(tables.contains(&"table_c".to_string()));
}

#[test]
fn test_create_role() {
    let (mgr, _dir) = make_manager();

    let xid = mgr.txn_manager_begin();
    let oid = mgr.create_role(xid, "alice", false, true, None).unwrap();
    mgr.txn_manager_commit(xid).unwrap();

    assert!(oid >= USER_OID_START);

    let role = mgr.get_role("alice").unwrap();
    assert_eq!(role.rolname, "alice");
    assert!(!role.rolsuper);
    assert!(role.rolcanlogin);
    assert!(role.rolpassword.is_none());
}

#[test]
fn test_authenticate() {
    let (mgr, _dir) = make_manager();

    let xid = mgr.txn_manager_begin();
    mgr.create_role(xid, "bob", false, true, Some("secret".to_string()))
        .unwrap();
    mgr.txn_manager_commit(xid).unwrap();

    assert!(mgr.authenticate("bob", "secret").unwrap());
    assert!(!mgr.authenticate("bob", "wrong").unwrap());
}

#[test]
fn test_catalog_persistence() {
    let dir = tempdir().unwrap();
    let wal_dir = dir.path().join("wal");
    let data_dir = dir.path().join("data");

    // First open: create 2 tables
    {
        let wal = Arc::new(WalWriter::open(&wal_dir).unwrap());
        let txn = Arc::new(TransactionManager::new(wal.clone()));
        let mgr = CatalogManager::open(&data_dir, wal, txn).unwrap();

        let cols = vec![ColumnDef {
            name: "id".to_string(),
            data_type: DataType::Int8,
            not_null: true,
            has_default: false,
            default_expr: None,
            attnum: 1,
            serial: false,
            references: None,
        }];

        let xid = mgr.txn_manager_begin();
        mgr.create_table(xid, "public", "persistent_a", cols.clone())
            .unwrap();
        mgr.create_table(xid, "public", "persistent_b", cols)
            .unwrap();
        mgr.txn_manager_commit(xid).unwrap();
    }

    // Second open: verify tables still visible
    {
        let wal = Arc::new(WalWriter::open(&wal_dir).unwrap());
        let txn = Arc::new(TransactionManager::new(wal.clone()));
        let mgr = CatalogManager::open(&data_dir, wal, txn).unwrap();

        let schema_a = mgr.get_table("public", "persistent_a").unwrap();
        assert_eq!(schema_a.name, "persistent_a");
        assert_eq!(schema_a.columns.len(), 1);
        assert_eq!(schema_a.columns[0].data_type, DataType::Int8);

        let schema_b = mgr.get_table("public", "persistent_b").unwrap();
        assert_eq!(schema_b.name, "persistent_b");
    }
}
