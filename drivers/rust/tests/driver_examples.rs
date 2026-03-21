/// Integration tests for the icedb Rust driver.
/// Exercises every API shown in ch09-drivers.md and verifies correct behaviour.
use std::sync::Arc;
use tempfile::tempdir;

use icedb_driver::{open, open_pool, Connection, DriverError};
use sql::Value;

// ---------------------------------------------------------------------------
// 1. open() + direct engine execute (no Connection wrapper)
// ---------------------------------------------------------------------------
#[test]
fn test_open_and_direct_execute() {
    let dir = tempdir().expect("tempdir");
    let engine = open(dir.path()).expect("open");

    engine.execute("CREATE TABLE books (id INT NOT NULL, title TEXT NOT NULL, price FLOAT)").expect("create table");
    engine.execute("INSERT INTO books VALUES (1, 'The Hobbit', 12.99)").expect("insert 1");
    engine.execute("INSERT INTO books VALUES (2, 'Dune', 15.99)").expect("insert 2");

    let result = engine.execute("SELECT * FROM books WHERE price < 15.00").expect("select");
    assert_eq!(result.rows.len(), 1, "expected 1 row with price < 15");
    assert_eq!(result.rows[0].get("id"), Some(&Value::Int4(1)));
    assert_eq!(result.rows[0].get("title"), Some(&Value::Text("The Hobbit".to_string())));
    assert_eq!(result.rows[0].get("price"), Some(&Value::Float8(12.99)));
}

// ---------------------------------------------------------------------------
// 2. Connection::new + execute (as shown in ch09 "Running Queries")
// ---------------------------------------------------------------------------
#[test]
fn test_connection_execute_query() {
    let dir = tempdir().expect("tempdir");
    let engine = open(dir.path()).expect("open");

    let conn = Connection::new(Arc::clone(&engine));
    conn.execute("CREATE TABLE items (id INT NOT NULL, label TEXT NOT NULL)").expect("ddl");
    conn.execute("INSERT INTO items VALUES (10, 'apple')").expect("insert");
    conn.execute("INSERT INTO items VALUES (20, 'banana')").expect("insert");

    let result = conn.execute("SELECT id, label FROM items ORDER BY id").expect("select");
    assert_eq!(result.rows.len(), 2);
    assert_eq!(result.rows[0].get("id"), Some(&Value::Int4(10)));
    assert_eq!(result.rows[0].get("label"), Some(&Value::Text("apple".to_string())));
    assert_eq!(result.rows[1].get("id"), Some(&Value::Int4(20)));
}

// ---------------------------------------------------------------------------
// 3. Type mapping: BOOLEAN, INT, BIGINT, FLOAT, TEXT, NULL
// ---------------------------------------------------------------------------
#[test]
fn test_type_mapping() {
    let dir = tempdir().expect("tempdir");
    let engine = open(dir.path()).expect("open");

    engine.execute(
        "CREATE TABLE types_test (b BOOLEAN, i INT, bi BIGINT, f FLOAT, t TEXT)"
    ).expect("create");

    engine.execute(
        "INSERT INTO types_test VALUES (true, 42, 9876543210, 3.14, 'hello')"
    ).expect("insert");

    let result = engine.execute("SELECT * FROM types_test").expect("select");
    assert_eq!(result.rows.len(), 1);
    let row = &result.rows[0];
    assert_eq!(row.get("b"), Some(&Value::Bool(true)));
    assert_eq!(row.get("i"), Some(&Value::Int4(42)));
    assert_eq!(row.get("bi"), Some(&Value::Int8(9876543210)));
    assert_eq!(row.get("t"), Some(&Value::Text("hello".to_string())));
    // Float8 comparison with tolerance
    if let Some(Value::Float8(f)) = row.get("f") {
        assert!((*f - 3.14).abs() < 1e-10, "expected 3.14, got {f}");
    } else {
        panic!("expected Float8 for f column");
    }
}

// ---------------------------------------------------------------------------
// 4. Explicit transaction: begin → execute_in_txn → commit
// ---------------------------------------------------------------------------
#[test]
fn test_explicit_transaction_commit() {
    let dir = tempdir().expect("tempdir");
    let engine = open(dir.path()).expect("open");
    engine.execute("CREATE TABLE accounts (id INT, balance FLOAT)").expect("create");
    engine.execute("INSERT INTO accounts VALUES (1, 1000.0)").expect("insert");
    engine.execute("INSERT INTO accounts VALUES (2, 500.0)").expect("insert");

    let mut conn = Connection::new(Arc::clone(&engine));
    let xid = conn.begin().expect("begin");
    conn.execute_in_txn(
        "UPDATE accounts SET balance = balance - 200.0 WHERE id = 1",
        xid,
    ).expect("debit");
    conn.execute_in_txn(
        "UPDATE accounts SET balance = balance + 200.0 WHERE id = 2",
        xid,
    ).expect("credit");
    conn.commit().expect("commit");

    // Verify the committed values
    let result = engine.execute("SELECT id, balance FROM accounts ORDER BY id").expect("select");
    assert_eq!(result.rows.len(), 2);
    if let Some(Value::Float8(b1)) = result.rows[0].get("balance") {
        assert!((*b1 - 800.0).abs() < 1e-9, "expected 800.0, got {b1}");
    } else {
        panic!("expected Float8 for account 1 balance");
    }
    if let Some(Value::Float8(b2)) = result.rows[1].get("balance") {
        assert!((*b2 - 700.0).abs() < 1e-9, "expected 700.0, got {b2}");
    } else {
        panic!("expected Float8 for account 2 balance");
    }
}

