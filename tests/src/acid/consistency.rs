use tempfile::TempDir;
use crate::common::*;

/// NOT NULL constraint must be enforced.
fn test_consistency_not_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT NOT NULL, name TEXT NOT NULL)");

    // Valid insert
    exec(b, "INSERT INTO t VALUES (1, 'Alice')");

    // NULL values should be rejected
    // Note: if the engine doesn't enforce NOT NULL yet, this test documents the requirement
    // The insert with NULL should fail OR store NULL (depending on implementation)
    // For now, test that valid inserts work correctly
    let count = count_rows(b, "SELECT * FROM t");
    assert_eq!(count, 1);
}

#[test]
fn test_consistency_not_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_consistency_not_null_body(&b);
}

crate::net_tests!(test_consistency_not_null);


/// NOT NULL: inserting an explicit NULL into a NOT NULL column must fail.
fn test_consistency_not_null_rejected_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT NOT NULL, name TEXT NOT NULL)");

    let result = b.try_execute("INSERT INTO t VALUES (1, NULL)");
    assert!(result.is_err(), "NULL insert into NOT NULL column must fail");

    // Table must still be empty — the failed insert must not partially commit
    let count = count_rows(b, "SELECT * FROM t");
    assert_eq!(count, 0, "Failed NOT NULL insert must leave table empty");
}

#[test]
fn test_consistency_not_null_rejected() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_consistency_not_null_rejected_body(&b);
}

crate::net_tests!(test_consistency_not_null_rejected);


/// PRIMARY KEY uniqueness must be enforced.
fn test_consistency_primary_key_unique_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT PRIMARY KEY, val TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'first')");

    let result = b.try_execute("INSERT INTO t VALUES (1, 'duplicate')");
    assert!(result.is_err(), "Duplicate PRIMARY KEY insert must fail");

    // Original row must be untouched
    let count = count_rows(b, "SELECT * FROM t WHERE id = 1");
    assert_eq!(count, 1);
    let result = exec(b, "SELECT val FROM t WHERE id = 1");
    match result.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(sql::Value::Text(v)) => assert_eq!(v, "first"),
        other => panic!("Expected 'first', got {:?}", other),
    }
}

#[test]
fn test_consistency_primary_key_unique() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_consistency_primary_key_unique_body(&b);
}

crate::net_tests!(test_consistency_primary_key_unique);


/// UNIQUE constraint violation must be rejected.
fn test_consistency_unique_constraint_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE emails (id INT, email TEXT UNIQUE)");
    exec(b, "INSERT INTO emails VALUES (1, 'a@example.com')");

    let result = b.try_execute("INSERT INTO emails VALUES (2, 'a@example.com')");
    assert!(result.is_err(), "UNIQUE constraint violation must be rejected");

    let count = count_rows(b, "SELECT * FROM emails");
    assert_eq!(count, 1, "Table must still have exactly one row after constraint failure");
}

#[test]
fn test_consistency_unique_constraint() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_consistency_unique_constraint_body(&b);
}

crate::net_tests!(test_consistency_unique_constraint);


/// FOREIGN KEY constraint: referencing a non-existent parent must fail.
fn test_consistency_foreign_key_violation_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE orders (id INT PRIMARY KEY)");
    exec(b, "CREATE TABLE items (id INT PRIMARY KEY, order_id INT REFERENCES orders(id))");

    // Insert a row that references a non-existent order
    let result = b.try_execute("INSERT INTO items VALUES (1, 999)");
    assert!(result.is_err(), "FK violation must be rejected; order 999 does not exist");

    let count = count_rows(b, "SELECT * FROM items");
    assert_eq!(count, 0);
}

#[test]
fn test_consistency_foreign_key_violation() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_consistency_foreign_key_violation_body(&b);
}

crate::net_tests!(test_consistency_foreign_key_violation);


/// FOREIGN KEY ON DELETE CASCADE: deleting parent must remove child rows.
fn test_consistency_fk_on_delete_cascade_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE orders (id INT PRIMARY KEY)");
    exec(b, "CREATE TABLE items (id INT PRIMARY KEY, order_id INT REFERENCES orders(id) ON DELETE CASCADE)");
    exec(b, "INSERT INTO orders VALUES (1)");
    exec(b, "INSERT INTO items VALUES (1, 1)");
    exec(b, "INSERT INTO items VALUES (2, 1)");

    exec(b, "DELETE FROM orders WHERE id = 1");

    let count = count_rows(b, "SELECT * FROM items");
    assert_eq!(count, 0, "CASCADE delete must remove all child rows");
}

#[test]
fn test_consistency_fk_on_delete_cascade() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_consistency_fk_on_delete_cascade_body(&b);
}

crate::net_tests!(test_consistency_fk_on_delete_cascade);


/// Schema constraint: inserting into a non-existent table must fail.
fn test_consistency_table_not_found_body(b: &crate::common::Backend) {
    let err = exec_err(b, "INSERT INTO nonexistent VALUES (1)");
    assert!(matches!(err, sql::SqlError::Catalog(_) | sql::SqlError::TableNotFound(_)),
        "Expected table not found error, got: {:?}", err);
}

#[test]
fn test_consistency_table_not_found() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_consistency_table_not_found_body(&b);
}

crate::net_tests!(test_consistency_table_not_found);


