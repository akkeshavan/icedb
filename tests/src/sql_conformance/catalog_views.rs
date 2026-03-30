/// Tests for information_schema/pg_catalog virtual views, COPY FROM/TO, and PREPARE/EXECUTE.
use tempfile::TempDir;
use crate::common::{make_engine, exec, exec_err, Backend};
use sql::Value;

// ── information_schema.tables ─────────────────────────────────────────────────

fn test_information_schema_tables_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE catalog_test (id INT)");
    let result = exec(b, "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'");
    assert!(
        result.rows.iter().any(|r| r.get("table_name") == Some(&Value::Text("catalog_test".to_string()))),
        "Expected catalog_test in information_schema.tables, got: {:?}",
        result.rows
    );
}

#[test]
fn test_information_schema_tables() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_information_schema_tables_body(&b);
}

crate::net_tests!(test_information_schema_tables);


fn test_information_schema_tables_type_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE type_test (id INT)");
    let result = exec(b, "SELECT table_type FROM information_schema.tables WHERE table_name = 'type_test'");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].get("table_type"), Some(&Value::Text("BASE TABLE".to_string())));
}

#[test]
fn test_information_schema_tables_type() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_information_schema_tables_type_body(&b);
}

crate::net_tests!(test_information_schema_tables_type);


// ── information_schema.columns ────────────────────────────────────────────────

fn test_information_schema_columns_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE cols_test (id INT, name TEXT)");
    let result = exec(b, "SELECT column_name, data_type FROM information_schema.columns WHERE table_name = 'cols_test'");
    assert_eq!(result.rows.len(), 2, "Expected 2 columns, got: {:?}", result.rows);
    // Verify column names are present
    let col_names: Vec<Option<&Value>> = result.rows.iter().map(|r| r.get("column_name")).collect();
    assert!(col_names.contains(&Some(&Value::Text("id".to_string()))));
    assert!(col_names.contains(&Some(&Value::Text("name".to_string()))));
}

#[test]
fn test_information_schema_columns() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_information_schema_columns_body(&b);
}

crate::net_tests!(test_information_schema_columns);


fn test_information_schema_columns_ordinal_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE ordinal_test (a INT, b INT, c TEXT)");
    let result = exec(b, "SELECT column_name, ordinal_position FROM information_schema.columns WHERE table_name = 'ordinal_test' ORDER BY ordinal_position");
    assert_eq!(result.rows.len(), 3);
    assert_eq!(result.rows[0].get("ordinal_position"), Some(&Value::Int4(1)));
    assert_eq!(result.rows[1].get("ordinal_position"), Some(&Value::Int4(2)));
    assert_eq!(result.rows[2].get("ordinal_position"), Some(&Value::Int4(3)));
}

#[test]
fn test_information_schema_columns_ordinal() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_information_schema_columns_ordinal_body(&b);
}

crate::net_tests!(test_information_schema_columns_ordinal);


// ── information_schema.schemata ───────────────────────────────────────────────

fn test_information_schema_schemata_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT schema_name FROM information_schema.schemata");
    let names: Vec<Option<&Value>> = result.rows.iter().map(|r| r.get("schema_name")).collect();
    assert!(names.contains(&Some(&Value::Text("public".to_string()))));
}

#[test]
fn test_information_schema_schemata() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_information_schema_schemata_body(&b);
}

crate::net_tests!(test_information_schema_schemata);


// ── pg_catalog.pg_tables ──────────────────────────────────────────────────────

fn test_pg_catalog_pg_tables_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE pgtables_test (id INT)");
    let result = exec(b, "SELECT tablename FROM pg_catalog.pg_tables WHERE schemaname = 'public'");
    assert!(
        result.rows.iter().any(|r| r.get("tablename") == Some(&Value::Text("pgtables_test".to_string()))),
        "Expected pgtables_test in pg_catalog.pg_tables, got: {:?}",
        result.rows
    );
}

#[test]
fn test_pg_catalog_pg_tables() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_pg_catalog_pg_tables_body(&b);
}

crate::net_tests!(test_pg_catalog_pg_tables);


fn test_pg_tables_without_schema_prefix_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE pg_test_tbl (id INT)");
    // pg_tables without schema prefix should also work
    let result = exec(b, "SELECT tablename FROM pg_tables WHERE schemaname = 'public'");
    assert!(
        result.rows.iter().any(|r| r.get("tablename") == Some(&Value::Text("pg_test_tbl".to_string()))),
        "Expected pg_test_tbl in pg_tables, got: {:?}",
        result.rows
    );
}

#[test]
fn test_pg_tables_without_schema_prefix() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_pg_tables_without_schema_prefix_body(&b);
}

crate::net_tests!(test_pg_tables_without_schema_prefix);


// ── pg_catalog.pg_namespace ───────────────────────────────────────────────────

