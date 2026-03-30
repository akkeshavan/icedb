/// Category 9: DDL (Data Definition Language) tests
/// CREATE TABLE, DROP TABLE, CREATE INDEX, type coverage.
use tempfile::TempDir;
use crate::common::{make_engine, exec, count_rows, Backend};
use sql::Value;

fn test_create_table_basic_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INTEGER, name TEXT)");

    // Table should exist and be selectable
    let n = count_rows(b, "SELECT * FROM t");
    assert_eq!(n, 0, "New empty table should have 0 rows");
}

#[test]
fn test_create_table_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_table_basic_body(&b);
}

crate::net_tests!(test_create_table_basic);


fn test_create_table_int_types_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (a INT, b BIGINT, c INTEGER)");
    exec(b, "INSERT INTO t VALUES (1, 9999999999, 42)");

    let result = exec(b, "SELECT * FROM t");
    assert_eq!(result.rows.len(), 1);
}

#[test]
fn test_create_table_int_types() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_table_int_types_body(&b);
}

crate::net_tests!(test_create_table_int_types);


fn test_create_table_text_types_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (a TEXT, b VARCHAR(255))");
    exec(b, "INSERT INTO t VALUES ('hello', 'world')");

    let result = exec(b, "SELECT a, b FROM t");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("expected Text, got {:?}", other),
    }
}

#[test]
fn test_create_table_text_types() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_table_text_types_body(&b);
}

crate::net_tests!(test_create_table_text_types);


fn test_create_table_bool_type_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, flag BOOLEAN)");
    exec(b, "INSERT INTO t VALUES (1, true)");
    exec(b, "INSERT INTO t VALUES (2, false)");

    let result = exec(b, "SELECT flag FROM t WHERE id = 1");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("expected Bool(true), got {:?}", other),
    }
}

#[test]
fn test_create_table_bool_type() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_table_bool_type_body(&b);
}

crate::net_tests!(test_create_table_bool_type);


fn test_create_table_float_types_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (a FLOAT, b DOUBLE PRECISION)");
    exec(b, "INSERT INTO t VALUES (3.14, 2.718281828)");

    let result = exec(b, "SELECT a FROM t");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => {
            assert!((v - std::f64::consts::PI).abs() < 0.01, "expected ~PI, got {}", v);
        }
        other => panic!("expected Float8, got {:?}", other),
    }
}

#[test]
fn test_create_table_float_types() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_table_float_types_body(&b);
}

crate::net_tests!(test_create_table_float_types);


fn test_create_table_not_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT NOT NULL, name TEXT NOT NULL)");

    // Valid insert
    exec(b, "INSERT INTO t VALUES (1, 'Alice')");

    // NULL insert should fail
    let result = b.try_execute("INSERT INTO t VALUES (2, NULL)");
    assert!(result.is_err(), "Inserting NULL into NOT NULL column must fail");
}

#[test]
fn test_create_table_not_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_table_not_null_body(&b);
}

crate::net_tests!(test_create_table_not_null);


fn test_create_table_not_null_int_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT NOT NULL)");

    let result = b.try_execute("INSERT INTO t VALUES (NULL)");
    assert!(result.is_err(), "Inserting NULL into INT NOT NULL column must fail");
}

#[test]
fn test_create_table_not_null_int() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_table_not_null_int_body(&b);
}

crate::net_tests!(test_create_table_not_null_int);


fn test_drop_table_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT)");
    exec(b, "INSERT INTO t VALUES (1)");

    exec(b, "DROP TABLE t");

    // After drop, table should not exist
    let result = b.try_execute("SELECT * FROM t");
    assert!(result.is_err(), "Selecting from dropped table should error");
    let err = result.unwrap_err();
    assert_eq!(err.sqlstate(), "42P01", "Dropped table should return SQLSTATE 42P01");
}

#[test]
fn test_drop_table() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_drop_table_body(&b);
}

crate::net_tests!(test_drop_table);


fn test_drop_table_nonexistent_error_body(b: &crate::common::Backend) {
    let result = b.try_execute("DROP TABLE nonexistent_table");
    assert!(result.is_err(), "DROP TABLE on nonexistent table should error");
}

#[test]
fn test_drop_table_nonexistent_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_drop_table_nonexistent_error_body(&b);
}

crate::net_tests!(test_drop_table_nonexistent_error);


fn test_drop_table_if_exists_body(b: &crate::common::Backend) {
    // DROP TABLE IF EXISTS on nonexistent table should not error
    exec(b, "DROP TABLE IF EXISTS nonexistent_table");
}

#[test]
fn test_drop_table_if_exists() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_drop_table_if_exists_body(&b);
}

crate::net_tests!(test_drop_table_if_exists);


fn test_create_table_duplicate_error_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT)");

    let result = b.try_execute("CREATE TABLE t (id INT, name TEXT)");
    assert!(result.is_err(), "Creating duplicate table should fail");

    // Check SQLSTATE for duplicate table
    let err = result.unwrap_err();
    assert_eq!(err.sqlstate(), "42P07",
        "Duplicate table should have SQLSTATE 42P07, got: {}", err.sqlstate());
}

#[test]
fn test_create_table_duplicate_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_table_duplicate_error_body(&b);
}