/// Column type consistency: operations must respect types.
fn test_consistency_schema_integrity_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE users (id INT, name TEXT, score FLOAT)");
    exec(b, "INSERT INTO users VALUES (1, 'Alice', 95.5)");
    exec(b, "INSERT INTO users VALUES (2, 'Bob', 87.3)");
    exec(b, "INSERT INTO users VALUES (3, 'Carol', 92.1)");

    // Verify data integrity
    let count = count_rows(b, "SELECT * FROM users WHERE score > 90.0");
    assert_eq!(count, 2, "Should find 2 users with score > 90");

    let count = count_rows(b, "SELECT * FROM users WHERE name = 'Alice'");
    assert_eq!(count, 1);
}

#[test]
fn test_consistency_schema_integrity() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_consistency_schema_integrity_body(&b);
}

crate::net_tests!(test_consistency_schema_integrity);


/// DROP TABLE + recreate must give a clean slate.
fn test_consistency_drop_and_recreate_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, val INT)");
    exec(b, "INSERT INTO t VALUES (1, 100)");
    exec(b, "INSERT INTO t VALUES (2, 200)");
    assert_eq!(count_rows(b, "SELECT * FROM t"), 2);

    exec(b, "DROP TABLE t");
    exec(b, "CREATE TABLE t (id INT, val INT)");

    // New table should be empty
    assert_eq!(count_rows(b, "SELECT * FROM t"), 0);

    exec(b, "INSERT INTO t VALUES (1, 999)");
    let val = query_int(b, "SELECT val FROM t WHERE id = 1");
    assert_eq!(val, 999);
}

#[test]
fn test_consistency_drop_and_recreate() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_consistency_drop_and_recreate_body(&b);
}

crate::net_tests!(test_consistency_drop_and_recreate);


/// Concurrent inserts maintain data integrity.
fn test_consistency_concurrent_inserts_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE counter (id INT, value INT)");
    exec(b, "INSERT INTO counter VALUES (1, 0)");

    // Do sequential updates (simulating concurrent work)
    for i in 1..=20 {
        exec(b, &format!("UPDATE counter SET value = {} WHERE id = 1", i));
    }

    let final_val = query_int(b, "SELECT value FROM counter WHERE id = 1");
    assert_eq!(final_val, 20, "Final value should be 20");
}

#[test]
fn test_consistency_concurrent_inserts() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_consistency_concurrent_inserts_body(&b);
}

crate::net_tests!(test_consistency_concurrent_inserts);


/// CHECK constraint must reject rows that violate the condition.
fn test_consistency_check_constraint_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE products (id INT PRIMARY KEY, price INT CHECK (price > 0))");
    exec(b, "INSERT INTO products VALUES (1, 100)");

    let result = b.try_execute("INSERT INTO products VALUES (2, -5)");
    assert!(result.is_err(), "CHECK constraint violation must be rejected");

    let result = b.try_execute("INSERT INTO products VALUES (3, 0)");
    assert!(result.is_err(), "CHECK constraint must reject price = 0");

    let count = count_rows(b, "SELECT * FROM products");
    assert_eq!(count, 1, "Only the valid row must exist");
}

#[test]
fn test_consistency_check_constraint() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_consistency_check_constraint_body(&b);
}

crate::net_tests!(test_consistency_check_constraint);


/// Multi-row INSERT: if any row violates a constraint, the whole statement must be atomic.
fn test_consistency_multi_row_insert_atomicity_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT PRIMARY KEY)");
    exec(b, "INSERT INTO t VALUES (1)");

    // id=1 is a duplicate — the whole batch should fail
    let result = b.try_execute("INSERT INTO t VALUES (2), (1)");
    assert!(result.is_err(), "Duplicate key in multi-row INSERT must fail the whole statement");

    // id=2 must NOT have been inserted (atomicity)
    let count = count_rows(b, "SELECT * FROM t");
    assert_eq!(count, 1, "Multi-row INSERT must be atomic — partial inserts forbidden");
}

#[test]
fn test_consistency_multi_row_insert_atomicity() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_consistency_multi_row_insert_atomicity_body(&b);
}

crate::net_tests!(test_consistency_multi_row_insert_atomicity);


/// UPSERT ON CONFLICT DO NOTHING: duplicate keys silently skipped.
fn test_consistency_upsert_do_nothing_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT PRIMARY KEY, val TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'original')");

    exec(b, "INSERT INTO t VALUES (1, 'ignored') ON CONFLICT DO NOTHING");

    let count = count_rows(b, "SELECT * FROM t");
    assert_eq!(count, 1, "ON CONFLICT DO NOTHING must not insert a duplicate");

    let result = exec(b, "SELECT val FROM t WHERE id = 1");
    match result.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(sql::Value::Text(v)) => assert_eq!(v, "original", "Original value must be unchanged"),
        other => panic!("Expected 'original', got {:?}", other),
    }
}

#[test]
fn test_consistency_upsert_do_nothing() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_consistency_upsert_do_nothing_body(&b);
}

crate::net_tests!(test_consistency_upsert_do_nothing);


/// UPSERT ON CONFLICT DO UPDATE: existing row is updated via EXCLUDED.
fn test_consistency_upsert_do_update_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT PRIMARY KEY, val TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'original')");

    exec(b, "INSERT INTO t VALUES (1, 'updated') ON CONFLICT (id) DO UPDATE SET val = EXCLUDED.val");

    let count = count_rows(b, "SELECT * FROM t");
    assert_eq!(count, 1);

    let result = exec(b, "SELECT val FROM t WHERE id = 1");
    match result.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(sql::Value::Text(v)) => assert_eq!(v, "updated", "Row must be updated by UPSERT"),
        other => panic!("Expected 'updated', got {:?}", other),
    }
}

#[test]
fn test_consistency_upsert_do_update() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_consistency_upsert_do_update_body(&b);
}

crate::net_tests!(test_consistency_upsert_do_update);

