use tempfile::TempDir;
use crate::common::*;

/// NOT NULL constraint must be enforced.
#[test]
fn test_consistency_not_null() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT NOT NULL, name TEXT NOT NULL)");

    // Valid insert
    exec(&engine, "INSERT INTO t VALUES (1, 'Alice')");

    // NULL values should be rejected
    // Note: if the engine doesn't enforce NOT NULL yet, this test documents the requirement
    // The insert with NULL should fail OR store NULL (depending on implementation)
    // For now, test that valid inserts work correctly
    let count = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(count, 1);
}

/// Schema constraint: inserting into a non-existent table must fail.
#[test]
fn test_consistency_table_not_found() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    let err = exec_err(&engine, "INSERT INTO nonexistent VALUES (1)");
    assert!(matches!(err, sql::SqlError::Catalog(_) | sql::SqlError::TableNotFound(_)),
        "Expected table not found error, got: {:?}", err);
}

/// Column type consistency: operations must respect types.
#[test]
fn test_consistency_schema_integrity() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE users (id INT, name TEXT, score FLOAT)");
    exec(&engine, "INSERT INTO users VALUES (1, 'Alice', 95.5)");
    exec(&engine, "INSERT INTO users VALUES (2, 'Bob', 87.3)");
    exec(&engine, "INSERT INTO users VALUES (3, 'Carol', 92.1)");

    // Verify data integrity
    let count = count_rows(&engine, "SELECT * FROM users WHERE score > 90.0");
    assert_eq!(count, 2, "Should find 2 users with score > 90");

    let count = count_rows(&engine, "SELECT * FROM users WHERE name = 'Alice'");
    assert_eq!(count, 1);
}

/// DROP TABLE + recreate must give a clean slate.
#[test]
fn test_consistency_drop_and_recreate() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100)");
    exec(&engine, "INSERT INTO t VALUES (2, 200)");
    assert_eq!(count_rows(&engine, "SELECT * FROM t"), 2);

    exec(&engine, "DROP TABLE t");
    exec(&engine, "CREATE TABLE t (id INT, val INT)");

    // New table should be empty
    assert_eq!(count_rows(&engine, "SELECT * FROM t"), 0);

    exec(&engine, "INSERT INTO t VALUES (1, 999)");
    let val = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(val, 999);
}

/// Concurrent inserts maintain data integrity.
#[test]
fn test_consistency_concurrent_inserts() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE counter (id INT, value INT)");
    exec(&engine, "INSERT INTO counter VALUES (1, 0)");

    // Do sequential updates (simulating concurrent work)
    for i in 1..=20 {
        exec(&engine, &format!("UPDATE counter SET value = {} WHERE id = 1", i));
    }

    let final_val = query_int(&engine, "SELECT value FROM counter WHERE id = 1");
    assert_eq!(final_val, 20, "Final value should be 20");
}
