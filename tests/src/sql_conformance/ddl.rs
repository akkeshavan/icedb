/// Category 9: DDL (Data Definition Language) tests
/// CREATE TABLE, DROP TABLE, CREATE INDEX, type coverage.
use tempfile::TempDir;
use crate::common::{make_engine, exec, count_rows};
use sql::Value;

#[test]
fn test_create_table_basic() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INTEGER, name TEXT)").unwrap();

    // Table should exist and be selectable
    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 0, "New empty table should have 0 rows");
}

#[test]
fn test_create_table_int_types() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (a INT, b BIGINT, c INTEGER)").unwrap();
    exec(&engine, "INSERT INTO t VALUES (1, 9999999999, 42)");

    let result = engine.execute("SELECT * FROM t").unwrap();
    assert_eq!(result.rows.len(), 1);
}

#[test]
fn test_create_table_text_types() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (a TEXT, b VARCHAR(255))").unwrap();
    exec(&engine, "INSERT INTO t VALUES ('hello', 'world')");

    let result = engine.execute("SELECT a, b FROM t").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("expected Text, got {:?}", other),
    }
}

#[test]
fn test_create_table_bool_type() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, flag BOOLEAN)").unwrap();
    exec(&engine, "INSERT INTO t VALUES (1, true)");
    exec(&engine, "INSERT INTO t VALUES (2, false)");

    let result = engine.execute("SELECT flag FROM t WHERE id = 1").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("expected Bool(true), got {:?}", other),
    }
}

#[test]
fn test_create_table_float_types() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (a FLOAT, b DOUBLE PRECISION)").unwrap();
    exec(&engine, "INSERT INTO t VALUES (3.14, 2.718281828)");

    let result = engine.execute("SELECT a FROM t").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => {
            assert!((v - std::f64::consts::PI).abs() < 0.01, "expected ~PI, got {}", v);
        }
        other => panic!("expected Float8, got {:?}", other),
    }
}

#[test]
fn test_create_table_not_null() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT NOT NULL, name TEXT NOT NULL)").unwrap();

    // Valid insert
    engine.execute("INSERT INTO t VALUES (1, 'Alice')").unwrap();

    // NULL insert should fail
    let result = engine.execute("INSERT INTO t VALUES (2, NULL)");
    assert!(result.is_err(), "Inserting NULL into NOT NULL column must fail");
}

#[test]
fn test_create_table_not_null_int() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT NOT NULL)").unwrap();

    let result = engine.execute("INSERT INTO t VALUES (NULL)");
    assert!(result.is_err(), "Inserting NULL into INT NOT NULL column must fail");
}

#[test]
fn test_drop_table() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");
    exec(&engine, "INSERT INTO t VALUES (1)");

    engine.execute("DROP TABLE t").unwrap();

    // After drop, table should not exist
    let result = engine.execute("SELECT * FROM t");
    assert!(result.is_err(), "Selecting from dropped table should error");
    let err = result.unwrap_err();
    assert_eq!(err.sqlstate(), "42P01", "Dropped table should return SQLSTATE 42P01");
}

#[test]
fn test_drop_table_nonexistent_error() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    let result = engine.execute("DROP TABLE nonexistent_table");
    assert!(result.is_err(), "DROP TABLE on nonexistent table should error");
}

#[test]
fn test_drop_table_if_exists() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    // DROP TABLE IF EXISTS on nonexistent table should not error
    engine.execute("DROP TABLE IF EXISTS nonexistent_table").unwrap();
}

#[test]
fn test_create_table_duplicate_error() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");

    let result = engine.execute("CREATE TABLE t (id INT, name TEXT)");
    assert!(result.is_err(), "Creating duplicate table should fail");

    // Check SQLSTATE for duplicate table
    let err = result.unwrap_err();
    assert_eq!(err.sqlstate(), "42P07",
        "Duplicate table should have SQLSTATE 42P07, got: {}", err.sqlstate());
}