fn test_pg_catalog_pg_namespace_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT nspname FROM pg_catalog.pg_namespace");
    let names: Vec<Option<&Value>> = result.rows.iter().map(|r| r.get("nspname")).collect();
    assert!(names.contains(&Some(&Value::Text("public".to_string()))));
    assert!(names.contains(&Some(&Value::Text("pg_catalog".to_string()))));
}

#[test]
fn test_pg_catalog_pg_namespace() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_pg_catalog_pg_namespace_body(&b);
}

crate::net_tests!(test_pg_catalog_pg_namespace);


// ── pg_catalog.pg_type ────────────────────────────────────────────────────────

fn test_pg_catalog_pg_type_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT typname FROM pg_catalog.pg_type");
    let type_names: Vec<Option<&Value>> = result.rows.iter().map(|r| r.get("typname")).collect();
    assert!(type_names.contains(&Some(&Value::Text("text".to_string()))));
    assert!(type_names.contains(&Some(&Value::Text("int4".to_string()))));
    assert!(type_names.contains(&Some(&Value::Text("bool".to_string()))));
}

#[test]
fn test_pg_catalog_pg_type() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_pg_catalog_pg_type_body(&b);
}

crate::net_tests!(test_pg_catalog_pg_type);


// ── pg_catalog.pg_roles ───────────────────────────────────────────────────────

fn test_pg_catalog_pg_roles_body(b: &crate::common::Backend) {
    exec(b, "CREATE ROLE test_role_view LOGIN");
    let result = exec(b, "SELECT rolname, rolcanlogin FROM pg_catalog.pg_roles");
    assert!(
        result.rows.iter().any(|r| r.get("rolname") == Some(&Value::Text("test_role_view".to_string()))),
        "Expected test_role_view in pg_roles, got: {:?}",
        result.rows
    );
}

#[test]
fn test_pg_catalog_pg_roles() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_pg_catalog_pg_roles_body(&b);
}

crate::net_tests!(test_pg_catalog_pg_roles);


// ── pg_catalog.pg_class ───────────────────────────────────────────────────────

fn test_pg_catalog_pg_class_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE pgclass_test (id INT)");
    let result = exec(b, "SELECT relname FROM pg_catalog.pg_class WHERE relkind = 'r'");
    assert!(
        result.rows.iter().any(|r| r.get("relname") == Some(&Value::Text("pgclass_test".to_string()))),
        "Expected pgclass_test in pg_class, got: {:?}",
        result.rows
    );
}

#[test]
fn test_pg_catalog_pg_class() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_pg_catalog_pg_class_body(&b);
}

crate::net_tests!(test_pg_catalog_pg_class);


// ── pg_catalog.pg_attribute ───────────────────────────────────────────────────

fn test_pg_catalog_pg_attribute_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE pgattr_test (id INT, name TEXT)");
    let result = exec(b, "SELECT attname FROM pg_catalog.pg_attribute WHERE attisdropped = false");
    let names: Vec<Option<&Value>> = result.rows.iter().map(|r| r.get("attname")).collect();
    assert!(names.contains(&Some(&Value::Text("id".to_string()))));
    assert!(names.contains(&Some(&Value::Text("name".to_string()))));
}

#[test]
fn test_pg_catalog_pg_attribute() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_pg_catalog_pg_attribute_body(&b);
}

crate::net_tests!(test_pg_catalog_pg_attribute);


// ── COPY TO / COPY FROM ───────────────────────────────────────────────────────

fn test_copy_to_csv_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE copy_test (id INT, name TEXT)");
    exec(b, "INSERT INTO copy_test VALUES (1, 'Alice'), (2, 'Bob')");
    let tmp = std::env::temp_dir().join("icedb_copy_test.csv");
    exec(b, &format!("COPY copy_test TO '{}' (FORMAT CSV, HEADER true)", tmp.display()));
    let content = std::fs::read_to_string(&tmp).unwrap();
    assert!(content.contains("Alice"), "CSV should contain Alice: {}", content);
    assert!(content.contains("Bob"), "CSV should contain Bob: {}", content);
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_copy_to_csv() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_copy_to_csv_body(&b);
}

crate::net_tests!(test_copy_to_csv);


fn test_copy_from_csv_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE copy_in (id INT, name TEXT)");
    let tmp = std::env::temp_dir().join("icedb_copy_in.csv");
    std::fs::write(&tmp, "id,name\n1,Alice\n2,Bob\n").unwrap();
    exec(b, &format!("COPY copy_in FROM '{}' (FORMAT CSV, HEADER true)", tmp.display()));
    let result = exec(b, "SELECT COUNT(*) FROM copy_in");
    let count = match result.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(Value::Int8(n)) => *n,
        Some(Value::Int4(n)) => *n as i64,
        other => panic!("Expected count, got {:?}", other),
    };
    assert_eq!(count, 2, "Expected 2 rows after COPY FROM");
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_copy_from_csv() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_copy_from_csv_body(&b);
}