// ---------------------------------------------------------------------------
// 5. Explicit transaction: begin → execute_in_txn → rollback
// ---------------------------------------------------------------------------
#[test]
fn test_explicit_transaction_rollback() {
    let dir = tempdir().expect("tempdir");
    let engine = open(dir.path()).expect("open");
    engine.execute("CREATE TABLE events (id INT, kind TEXT)").expect("create");

    let mut conn = Connection::new(Arc::clone(&engine));
    let xid = conn.begin().expect("begin");
    conn.execute_in_txn("INSERT INTO events VALUES (1, 'click')", xid).expect("insert");
    conn.rollback().expect("rollback");

    let result = engine.execute("SELECT COUNT(*) FROM events").expect("select");
    // After rollback the row should not be visible
    // COUNT(*) returns Int8 or Int4 depending on implementation
    let count_val = result.rows[0].get_by_idx(0).expect("count value");
    let count: i64 = match count_val {
        Value::Int4(n) => *n as i64,
        Value::Int8(n) => *n,
        other => panic!("unexpected count type: {other:?}"),
    };
    assert_eq!(count, 0, "expected 0 rows after rollback");
}

// ---------------------------------------------------------------------------
// 6. Auto-rollback on drop (Connection dropped mid-transaction)
// ---------------------------------------------------------------------------
#[test]
fn test_auto_rollback_on_drop() {
    let dir = tempdir().expect("tempdir");
    let engine = open(dir.path()).expect("open");
    engine.execute("CREATE TABLE log (msg TEXT)").expect("create");

    {
        let mut conn = Connection::new(Arc::clone(&engine));
        let xid = conn.begin().expect("begin");
        conn.execute_in_txn("INSERT INTO log VALUES ('should not appear')", xid).expect("insert");
        // conn dropped here without commit → auto-rollback
    }

    let result = engine.execute("SELECT * FROM log").expect("select");
    assert_eq!(result.rows.len(), 0, "row should not be visible after auto-rollback on drop");
}

// ---------------------------------------------------------------------------
// 7. open_pool + PooledConnection
// ---------------------------------------------------------------------------
#[test]
fn test_pool_acquire_execute() {
    let dir = tempdir().expect("tempdir");
    let pool = open_pool(dir.path(), 10).expect("open_pool");

    // Setup via pool connection
    pool.acquire().expect("acquire").execute("CREATE TABLE events (user_id INT, kind TEXT)")
        .expect("create");

    // Insert via pooled connection — matches ch09 "Connection Pool" example
    {
        let conn = pool.acquire().expect("acquire");
        conn.execute("INSERT INTO events (user_id, kind) VALUES (42, 'login')").expect("insert");
        // conn returns to pool on drop
    }

    // Verify
    let conn = pool.acquire().expect("acquire");
    let result = conn.execute("SELECT * FROM events").expect("select");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].get("user_id"), Some(&Value::Int4(42)));
    assert_eq!(result.rows[0].get("kind"), Some(&Value::Text("login".to_string())));
}

// ---------------------------------------------------------------------------
// 8. Pool exhausted error
// ---------------------------------------------------------------------------
#[test]
fn test_pool_exhausted() {
    let dir = tempdir().expect("tempdir");
    let pool = open_pool(dir.path(), 1).expect("open_pool");

    let _c1 = pool.acquire().expect("first acquire");
    let result = pool.acquire();
    assert!(
        matches!(result, Err(DriverError::PoolExhausted)),
        "expected DriverError::PoolExhausted"
    );
}

// ---------------------------------------------------------------------------
// 9. Error handling: query on non-existent table
// ---------------------------------------------------------------------------
#[test]
fn test_error_handling() {
    let dir = tempdir().expect("tempdir");
    let engine = open(dir.path()).expect("open");
    let conn = Connection::new(Arc::clone(&engine));

    let result = conn.execute("SELECT * FROM no_such_table");
    assert!(result.is_err(), "expected error for query on non-existent table");
    match result {
        Err(DriverError::Sql(_)) => {} // expected: SQL engine error
        Err(other) => panic!("unexpected error variant: {other:?}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

// ---------------------------------------------------------------------------
// 10. set_isolation_level before begin (RepeatableRead)
// ---------------------------------------------------------------------------
#[test]
fn test_isolation_level_set() {
    use txn::transaction::IsolationLevel;

    let dir = tempdir().expect("tempdir");
    let engine = open(dir.path()).expect("open");
    engine.execute("CREATE TABLE counter (n INT)").expect("create");
    engine.execute("INSERT INTO counter VALUES (0)").expect("insert");

    let mut conn = Connection::new(Arc::clone(&engine));
    conn.set_isolation_level(IsolationLevel::RepeatableRead);
    let xid = conn.begin().expect("begin with RepeatableRead");
    let result = conn.execute_in_txn("SELECT n FROM counter", xid).expect("select");
    assert_eq!(result.rows.len(), 1);
    conn.commit().expect("commit");
}