#[test]
fn test_create_index() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, name TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Carol')");

    engine.execute("CREATE INDEX idx_t_id ON t(id)").unwrap();

    // Index created — queries still work correctly
    let result = engine.execute("SELECT name FROM t WHERE id = 2").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "Bob"),
        other => panic!("expected Bob, got {:?}", other),
    }
}

#[test]
fn test_create_index_multi_column() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (a INT, b INT, val TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 1, 'aa')");
    exec(&engine, "INSERT INTO t VALUES (1, 2, 'ab')");
    exec(&engine, "INSERT INTO t VALUES (2, 1, 'ba')");

    engine.execute("CREATE INDEX idx_t_ab ON t(a, b)").unwrap();

    let n = count_rows(&engine, "SELECT * FROM t WHERE a = 1");
    assert_eq!(n, 2, "Index should not affect query results for a=1 (2 rows)");
}

#[test]
fn test_drop_recreate_table() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'first')");

    engine.execute("DROP TABLE t").unwrap();

    // Recreate with different schema
    engine.execute("CREATE TABLE t (id INT, name TEXT, score INT)").unwrap();
    exec(&engine, "INSERT INTO t VALUES (1, 'Alice', 95)");

    let result = engine.execute("SELECT name, score FROM t WHERE id = 1").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "Alice"),
        other => panic!("expected Alice after recreate, got {:?}", other),
    }
}

#[test]
fn test_create_table_many_columns() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute(
        "CREATE TABLE wide (a INT, b INT, c INT, d TEXT, e TEXT, f FLOAT, g FLOAT, h BOOLEAN)"
    ).unwrap();
    exec(&engine, "INSERT INTO wide VALUES (1, 2, 3, 'hello', 'world', 3.14, 2.71, true)");

    let result = engine.execute("SELECT * FROM wide").unwrap();
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].values.len(), 8);
}

#[test]
fn test_table_survives_restart() {
    let dir = TempDir::new().unwrap();

    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE persistent (id INT, data TEXT)");
        exec(&engine, "INSERT INTO persistent VALUES (1, 'survives')");
    }

    // Re-open the engine (simulating restart)
    {
        let engine = make_engine(dir.path());
        let result = engine.execute("SELECT data FROM persistent WHERE id = 1").unwrap();
        assert_eq!(result.rows.len(), 1, "Table and data should persist after restart");
        match result.rows[0].get_by_idx(0) {
            Some(Value::Text(s)) => assert_eq!(s, "survives"),
            other => panic!("expected 'survives', got {:?}", other),
        }
    }
}

#[test]
fn test_create_table_then_alter_via_drop_recreate() {
    // No ALTER TABLE support yet — test the drop+recreate pattern
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");
    exec(&engine, "DROP TABLE t");
    exec(&engine, "CREATE TABLE t (id INT, name TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'test')");

    let result = engine.execute("SELECT name FROM t").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "test"),
        other => panic!("expected 'test', got {:?}", other),
    }
}

#[test]
fn test_alter_table_add_column() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");
    exec(&engine, "INSERT INTO t VALUES (1)");

    engine.execute("ALTER TABLE t ADD COLUMN name TEXT").unwrap();

    let result = engine.execute("SELECT id, name FROM t").unwrap();
    assert_eq!(result.rows.len(), 1);
}

#[test]
fn test_create_table_unique_constraint() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT UNIQUE, name TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'Alice')");

    // Duplicate id=1 should fail
    let result = engine.execute("INSERT INTO t VALUES (1, 'Bob')");
    assert!(result.is_err(), "UNIQUE constraint should reject duplicate id");
    let err = result.unwrap_err();
    assert_eq!(err.sqlstate(), "23505", "UNIQUE violation should be SQLSTATE 23505");
}

#[test]
fn test_create_table_primary_key() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT PRIMARY KEY, name TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'Alice')");

    // Duplicate PK should fail
    let result = engine.execute("INSERT INTO t VALUES (1, 'Bob')");
    assert!(result.is_err(), "PRIMARY KEY should reject duplicate values");
}
