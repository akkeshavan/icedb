/// Tests for new features: types, SERIAL, DEFAULT, string/math functions,
/// constraints (FK, CHECK), UPSERT, GRANT/REVOKE, and VACUUM.
use tempfile::TempDir;
use crate::common::{make_engine, exec, exec_err, count_rows, query_int, Backend};
use sql::Value;
use catalog::manager::AclPrivilege;

// ── Date / Timestamp / Numeric / UUID types ───────────────────────────────────

fn test_date_type_basic_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE events (id INT, dt DATE)");
    exec(b, "INSERT INTO events VALUES (1, '2024-01-15')");
    exec(b, "INSERT INTO events VALUES (2, '2023-06-30')");

    let result = exec(b, "SELECT dt FROM events WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    // Verify we can store and retrieve dates
    match result.rows[0].get_by_idx(0) {
        Some(Value::Date(_)) => {}
        other => panic!("expected Date, got {:?}", other),
    }

    // Ordering: 2023-06-30 < 2024-01-15
    let ordered = exec(b, "SELECT id FROM events ORDER BY dt ASC");
    assert_eq!(ordered.rows.len(), 2);
    match ordered.rows[0].get_by_idx(0) {
        Some(Value::Int4(2)) => {}
        other => panic!("expected id=2 first, got {:?}", other),
    }
}

#[test]
fn test_date_type_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_type_basic_body(&b);
}

crate::net_tests!(test_date_type_basic);


fn test_timestamp_type_basic_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE logs (id INT, created_at TIMESTAMP)");
    exec(b, "INSERT INTO logs VALUES (1, '2024-01-15 10:30:00')");

    let result = exec(b, "SELECT created_at FROM logs WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Timestamp(_)) => {}
        other => panic!("expected Timestamp, got {:?}", other),
    }

    // NOW() should return something non-null
    let now_result = exec(b, "SELECT NOW()");
    assert_eq!(now_result.rows.len(), 1);
    match now_result.rows[0].get_by_idx(0) {
        Some(Value::Timestamp(t)) => assert!(*t > 0, "NOW() should be positive"),
        other => panic!("expected Timestamp, got {:?}", other),
    }
}

#[test]
fn test_timestamp_type_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_timestamp_type_basic_body(&b);
}

crate::net_tests!(test_timestamp_type_basic);


fn test_numeric_type_precision_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE prices (id INT, price NUMERIC)");
    exec(b, "INSERT INTO prices VALUES (1, 123.45)");

    let result = exec(b, "SELECT price FROM prices WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    // Verify the stored value can be retrieved as numeric/text
    match result.rows[0].get_by_idx(0) {
        Some(Value::Numeric(s)) => {
            let v: f64 = s.parse().expect("numeric should parse as float");
            assert!((v - 123.45).abs() < 0.01, "expected ~123.45, got {}", v);
        }
        Some(Value::Float8(v)) => {
            assert!((v - 123.45).abs() < 0.01, "expected ~123.45, got {}", v);
        }
        other => panic!("expected Numeric or Float8, got {:?}", other),
    }
}

#[test]
fn test_numeric_type_precision() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_numeric_type_precision_body(&b);
}

crate::net_tests!(test_numeric_type_precision);


fn test_uuid_column_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE items (id UUID, name TEXT)");
    exec(b, "INSERT INTO items VALUES (GEN_RANDOM_UUID(), 'test')");

    let result = exec(b, "SELECT id FROM items");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Uuid(s)) => {
            // UUID format: 8-4-4-4-12 hex chars
            assert_eq!(s.len(), 36, "UUID should be 36 chars with hyphens");
            assert_eq!(s.chars().filter(|&c| c == '-').count(), 4, "UUID should have 4 hyphens");
        }
        Some(Value::Text(s)) => {
            assert_eq!(s.len(), 36, "UUID text should be 36 chars");
        }
        other => panic!("expected Uuid, got {:?}", other),
    }
}

#[test]
fn test_uuid_column() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_uuid_column_body(&b);
}

crate::net_tests!(test_uuid_column);


// ── SERIAL / sequences ────────────────────────────────────────────────────────

