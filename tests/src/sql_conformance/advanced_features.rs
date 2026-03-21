/// Tests for Tier 2 advanced features:
/// - Autovacuum daemon API
/// - LISTEN / NOTIFY
/// - CREATE FUNCTION (SQL language)
/// - Cost-based optimizer (IndexScan selection)
/// - pg_dump / pg_restore
use tempfile::TempDir;

use crate::common::{make_engine, exec};
use sql::Value;

// ── LISTEN / NOTIFY ───────────────────────────────────────────────────────────

#[test]
fn test_notify_basic() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    // NOTIFY should succeed without error
    engine.execute("NOTIFY my_channel, 'hello'").unwrap();
}

#[test]
fn test_notify_no_payload() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("NOTIFY events").unwrap();
}

#[test]
fn test_listen_basic() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("LISTEN my_channel").unwrap();
    engine.execute("UNLISTEN my_channel").unwrap();
}

#[test]
fn test_listen_notify_roundtrip() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    // Subscribe to channel via catalog directly
    let rx = engine.catalog.listen("test_chan");
    // Send notification
    engine.execute("NOTIFY test_chan, 'payload123'").unwrap();
    // Receive the notification
    let msg = rx.try_recv();
    assert!(msg.is_ok(), "expected a notification message");
    assert_eq!(msg.unwrap(), "payload123");
}

#[test]
fn test_unlisten_star() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("LISTEN chan1").unwrap();
    engine.execute("LISTEN chan2").unwrap();
    engine.execute("UNLISTEN *").unwrap();
}

// ── CREATE FUNCTION ───────────────────────────────────────────────────────────

#[test]
fn test_create_sql_function_basic() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute(
        "CREATE FUNCTION add_two(a INT, b INT) RETURNS INT LANGUAGE SQL AS $$ SELECT $1 + $2 $$"
    ).unwrap();
}

#[test]
fn test_create_and_call_sql_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute(
        "CREATE FUNCTION add_two(a INT, b INT) RETURNS INT LANGUAGE SQL AS $$ SELECT $1 + $2 $$"
    ).unwrap();
    let result = engine.execute("SELECT add_two(3, 4)").unwrap();
    assert_eq!(result.rows.len(), 1);
    let val = result.rows[0].get_by_idx(0).cloned().unwrap();
    match val {
        Value::Int4(7) | Value::Int8(7) => {}
        other => panic!("expected 7, got {:?}", other),
    }
}

#[test]
fn test_drop_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute(
        "CREATE FUNCTION temp_fn() RETURNS INT LANGUAGE SQL AS $$ SELECT 42 $$"
    ).unwrap();
    engine.execute("DROP FUNCTION temp_fn").unwrap();
}

#[test]
fn test_drop_function_if_exists() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    // Dropping a non-existent function with IF EXISTS should not error
    engine.execute("DROP FUNCTION IF EXISTS nonexistent_fn").unwrap();
}

#[test]
fn test_sql_function_constant() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute(
        "CREATE FUNCTION get_answer() RETURNS INT LANGUAGE SQL AS $$ SELECT 42 $$"
    ).unwrap();
    let result = engine.execute("SELECT get_answer()").unwrap();
    assert_eq!(result.rows.len(), 1);
    let val = result.rows[0].get_by_idx(0).cloned().unwrap();
    match val {
        Value::Int4(42) | Value::Int8(42) => {}
        other => panic!("expected 42, got {:?}", other),
    }
}

// ── Cost-based optimizer ──────────────────────────────────────────────────────

#[test]
fn test_optimizer_equality_on_indexed_column() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE opttest (id INT PRIMARY KEY, val TEXT)");
    exec(&engine, "INSERT INTO opttest VALUES (1, 'a'), (2, 'b'), (3, 'c')");
    // Query with equality on indexed column — optimizer should produce correct results
    // regardless of whether it picks IndexScan or SeqScan
    let result = exec(&engine, "SELECT val FROM opttest WHERE id = 2");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "b"),
        other => panic!("expected 'b', got {:?}", other),
    }
}

#[test]
fn test_optimizer_non_indexed_column_still_works() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE opttest2 (id INT, name TEXT)");
    exec(&engine, "INSERT INTO opttest2 VALUES (1, 'Alice'), (2, 'Bob')");
    // Filter on non-indexed column: should fall back to SeqScan
    let result = exec(&engine, "SELECT id FROM opttest2 WHERE name = 'Alice'");
    assert_eq!(result.rows.len(), 1);
}

// ── pg_dump / pg_restore ──────────────────────────────────────────────────────

#[test]
fn test_dump_to_file() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE dump_src (id INT, name TEXT)");
    exec(&engine, "INSERT INTO dump_src VALUES (1, 'Alice'), (2, 'Bob')");

    let dump_path = dir.path().join("dump.sql");
    let count = engine.dump_to_file(dump_path.to_str().unwrap()).unwrap();
    // At minimum: 1 CREATE TABLE + 2 INSERT statements
    assert!(count >= 3, "expected at least 3 statements, got {}", count);
    assert!(dump_path.exists(), "dump file should exist");
}

#[test]
fn test_dump_and_restore_roundtrip() {
    let src_dir = TempDir::new().unwrap();
    let engine = make_engine(src_dir.path());
    exec(&engine, "CREATE TABLE dump_src (id INT, name TEXT)");
    exec(&engine, "INSERT INTO dump_src VALUES (1, 'Alice'), (2, 'Bob')");

    let dump_path = src_dir.path().join("dump.sql");
    engine.dump_to_file(dump_path.to_str().unwrap()).unwrap();

    // Restore into a fresh engine
    let dst_dir = TempDir::new().unwrap();
    let engine2 = make_engine(dst_dir.path());
    engine2.restore_from_file(dump_path.to_str().unwrap()).unwrap();

    let result = engine2.execute("SELECT COUNT(*) FROM dump_src").unwrap();
    assert_eq!(result.rows.len(), 1);
    let count = match result.rows[0].get_by_idx(0) {
        Some(Value::Int8(n)) => *n,
        Some(Value::Int4(n)) => *n as i64,
        other => panic!("expected count, got {:?}", other),
    };
    assert_eq!(count, 2, "expected 2 rows after restore");
}

#[test]
fn test_dump_empty_database() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let dump_path = dir.path().join("empty_dump.sql");
    let count = engine.dump_to_file(dump_path.to_str().unwrap()).unwrap();
    // Empty DB: 0 statements
    assert_eq!(count, 0);
}

// ── Autovacuum API ────────────────────────────────────────────────────────────

#[test]
fn test_tables_needing_vacuum_initially_all() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE avac_test (id INT)");
    // Tables that haven't been vacuumed should need vacuum
    let tables = engine.catalog.tables_needing_vacuum("public", 300);
    assert!(
        tables.contains(&"avac_test".to_string()),
        "avac_test should need vacuum initially"
    );
}

#[test]
fn test_tables_needing_vacuum_after_vacuum() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE avac_test2 (id INT)");
    exec(&engine, "VACUUM avac_test2");
    // After vacuum with stale_secs=0, it should still need vacuum (elapsed > 0s always)
    // But with stale_secs=300, it should NOT need vacuum right after being vacuumed
    let tables = engine.catalog.tables_needing_vacuum("public", 300);
    assert!(
        !tables.contains(&"avac_test2".to_string()),
        "avac_test2 should not need vacuum right after being vacuumed"
    );
}