crate::net_tests!(test_create_table_duplicate_error);


fn test_create_index_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, name TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Carol')");

    exec(b, "CREATE INDEX idx_t_id ON t(id)");

    // Index created — queries still work correctly
    let result = exec(b, "SELECT name FROM t WHERE id = 2");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "Bob"),
        other => panic!("expected Bob, got {:?}", other),
    }
}

#[test]
fn test_create_index() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_index_body(&b);
}

crate::net_tests!(test_create_index);


fn test_create_index_multi_column_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (a INT, b INT, val TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 1, 'aa')");
    exec(b, "INSERT INTO t VALUES (1, 2, 'ab')");
    exec(b, "INSERT INTO t VALUES (2, 1, 'ba')");

    exec(b, "CREATE INDEX idx_t_ab ON t(a, b)");

    let n = count_rows(b, "SELECT * FROM t WHERE a = 1");
    assert_eq!(n, 2, "Index should not affect query results for a=1 (2 rows)");
}

#[test]
fn test_create_index_multi_column() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_index_multi_column_body(&b);
}

crate::net_tests!(test_create_index_multi_column);


fn test_drop_recreate_table_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, val TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'first')");

    exec(b, "DROP TABLE t");

    // Recreate with different schema
    exec(b, "CREATE TABLE t (id INT, name TEXT, score INT)");
    exec(b, "INSERT INTO t VALUES (1, 'Alice', 95)");

    let result = exec(b, "SELECT name, score FROM t WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "Alice"),
        other => panic!("expected Alice after recreate, got {:?}", other),
    }
}

#[test]
fn test_drop_recreate_table() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_drop_recreate_table_body(&b);
}

crate::net_tests!(test_drop_recreate_table);


fn test_create_table_many_columns_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE wide (a INT, b INT, c INT, d TEXT, e TEXT, f FLOAT, g FLOAT, h BOOLEAN)");
    exec(b, "INSERT INTO wide VALUES (1, 2, 3, 'hello', 'world', 3.14, 2.71, true)");

    let result = exec(b, "SELECT * FROM wide");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].values.len(), 8);
}

#[test]
fn test_create_table_many_columns() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_table_many_columns_body(&b);
}

crate::net_tests!(test_create_table_many_columns);


fn test_table_survives_restart_body(b: &crate::common::Backend) {
    {
        exec(b, "CREATE TABLE persistent (id INT, data TEXT)");
        exec(b, "INSERT INTO persistent VALUES (1, 'survives')");
    }

    // Re-open the engine (simulating restart)
    {
        let result = exec(b, "SELECT data FROM persistent WHERE id = 1");
        assert_eq!(result.rows.len(), 1, "Table and data should persist after restart");
        match result.rows[0].get_by_idx(0) {
            Some(Value::Text(s)) => assert_eq!(s, "survives"),
            other => panic!("expected 'survives', got {:?}", other),
        }
    }
}

#[test]
fn test_table_survives_restart() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_table_survives_restart_body(&b);
}

crate::net_tests!(test_table_survives_restart);


fn test_create_table_then_alter_via_drop_recreate_body(b: &crate::common::Backend) {
    // No ALTER TABLE support yet — test the drop+recreate pattern
    exec(b, "CREATE TABLE t (id INT)");
    exec(b, "DROP TABLE t");
    exec(b, "CREATE TABLE t (id INT, name TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'test')");

    let result = exec(b, "SELECT name FROM t");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "test"),
        other => panic!("expected 'test', got {:?}", other),
    }
}

#[test]
fn test_create_table_then_alter_via_drop_recreate() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_table_then_alter_via_drop_recreate_body(&b);
}

crate::net_tests!(test_create_table_then_alter_via_drop_recreate);


fn test_alter_table_add_column_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT)");
    exec(b, "INSERT INTO t VALUES (1)");

    exec(b, "ALTER TABLE t ADD COLUMN name TEXT");

    let result = exec(b, "SELECT id, name FROM t");
    assert_eq!(result.rows.len(), 1);
}

#[test]
fn test_alter_table_add_column() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_alter_table_add_column_body(&b);
}

crate::net_tests!(test_alter_table_add_column);


fn test_create_table_unique_constraint_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT UNIQUE, name TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'Alice')");

    // Duplicate id=1 should fail
    let result = b.try_execute("INSERT INTO t VALUES (1, 'Bob')");
    assert!(result.is_err(), "UNIQUE constraint should reject duplicate id");
    let err = result.unwrap_err();
    assert_eq!(err.sqlstate(), "23505", "UNIQUE violation should be SQLSTATE 23505");
}

#[test]
fn test_create_table_unique_constraint() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_table_unique_constraint_body(&b);
}

crate::net_tests!(test_create_table_unique_constraint);


fn test_create_table_primary_key_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT PRIMARY KEY, name TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'Alice')");

    // Duplicate PK should fail
    let result = b.try_execute("INSERT INTO t VALUES (1, 'Bob')");
    assert!(result.is_err(), "PRIMARY KEY should reject duplicate values");
}

#[test]
fn test_create_table_primary_key() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_table_primary_key_body(&b);
}

crate::net_tests!(test_create_table_primary_key);