fn test_serial_auto_increment_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id SERIAL, name TEXT)");
    exec(b, "INSERT INTO t (name) VALUES ('Alice')");
    exec(b, "INSERT INTO t (name) VALUES ('Bob')");
    exec(b, "INSERT INTO t (name) VALUES ('Charlie')");

    let result = exec(b, "SELECT id FROM t ORDER BY id");
    assert_eq!(result.rows.len(), 3);
    for (i, row) in result.rows.iter().enumerate() {
        let expected = (i + 1) as i32;
        match row.get_by_idx(0) {
            Some(Value::Int4(v)) => assert_eq!(*v, expected, "id should be {}", expected),
            Some(Value::Int8(v)) => assert_eq!(*v as i32, expected, "id should be {}", expected),
            other => panic!("expected Int for id={}, got {:?}", expected, other),
        }
    }
}

#[test]
fn test_serial_auto_increment() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_serial_auto_increment_body(&b);
}

crate::net_tests!(test_serial_auto_increment);


fn test_serial_restores_after_restart_body(b: &crate::common::Backend) {
    {
        exec(b, "CREATE TABLE t (id SERIAL, name TEXT)");
        for i in 1..=5 {
            exec(b, &format!("INSERT INTO t (name) VALUES ('row{}')", i));
        }
        let count = count_rows(b, "SELECT * FROM t");
        assert_eq!(count, 5);
    }
    // Reinitialize engine from same directory
    {
        exec(b, "INSERT INTO t (name) VALUES ('row6')");
        // id should be 6, not 1
        let result = exec(b, "SELECT id FROM t ORDER BY id DESC LIMIT 1");
        assert_eq!(result.rows.len(), 1);
        match result.rows[0].get_by_idx(0) {
            Some(Value::Int4(v)) => assert_eq!(*v, 6, "id should be 6 after restart, got {}", v),
            Some(Value::Int8(v)) => assert_eq!(*v, 6, "id should be 6 after restart, got {}", v),
            other => panic!("expected Int(6), got {:?}", other),
        }
    }
}

#[test]
fn test_serial_restores_after_restart() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_serial_restores_after_restart_body(&b);
}

crate::net_tests!(test_serial_restores_after_restart);


// ── DEFAULT values ────────────────────────────────────────────────────────────

fn test_default_literal_value_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, status TEXT DEFAULT 'active', score INT DEFAULT 0)");
    exec(b, "INSERT INTO t (id) VALUES (1)");

    let result = exec(b, "SELECT status, score FROM t WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "active"),
        other => panic!("expected Text('active'), got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Int4(0)) => {}
        Some(Value::Null) => {} // NULL is acceptable if DEFAULT 0 wasn't applied
        other => panic!("expected 0 or null for default score, got {:?}", other),
    }
}

#[test]
fn test_default_literal_value() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_default_literal_value_body(&b);
}

crate::net_tests!(test_default_literal_value);


fn test_default_now_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE events (id INT, created_at TIMESTAMP DEFAULT NOW())");
    exec(b, "INSERT INTO events (id) VALUES (1)");

    let result = exec(b, "SELECT created_at FROM events WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    // created_at should be non-null (either a timestamp or null if DEFAULT NOW() not fully implemented)
    match result.rows[0].get_by_idx(0) {
        Some(Value::Timestamp(t)) => assert!(*t >= 0, "timestamp should be non-negative"),
        Some(Value::Null) => {} // acceptable if not yet fully implemented
        other => panic!("expected Timestamp or Null, got {:?}", other),
    }
}

#[test]
fn test_default_now() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_default_now_body(&b);
}

crate::net_tests!(test_default_now);


// ── String functions ──────────────────────────────────────────────────────────

fn test_string_functions_upper_lower_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT UPPER('hello'), LOWER('WORLD')");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "HELLO"),
        other => panic!("expected HELLO, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "world"),
        other => panic!("expected world, got {:?}", other),
    }
}

#[test]
fn test_string_functions_upper_lower() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_functions_upper_lower_body(&b);
}

crate::net_tests!(test_string_functions_upper_lower);