crate::net_tests!(test_copy_from_csv);


fn test_copy_roundtrip_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE copy_roundtrip (id INT, val TEXT)");
    exec(b, "INSERT INTO copy_roundtrip VALUES (10, 'hello'), (20, 'world')");

    let tmp = std::env::temp_dir().join("icedb_roundtrip.csv");
    exec(b, &format!("COPY copy_roundtrip TO '{}' (FORMAT CSV, HEADER true)", tmp.display()));

    exec(b, "CREATE TABLE copy_roundtrip2 (id INT, val TEXT)");
    exec(b, &format!("COPY copy_roundtrip2 FROM '{}' (FORMAT CSV, HEADER true)", tmp.display()));

    let result = exec(b, "SELECT id, val FROM copy_roundtrip2 ORDER BY id");
    assert_eq!(result.rows.len(), 2);
    assert_eq!(result.rows[0].get("id"), Some(&Value::Int4(10)));
    assert_eq!(result.rows[1].get("id"), Some(&Value::Int4(20)));
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_copy_roundtrip() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_copy_roundtrip_body(&b);
}

crate::net_tests!(test_copy_roundtrip);


// ── PREPARE / EXECUTE / DEALLOCATE ───────────────────────────────────────────

fn test_prepare_execute_basic_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE prep_test (id INT, val TEXT)");
    exec(b, "INSERT INTO prep_test VALUES (1, 'one'), (2, 'two'), (3, 'three')");

    exec(b, "PREPARE myq AS SELECT val FROM prep_test WHERE id = $1");
    let result = exec(b, "EXECUTE myq(1)");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].get("val"), Some(&Value::Text("one".to_string())));
}

#[test]
fn test_prepare_execute_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_prepare_execute_basic_body(&b);
}

crate::net_tests!(test_prepare_execute_basic);


fn test_prepare_execute_multiple_params_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE prep_multi (id INT, val TEXT)");
    exec(b, "INSERT INTO prep_multi VALUES (1, 'a'), (2, 'b'), (3, 'c')");

    exec(b, "PREPARE range_q AS SELECT id, val FROM prep_multi WHERE id >= $1 AND id <= $2");
    let result = exec(b, "EXECUTE range_q(1, 2)");
    assert_eq!(result.rows.len(), 2);
}

#[test]
fn test_prepare_execute_multiple_params() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_prepare_execute_multiple_params_body(&b);
}

crate::net_tests!(test_prepare_execute_multiple_params);


fn test_prepare_execute_reuse_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE prep_reuse (id INT, val TEXT)");
    exec(b, "INSERT INTO prep_reuse VALUES (1, 'first'), (2, 'second')");

    exec(b, "PREPARE lookup AS SELECT val FROM prep_reuse WHERE id = $1");

    let r1 = exec(b, "EXECUTE lookup(1)");
    assert_eq!(r1.rows.len(), 1);
    assert_eq!(r1.rows[0].get("val"), Some(&Value::Text("first".to_string())));

    let r2 = exec(b, "EXECUTE lookup(2)");
    assert_eq!(r2.rows.len(), 1);
    assert_eq!(r2.rows[0].get("val"), Some(&Value::Text("second".to_string())));
}

#[test]
fn test_prepare_execute_reuse() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_prepare_execute_reuse_body(&b);
}

crate::net_tests!(test_prepare_execute_reuse);


fn test_deallocate_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE dealloc_test (id INT)");
    exec(b, "INSERT INTO dealloc_test VALUES (1)");

    exec(b, "PREPARE stmtname AS SELECT id FROM dealloc_test");
    // Verify it works
    let r = exec(b, "EXECUTE stmtname()");
    assert_eq!(r.rows.len(), 1);

    // Deallocate it
    exec(b, "DEALLOCATE stmtname");

    // Now executing should fail
    let err = exec_err(b, "EXECUTE stmtname()");
    let err_str = format!("{}", err);
    assert!(err_str.contains("stmtname") || err_str.contains("does not exist"),
        "Expected not-found error, got: {}", err_str);
}

#[test]
fn test_deallocate() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_deallocate_body(&b);
}

crate::net_tests!(test_deallocate);


fn test_deallocate_all_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE dealloc_all_test (id INT)");
    exec(b, "PREPARE s1 AS SELECT id FROM dealloc_all_test");
    exec(b, "PREPARE s2 AS SELECT id FROM dealloc_all_test");
    exec(b, "DEALLOCATE ALL");
    let err1 = exec_err(b, "EXECUTE s1()");
    assert!(format!("{}", err1).contains("does not exist") || format!("{}", err1).contains("s1"));
}

#[test]
fn test_deallocate_all() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_deallocate_all_body(&b);
}

crate::net_tests!(test_deallocate_all);