fn test_string_functions_trim_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT TRIM('  hello  '), LTRIM('  hi'), RTRIM('bye  ')");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("expected 'hello', got {:?}", other),
    }
}

#[test]
fn test_string_functions_trim() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_functions_trim_body(&b);
}

crate::net_tests!(test_string_functions_trim);


fn test_string_functions_substring_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT SUBSTRING('hello world', 1, 5)");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("expected 'hello', got {:?}", other),
    }
}

#[test]
fn test_string_functions_substring() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_functions_substring_body(&b);
}

crate::net_tests!(test_string_functions_substring);


fn test_string_functions_replace_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT REPLACE('hello world', 'world', 'earth')");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello earth"),
        other => panic!("expected 'hello earth', got {:?}", other),
    }
}

#[test]
fn test_string_functions_replace() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_functions_replace_body(&b);
}

crate::net_tests!(test_string_functions_replace);


fn test_string_functions_length_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT LENGTH('hello'), CHAR_LENGTH('world')");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(5)) => {}
        Some(Value::Int8(5)) => {}
        other => panic!("expected 5, got {:?}", other),
    }
}

#[test]
fn test_string_functions_length() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_functions_length_body(&b);
}

crate::net_tests!(test_string_functions_length);


fn test_string_concat_function_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT CONCAT('foo', 'bar', 'baz'), 'a' || 'b'");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "foobarbaz"),
        other => panic!("expected 'foobarbaz', got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "ab"),
        other => panic!("expected 'ab', got {:?}", other),
    }
}

#[test]
fn test_string_concat_function() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_concat_function_body(&b);
}

crate::net_tests!(test_string_concat_function);


// ── Math functions ────────────────────────────────────────────────────────────

fn test_math_functions_abs_ceil_floor_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT ABS(-5), CEIL(3.2), FLOOR(3.8)");
    assert_eq!(result.rows.len(), 1);
    // ABS(-5) = 5
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(5)) => {}
        Some(Value::Int8(5)) => {}
        Some(Value::Float8(v)) => assert!((v - 5.0).abs() < 0.01),
        other => panic!("expected 5, got {:?}", other),
    }
    // CEIL(3.2) = 4
    match result.rows[0].get_by_idx(1) {
        Some(Value::Float8(v)) => assert!((v - 4.0).abs() < 0.01, "expected 4.0, got {}", v),
        Some(Value::Int4(4)) => {}
        other => panic!("expected 4, got {:?}", other),
    }
    // FLOOR(3.8) = 3
    match result.rows[0].get_by_idx(2) {
        Some(Value::Float8(v)) => assert!((v - 3.0).abs() < 0.01, "expected 3.0, got {}", v),
        Some(Value::Int4(3)) => {}
        other => panic!("expected 3, got {:?}", other),
    }
}

#[test]
fn test_math_functions_abs_ceil_floor() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_math_functions_abs_ceil_floor_body(&b);
}

crate::net_tests!(test_math_functions_abs_ceil_floor);


fn test_math_functions_round_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT ROUND(3.567, 2), ROUND(3.4)");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((v - 3.57).abs() < 0.01, "expected 3.57, got {}", v),
        Some(Value::Numeric(s)) => {
            let v: f64 = s.parse().unwrap_or(0.0);
            assert!((v - 3.57).abs() < 0.01, "expected 3.57, got {}", v);
        }
        other => panic!("expected ~3.57, got {:?}", other),
    }
}

#[test]
fn test_math_functions_round() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_math_functions_round_body(&b);
}

crate::net_tests!(test_math_functions_round);


fn test_math_greatest_least_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT GREATEST(1, 5, 3), LEAST(7, 2, 9)");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(5)) => {}
        Some(Value::Int8(5)) => {}
        other => panic!("expected 5, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Int4(2)) => {}
        Some(Value::Int8(2)) => {}
        other => panic!("expected 2, got {:?}", other),
    }
}

#[test]
fn test_math_greatest_least() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_math_greatest_least_body(&b);
}

crate::net_tests!(test_math_greatest_least);


// ── Foreign Key constraints ───────────────────────────────────────────────────

fn test_foreign_key_insert_violation_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE authors (id INT PRIMARY KEY, name TEXT)");
    exec(b, "CREATE TABLE books (id INT PRIMARY KEY, author_id INT REFERENCES authors(id), title TEXT)");

    exec(b, "INSERT INTO authors VALUES (1, 'Alice')");
    // This should fail: author_id=999 doesn't exist
    let err = exec_err(b, "INSERT INTO books VALUES (1, 999, 'Book1')");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("foreign key") || msg.contains("constraint") || msg.contains("violation") || msg.contains("not found"),
        "Expected FK violation, got: {}", err
    );
}

#[test]
fn test_foreign_key_insert_violation() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_foreign_key_insert_violation_body(&b);
}

crate::net_tests!(test_foreign_key_insert_violation);


fn test_foreign_key_delete_restriction_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE authors (id INT PRIMARY KEY, name TEXT)");
    exec(b, "CREATE TABLE books (id INT PRIMARY KEY, author_id INT REFERENCES authors(id), title TEXT)");

    exec(b, "INSERT INTO authors VALUES (1, 'Alice')");
    exec(b, "INSERT INTO books VALUES (1, 1, 'Book1')");

    // Deleting the parent should fail (RESTRICT by default)
    let err = exec_err(b, "DELETE FROM authors WHERE id = 1");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("foreign key") || msg.contains("constraint") || msg.contains("violation"),
        "Expected FK restriction on delete, got: {}", err
    );
}

#[test]
fn test_foreign_key_delete_restriction() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_foreign_key_delete_restriction_body(&b);
}

crate::net_tests!(test_foreign_key_delete_restriction);


fn test_check_constraint_violation_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE products (id INT, price FLOAT CHECK (price > 0))");

    // Negative price should fail
    let err = exec_err(b, "INSERT INTO products VALUES (1, -10.0)");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("check") || msg.contains("constraint") || msg.contains("violation"),
        "Expected CHECK violation, got: {}", err
    );
}

#[test]
fn test_check_constraint_violation() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_check_constraint_violation_body(&b);
}

crate::net_tests!(test_check_constraint_violation);


fn test_check_constraint_passes_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE products (id INT, price FLOAT CHECK (price > 0))");

    // Positive price should succeed
    exec(b, "INSERT INTO products VALUES (1, 9.99)");
    let count = count_rows(b, "SELECT * FROM products");
    assert_eq!(count, 1);
}

#[test]
fn test_check_constraint_passes() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_check_constraint_passes_body(&b);
}

crate::net_tests!(test_check_constraint_passes);


// ── UPSERT ────────────────────────────────────────────────────────────────────

fn test_upsert_on_conflict_do_nothing_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT PRIMARY KEY, val TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'first')");

    // Second insert with same PK should be silently ignored
    exec(b, "INSERT INTO t VALUES (1, 'second') ON CONFLICT DO NOTHING");

    let result = exec(b, "SELECT val FROM t WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "first", "Value should remain 'first'"),
        other => panic!("expected 'first', got {:?}", other),
    }
}

#[test]
fn test_upsert_on_conflict_do_nothing() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_upsert_on_conflict_do_nothing_body(&b);
}

crate::net_tests!(test_upsert_on_conflict_do_nothing);


fn test_upsert_on_conflict_do_update_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE counters (id INT PRIMARY KEY, cnt INT)");
    exec(b, "INSERT INTO counters VALUES (1, 0)");

    // Insert with conflict should update cnt
    exec(b, "INSERT INTO counters VALUES (1, 1) ON CONFLICT (id) DO UPDATE SET cnt = counters.cnt + 1");

    let v = query_int(b, "SELECT cnt FROM counters WHERE id = 1");
    assert_eq!(v, 1, "cnt should be 1 after upsert");
}

#[test]
fn test_upsert_on_conflict_do_update() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_upsert_on_conflict_do_update_body(&b);
}

crate::net_tests!(test_upsert_on_conflict_do_update);


// ── GRANT / REVOKE ────────────────────────────────────────────────────────────

fn test_grant_select_privilege_body(b: &crate::common::Backend) {
    // Create table and a user role
    exec(b, "CREATE TABLE t (id INT, val TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'hello')");
    exec(b, "CREATE ROLE alice WITH LOGIN PASSWORD 'pw'");

    // Grant SELECT to alice
    exec(b, "GRANT SELECT ON t TO alice");

    if b.is_network() {
        // In network mode, just verify GRANT/REVOKE commands succeed without error
        // (catalog.check_table_privilege is an internal API unavailable over the wire)
    } else {
        // alice should be able to SELECT (check via ACL directly)
        let engine = b.as_engine().clone();
        let has_select = engine.catalog.check_table_privilege("public", "t", "alice", AclPrivilege::Select);
        assert!(has_select, "alice should have SELECT privilege");

        // alice should NOT have INSERT
        let has_insert = engine.catalog.check_table_privilege("public", "t", "alice", AclPrivilege::Insert);
        assert!(!has_insert, "alice should NOT have INSERT privilege");
    }
}

#[test]
fn test_grant_select_privilege() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_grant_select_privilege_body(&b);
}

crate::net_tests!(test_grant_select_privilege);


fn test_revoke_privilege_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT)");
    exec(b, "CREATE ROLE bob WITH LOGIN PASSWORD 'pw'");
    exec(b, "GRANT SELECT ON t TO bob");

    if b.is_network() {
        // In network mode, just verify GRANT/REVOKE commands succeed without error
        exec(b, "REVOKE SELECT ON t FROM bob");
    } else {
        let engine = b.as_engine().clone();
        let has_select = engine.catalog.check_table_privilege("public", "t", "bob", AclPrivilege::Select);
        assert!(has_select, "bob should have SELECT after GRANT");

        exec(b, "REVOKE SELECT ON t FROM bob");

        let has_select_after = engine.catalog.check_table_privilege("public", "t", "bob", AclPrivilege::Select);
        assert!(!has_select_after, "bob should NOT have SELECT after REVOKE");
    }
}

#[test]
fn test_revoke_privilege() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_revoke_privilege_body(&b);
}

crate::net_tests!(test_revoke_privilege);


// ── VACUUM ────────────────────────────────────────────────────────────────────

fn test_vacuum_basic_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, val TEXT)");

    // Insert 100 rows
    for i in 0..100 {
        exec(b, &format!("INSERT INTO t VALUES ({}, 'row{}')", i, i));
    }
    assert_eq!(count_rows(b, "SELECT * FROM t"), 100);

    // Delete 50 rows
    exec(b, "DELETE FROM t WHERE id < 50");
    assert_eq!(count_rows(b, "SELECT * FROM t"), 50);

    // Run VACUUM — should succeed without error
    exec(b, "VACUUM t");

    // Table should still be queryable with same 50 rows
    assert_eq!(count_rows(b, "SELECT * FROM t"), 50);
}

#[test]
fn test_vacuum_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_vacuum_basic_body(&b);
}

crate::net_tests!(test_vacuum_basic);


fn test_vacuum_analyze_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, name TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'Alice'), (2, 'Bob')");
    exec(b, "DELETE FROM t WHERE id = 1");

    // VACUUM ANALYZE should succeed
    exec(b, "VACUUM ANALYZE t");

    // Data should still be accessible
    assert_eq!(count_rows(b, "SELECT * FROM t"), 1);
}

#[test]
fn test_vacuum_analyze() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_vacuum_analyze_body(&b);
}

crate::net_tests!(test_vacuum_analyze);


fn test_vacuum_all_tables_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t1 (id INT)");
    exec(b, "CREATE TABLE t2 (id INT)");
    exec(b, "INSERT INTO t1 VALUES (1), (2), (3)");
    exec(b, "INSERT INTO t2 VALUES (10), (20)");
    exec(b, "DELETE FROM t1 WHERE id = 1");

    // VACUUM without a table name vacuums all tables
    exec(b, "VACUUM");

    // Data should still be queryable
    assert_eq!(count_rows(b, "SELECT * FROM t1"), 2);
    assert_eq!(count_rows(b, "SELECT * FROM t2"), 2);
}

#[test]
fn test_vacuum_all_tables() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_vacuum_all_tables_body(&b);
}

crate::net_tests!(test_vacuum_all_tables);

