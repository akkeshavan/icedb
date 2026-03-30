/// Tests for Tier 2 advanced features:
/// - Autovacuum daemon API
/// - LISTEN / NOTIFY
/// - CREATE FUNCTION (SQL language)
/// - Cost-based optimizer (IndexScan selection)
/// - pg_dump / pg_restore
use tempfile::TempDir;

use crate::common::{make_engine, exec, exec_session, Backend};
use sql::Value;
#[allow(unused_imports)]
use catalog;

// ── LISTEN / NOTIFY ───────────────────────────────────────────────────────────

fn test_notify_basic_body(b: &crate::common::Backend) {
    // NOTIFY should succeed without error
    exec(b, "NOTIFY my_channel, 'hello'");
}

#[test]
fn test_notify_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_notify_basic_body(&b);
}

crate::net_tests!(test_notify_basic);


fn test_notify_no_payload_body(b: &crate::common::Backend) {
    exec(b, "NOTIFY events");
}

#[test]
fn test_notify_no_payload() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_notify_no_payload_body(&b);
}

crate::net_tests!(test_notify_no_payload);


fn test_listen_basic_body(b: &crate::common::Backend) {
    exec(b, "LISTEN my_channel");
    exec(b, "UNLISTEN my_channel");
}

#[test]
fn test_listen_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_listen_basic_body(&b);
}

crate::net_tests!(test_listen_basic);


fn test_listen_notify_roundtrip_body(b: &crate::common::Backend) {
    if b.is_network() { return; } // Uses internal catalog.listen() API not available over wire
    // Subscribe to channel via catalog directly
    let engine = b.as_engine().clone();
    let rx = engine.catalog.listen("test_chan");
    // Send notification
    exec(b, "NOTIFY test_chan, 'payload123'");
    // Receive the notification
    let msg = rx.try_recv();
    assert!(msg.is_ok(), "expected a notification message");
    assert_eq!(msg.unwrap(), "payload123");
}

#[test]
fn test_listen_notify_roundtrip() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_listen_notify_roundtrip_body(&b);
}

crate::net_tests!(test_listen_notify_roundtrip);


fn test_unlisten_star_body(b: &crate::common::Backend) {
    exec(b, "LISTEN chan1");
    exec(b, "LISTEN chan2");
    exec(b, "UNLISTEN *");
}

#[test]
fn test_unlisten_star() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_unlisten_star_body(&b);
}

crate::net_tests!(test_unlisten_star);


// ── CREATE FUNCTION ───────────────────────────────────────────────────────────

fn test_create_sql_function_basic_body(b: &crate::common::Backend) {
    exec(b, "CREATE FUNCTION add_two(a INT, b INT) RETURNS INT LANGUAGE SQL AS $$ SELECT $1 + $2 $$");
}

#[test]
fn test_create_sql_function_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_sql_function_basic_body(&b);
}

crate::net_tests!(test_create_sql_function_basic);


fn test_create_and_call_sql_function_body(b: &crate::common::Backend) {
    exec(b, "CREATE FUNCTION add_two(a INT, b INT) RETURNS INT LANGUAGE SQL AS $$ SELECT $1 + $2 $$");
    let result = exec(b, "SELECT add_two(3, 4)");
    assert_eq!(result.rows.len(), 1);
    let val = result.rows[0].get_by_idx(0).cloned().unwrap();
    match val {
        Value::Int4(7) | Value::Int8(7) => {}
        other => panic!("expected 7, got {:?}", other),
    }
}

#[test]
fn test_create_and_call_sql_function() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_and_call_sql_function_body(&b);
}

crate::net_tests!(test_create_and_call_sql_function);


fn test_drop_function_body(b: &crate::common::Backend) {
    exec(b, "CREATE FUNCTION temp_fn() RETURNS INT LANGUAGE SQL AS $$ SELECT 42 $$");
    exec(b, "DROP FUNCTION temp_fn");
}

#[test]
fn test_drop_function() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_drop_function_body(&b);
}

crate::net_tests!(test_drop_function);


fn test_drop_function_if_exists_body(b: &crate::common::Backend) {
    // Dropping a non-existent function with IF EXISTS should not error
    exec(b, "DROP FUNCTION IF EXISTS nonexistent_fn");
}

#[test]
fn test_drop_function_if_exists() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_drop_function_if_exists_body(&b);
}

crate::net_tests!(test_drop_function_if_exists);


fn test_sql_function_constant_body(b: &crate::common::Backend) {
    exec(b, "CREATE FUNCTION get_answer() RETURNS INT LANGUAGE SQL AS $$ SELECT 42 $$");
    let result = exec(b, "SELECT get_answer()");
    assert_eq!(result.rows.len(), 1);
    let val = result.rows[0].get_by_idx(0).cloned().unwrap();
    match val {
        Value::Int4(42) | Value::Int8(42) => {}
        other => panic!("expected 42, got {:?}", other),
    }
}

#[test]
fn test_sql_function_constant() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_sql_function_constant_body(&b);
}

crate::net_tests!(test_sql_function_constant);


// ── Cost-based optimizer ──────────────────────────────────────────────────────

fn test_optimizer_equality_on_indexed_column_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE opttest (id INT PRIMARY KEY, val TEXT)");
    exec(b, "INSERT INTO opttest VALUES (1, 'a'), (2, 'b'), (3, 'c')");
    // Query with equality on indexed column — optimizer should produce correct results
    // regardless of whether it picks IndexScan or SeqScan
    let result = exec(b, "SELECT val FROM opttest WHERE id = 2");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "b"),
        other => panic!("expected 'b', got {:?}", other),
    }
}

#[test]
fn test_optimizer_equality_on_indexed_column() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_optimizer_equality_on_indexed_column_body(&b);
}

crate::net_tests!(test_optimizer_equality_on_indexed_column);


fn test_optimizer_non_indexed_column_still_works_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE opttest2 (id INT, name TEXT)");
    exec(b, "INSERT INTO opttest2 VALUES (1, 'Alice'), (2, 'Bob')");
    // Filter on non-indexed column: should fall back to SeqScan
    let result = exec(b, "SELECT id FROM opttest2 WHERE name = 'Alice'");
    assert_eq!(result.rows.len(), 1);
}

#[test]
fn test_optimizer_non_indexed_column_still_works() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_optimizer_non_indexed_column_still_works_body(&b);
}

crate::net_tests!(test_optimizer_non_indexed_column_still_works);


// ── pg_dump / pg_restore ──────────────────────────────────────────────────────

fn test_dump_to_file_body(b: &crate::common::Backend) {
    if b.is_network() { return; } // Uses engine.dump_to_file() internal API
    exec(b, "CREATE TABLE dump_src (id INT, name TEXT)");
    exec(b, "INSERT INTO dump_src VALUES (1, 'Alice'), (2, 'Bob')");

    let engine = b.as_engine().clone();
    let tmp = tempfile::TempDir::new().unwrap();
    let dump_path = tmp.path().join("dump.sql");
    let count = engine.dump_to_file(dump_path.to_str().unwrap()).unwrap();
    // At minimum: 1 CREATE TABLE + 2 INSERT statements
    assert!(count >= 3, "expected at least 3 statements, got {}", count);
    assert!(dump_path.exists(), "dump file should exist");
}

#[test]
fn test_dump_to_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_dump_to_file_body(&b);
}

crate::net_tests!(test_dump_to_file);


fn test_dump_and_restore_roundtrip_body(b: &crate::common::Backend) {
    if b.is_network() { return; } // Uses engine.dump_to_file() / restore_from_file() internal API
    let src_dir = TempDir::new().unwrap();
    exec(b, "CREATE TABLE dump_src (id INT, name TEXT)");
    exec(b, "INSERT INTO dump_src VALUES (1, 'Alice'), (2, 'Bob')");

    let engine = b.as_engine().clone();
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
fn test_dump_and_restore_roundtrip() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_dump_and_restore_roundtrip_body(&b);
}

crate::net_tests!(test_dump_and_restore_roundtrip);


fn test_dump_empty_database_body(b: &crate::common::Backend) {
    if b.is_network() { return; } // Uses engine.dump_to_file() internal API
    let engine = b.as_engine().clone();
    let tmp = tempfile::TempDir::new().unwrap();
    let dump_path = tmp.path().join("empty_dump.sql");
    let count = engine.dump_to_file(dump_path.to_str().unwrap()).unwrap();
    // Empty DB: 0 statements
    assert_eq!(count, 0);
}

#[test]
fn test_dump_empty_database() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_dump_empty_database_body(&b);
}

crate::net_tests!(test_dump_empty_database);


// ── Autovacuum API ────────────────────────────────────────────────────────────

fn test_tables_needing_vacuum_initially_all_body(b: &crate::common::Backend) {
    if b.is_network() { return; } // Uses internal catalog.tables_needing_vacuum() API
    exec(b, "CREATE TABLE avac_test (id INT)");
    // Tables that haven't been vacuumed should need vacuum
    let engine = b.as_engine().clone();
    let tables = engine.catalog.tables_needing_vacuum("public", 300);
    assert!(
        tables.contains(&"avac_test".to_string()),
        "avac_test should need vacuum initially"
    );
}

#[test]
fn test_tables_needing_vacuum_initially_all() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_tables_needing_vacuum_initially_all_body(&b);
}

crate::net_tests!(test_tables_needing_vacuum_initially_all);


fn test_tables_needing_vacuum_after_vacuum_body(b: &crate::common::Backend) {
    if b.is_network() { return; } // Uses internal catalog.tables_needing_vacuum() API
    exec(b, "CREATE TABLE avac_test2 (id INT)");
    exec(b, "VACUUM avac_test2");
    // After vacuum with stale_secs=0, it should still need vacuum (elapsed > 0s always)
    // But with stale_secs=300, it should NOT need vacuum right after being vacuumed
    let engine = b.as_engine().clone();
    let tables = engine.catalog.tables_needing_vacuum("public", 300);
    assert!(
        !tables.contains(&"avac_test2".to_string()),
        "avac_test2 should not need vacuum right after being vacuumed"
    );
}

#[test]
fn test_tables_needing_vacuum_after_vacuum() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_tables_needing_vacuum_after_vacuum_body(&b);
}

crate::net_tests!(test_tables_needing_vacuum_after_vacuum);


// ── Issue 16: SET TRANSACTION ISOLATION LEVEL ─────────────────────────────────

fn test_set_transaction_isolation_level_serializable_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE iso_test (id INT, val INT)");
    exec(b, "INSERT INTO iso_test VALUES (1, 10)");

    // SET TRANSACTION ISOLATION LEVEL SERIALIZABLE followed by BEGIN should work
    exec_session(b, "s1", "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE");
    exec_session(b, "s1", "BEGIN");
    exec_session(b, "s1", "INSERT INTO iso_test VALUES (2, 20)");
    exec_session(b, "s1", "COMMIT");

    let r = exec(b, "SELECT COUNT(*) FROM iso_test");
    assert_eq!(r.rows.len(), 1);
    match r.rows[0].get_by_idx(0) {
        Some(Value::Int8(2)) | Some(Value::Int4(2)) => {}
        other => panic!("expected count 2, got {:?}", other),
    }
}

#[test]
fn test_set_transaction_isolation_level_serializable() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_set_transaction_isolation_level_serializable_body(&b);
}

crate::net_tests!(test_set_transaction_isolation_level_serializable);


fn test_set_transaction_isolation_level_repeatable_read_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE iso_rr (id INT)");
    exec(b, "INSERT INTO iso_rr VALUES (1)");

    // SET TRANSACTION ISOLATION LEVEL REPEATABLE READ should be accepted without error
    exec_session(b, "s1", "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ");
    exec_session(b, "s1", "BEGIN");
    let r = exec_session(b, "s1", "SELECT id FROM iso_rr");
    assert_eq!(r.rows.len(), 1);
    exec_session(b, "s1", "COMMIT");
}

#[test]
fn test_set_transaction_isolation_level_repeatable_read() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_set_transaction_isolation_level_repeatable_read_body(&b);
}

crate::net_tests!(test_set_transaction_isolation_level_repeatable_read);


fn test_set_session_characteristics_isolation_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE iso_sc (x INT)");
    exec(b, "INSERT INTO iso_sc VALUES (42)");

    // SET SESSION CHARACTERISTICS AS TRANSACTION ISOLATION LEVEL SERIALIZABLE
    exec_session(b, "s1", "SET SESSION CHARACTERISTICS AS TRANSACTION ISOLATION LEVEL SERIALIZABLE");
    exec_session(b, "s1", "BEGIN");
    let r = exec_session(b, "s1", "SELECT x FROM iso_sc");
    assert_eq!(r.rows.len(), 1);
    exec_session(b, "s1", "COMMIT");
}

#[test]
fn test_set_session_characteristics_isolation() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_set_session_characteristics_isolation_body(&b);
}

crate::net_tests!(test_set_session_characteristics_isolation);


fn test_set_transaction_read_committed_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE iso_rc (id INT)");

    // READ COMMITTED is the default, but explicit setting should work
    exec_session(b, "s1", "SET TRANSACTION ISOLATION LEVEL READ COMMITTED");
    exec_session(b, "s1", "BEGIN");
    exec_session(b, "s1", "INSERT INTO iso_rc VALUES (1)");
    exec_session(b, "s1", "COMMIT");

    let r = exec(b, "SELECT COUNT(*) FROM iso_rc");
    match r.rows[0].get_by_idx(0) {
        Some(Value::Int8(1)) | Some(Value::Int4(1)) => {}
        other => panic!("expected 1, got {:?}", other),
    }
}

#[test]
fn test_set_transaction_read_committed() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_set_transaction_read_committed_body(&b);
}

crate::net_tests!(test_set_transaction_read_committed);


// ── Issue 7: FK ON UPDATE CASCADE ─────────────────────────────────────────────

fn test_fk_on_update_cascade_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE parent (id INT PRIMARY KEY, name TEXT)");
    exec(b, "CREATE TABLE child (id INT, parent_id INT REFERENCES parent(id) ON UPDATE CASCADE ON DELETE CASCADE)");

    exec(b, "INSERT INTO parent VALUES (1, 'Alice')");
    exec(b, "INSERT INTO child VALUES (10, 1)");
    exec(b, "INSERT INTO child VALUES (11, 1)");

    // Update the parent PK — child should cascade
    exec(b, "UPDATE parent SET id = 99 WHERE id = 1");

    // Child rows should now reference 99
    let r = exec(b, "SELECT parent_id FROM child ORDER BY id");
    assert_eq!(r.rows.len(), 2);
    for row in &r.rows {
        match row.get_by_idx(0) {
            Some(Value::Int4(99)) | Some(Value::Int8(99)) => {}
            other => panic!("expected parent_id=99 after cascade update, got {:?}", other),
        }
    }
}

#[test]
fn test_fk_on_update_cascade() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_fk_on_update_cascade_body(&b);
}

crate::net_tests!(test_fk_on_update_cascade);


fn test_fk_on_update_restrict_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE par2 (id INT PRIMARY KEY)");
    exec(b, "CREATE TABLE chd2 (id INT, par_id INT REFERENCES par2(id) ON UPDATE RESTRICT)");

    exec(b, "INSERT INTO par2 VALUES (1)");
    exec(b, "INSERT INTO chd2 VALUES (10, 1)");

    // Update the parent PK should fail (restrict)
    let err = b.try_execute("UPDATE par2 SET id = 2 WHERE id = 1");
    assert!(err.is_err(), "expected FK violation on UPDATE with RESTRICT");
}

#[test]
fn test_fk_on_update_restrict() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_fk_on_update_restrict_body(&b);
}

crate::net_tests!(test_fk_on_update_restrict);


fn test_fk_on_update_set_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE par3 (id INT PRIMARY KEY)");
    exec(b, "CREATE TABLE chd3 (id INT, par_id INT REFERENCES par3(id) ON UPDATE SET NULL)");

    exec(b, "INSERT INTO par3 VALUES (5)");
    exec(b, "INSERT INTO chd3 VALUES (1, 5)");

    exec(b, "UPDATE par3 SET id = 99 WHERE id = 5");

    let r = exec(b, "SELECT par_id FROM chd3");
    assert_eq!(r.rows.len(), 1);
    match r.rows[0].get_by_idx(0) {
        Some(Value::Null) => {}
        other => panic!("expected NULL after SET NULL cascade, got {:?}", other),
    }
}

#[test]
fn test_fk_on_update_set_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_fk_on_update_set_null_body(&b);
}

crate::net_tests!(test_fk_on_update_set_null);


// ── Issue 11: CREATE SEQUENCE standalone ─────────────────────────────────────

fn test_create_sequence_nextval_body(b: &crate::common::Backend) {
    exec(b, "CREATE SEQUENCE myseq");

    // nextval should return sequential values starting at 1
    let v1 = match exec(b, "SELECT nextval('myseq')").rows[0].get_by_idx(0) {
        Some(Value::Int8(n)) => *n,
        other => panic!("expected Int8, got {:?}", other),
    };
    let v2 = match exec(b, "SELECT nextval('myseq')").rows[0].get_by_idx(0) {
        Some(Value::Int8(n)) => *n,
        other => panic!("expected Int8, got {:?}", other),
    };
    assert_eq!(v1, 1);
    assert_eq!(v2, 2);
}

#[test]
fn test_create_sequence_nextval() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_sequence_nextval_body(&b);
}

crate::net_tests!(test_create_sequence_nextval);


fn test_create_sequence_currval_body(b: &crate::common::Backend) {
    exec(b, "CREATE SEQUENCE seqcurr START 10");
    exec(b, "SELECT nextval('seqcurr')"); // must call nextval first

    let cv = match exec(b, "SELECT currval('seqcurr')").rows[0].get_by_idx(0) {
        Some(Value::Int8(n)) => *n,
        other => panic!("expected Int8, got {:?}", other),
    };
    assert_eq!(cv, 10, "currval should return last nextval result");
}

#[test]
fn test_create_sequence_currval() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_sequence_currval_body(&b);
}

crate::net_tests!(test_create_sequence_currval);


fn test_create_sequence_setval_body(b: &crate::common::Backend) {
    exec(b, "CREATE SEQUENCE seqset");
    exec(b, "SELECT setval('seqset', 100)");

    let v = match exec(b, "SELECT nextval('seqset')").rows[0].get_by_idx(0) {
        Some(Value::Int8(n)) => *n,
        other => panic!("expected Int8, got {:?}", other),
    };
    // After setval(seq, 100), next nextval should return 100
    assert_eq!(v, 100);
}

#[test]
fn test_create_sequence_setval() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_sequence_setval_body(&b);
}

crate::net_tests!(test_create_sequence_setval);


fn test_create_sequence_if_not_exists_body(b: &crate::common::Backend) {
    exec(b, "CREATE SEQUENCE idempseq START 5");
    // Should not error on second creation with IF NOT EXISTS
    exec(b, "CREATE SEQUENCE IF NOT EXISTS idempseq START 5");

    let v = match exec(b, "SELECT nextval('idempseq')").rows[0].get_by_idx(0) {
        Some(Value::Int8(n)) => *n,
        other => panic!("expected Int8, got {:?}", other),
    };
    assert_eq!(v, 5);
}

#[test]
fn test_create_sequence_if_not_exists() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_create_sequence_if_not_exists_body(&b);
}

crate::net_tests!(test_create_sequence_if_not_exists);


fn test_drop_sequence_body(b: &crate::common::Backend) {
    exec(b, "CREATE SEQUENCE dropseq");
    exec(b, "SELECT nextval('dropseq')"); // use it
    exec(b, "DROP SEQUENCE dropseq");

    // nextval should now fail
    let err = b.try_execute("SELECT nextval('dropseq')");
    assert!(err.is_err(), "expected error after DROP SEQUENCE");
}

#[test]
fn test_drop_sequence() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_drop_sequence_body(&b);
}

crate::net_tests!(test_drop_sequence);


fn test_sequence_schema_qualified_body(b: &crate::common::Backend) {
    exec(b, "CREATE SEQUENCE public.schseq START 42");
    let v = match exec(b, "SELECT nextval('public.schseq')").rows[0].get_by_idx(0) {
        Some(Value::Int8(n)) => *n,
        other => panic!("expected Int8, got {:?}", other),
    };
    assert_eq!(v, 42);
}

#[test]
fn test_sequence_schema_qualified() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_sequence_schema_qualified_body(&b);
}

crate::net_tests!(test_sequence_schema_qualified);


// ── Issue 15: ALTER TABLE advanced ops ───────────────────────────────────────

fn test_alter_table_set_not_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE nn_test (id INT, name TEXT)");
    exec(b, "ALTER TABLE nn_test ALTER COLUMN name SET NOT NULL");

    if b.is_network() {
        // Verify functionally: inserting NULL should fail
        let err = b.try_execute("INSERT INTO nn_test VALUES (1, NULL)");
        assert!(err.is_err(), "NOT NULL constraint should reject NULL insert");
    } else {
        let engine = b.as_engine().clone();
        let schema = engine.catalog.get_table("public", "nn_test").unwrap();
        let col = schema.columns.iter().find(|c| c.name == "name").unwrap();
        assert!(col.not_null, "column should be NOT NULL after ALTER");
    }
}

#[test]
fn test_alter_table_set_not_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_alter_table_set_not_null_body(&b);
}

crate::net_tests!(test_alter_table_set_not_null);


fn test_alter_table_drop_not_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE nn_drop (id INT, name TEXT NOT NULL)");
    exec(b, "ALTER TABLE nn_drop ALTER COLUMN name DROP NOT NULL");

    if b.is_network() {
        // Verify functionally: inserting NULL should now succeed
        exec(b, "INSERT INTO nn_drop VALUES (1, NULL)");
        let r = exec(b, "SELECT COUNT(*) FROM nn_drop");
        match r.rows[0].get_by_idx(0) {
            Some(Value::Int8(1)) | Some(Value::Int4(1)) => {}
            other => panic!("expected 1 row after nullable insert, got {:?}", other),
        }
    } else {
        let engine = b.as_engine().clone();
        let schema = engine.catalog.get_table("public", "nn_drop").unwrap();
        let col = schema.columns.iter().find(|c| c.name == "name").unwrap();
        assert!(!col.not_null, "column should be nullable after DROP NOT NULL");
    }
}

#[test]
fn test_alter_table_drop_not_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_alter_table_drop_not_null_body(&b);
}

crate::net_tests!(test_alter_table_drop_not_null);


fn test_alter_table_set_column_type_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE type_test (id INT, val TEXT)");
    exec(b, "ALTER TABLE type_test ALTER COLUMN val TYPE BIGINT");

    if b.is_network() {
        // Verify functionally: insert a BIGINT value and read it back
        exec(b, "INSERT INTO type_test VALUES (1, 9876543210)");
        let r = exec(b, "SELECT val FROM type_test WHERE id = 1");
        match r.rows[0].get_by_idx(0) {
            Some(Value::Int8(9876543210)) | Some(Value::Int4(_)) => {}
            other => panic!("expected bigint value after ALTER COLUMN TYPE, got {:?}", other),
        }
    } else {
        let engine = b.as_engine().clone();
        let schema = engine.catalog.get_table("public", "type_test").unwrap();
        let col = schema.columns.iter().find(|c| c.name == "val").unwrap();
        assert_eq!(col.data_type, catalog::DataType::Int8, "column type should be BIGINT");
    }
}

#[test]
fn test_alter_table_set_column_type() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_alter_table_set_column_type_body(&b);
}

crate::net_tests!(test_alter_table_set_column_type);


fn test_alter_table_add_check_constraint_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE ck_test (id INT, age INT)");
    exec(b, "ALTER TABLE ck_test ADD CONSTRAINT age_positive CHECK (age > 0)");

    // Insert valid row
    exec(b, "INSERT INTO ck_test VALUES (1, 25)");

    // Insert invalid row should fail
    let err = b.try_execute("INSERT INTO ck_test VALUES (2, -1)");
    assert!(err.is_err(), "expected CHECK constraint violation");
}

#[test]
fn test_alter_table_add_check_constraint() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_alter_table_add_check_constraint_body(&b);
}

crate::net_tests!(test_alter_table_add_check_constraint);


fn test_alter_table_drop_constraint_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE ck_drop (id INT, score INT)");
    exec(b, "ALTER TABLE ck_drop ADD CONSTRAINT score_pos CHECK (score > 0)");

    // Constraint in place: should reject negative
    assert!(b.try_execute("INSERT INTO ck_drop VALUES (1, -5)").is_err());

    // Drop the constraint
    exec(b, "ALTER TABLE ck_drop DROP CONSTRAINT score_pos");

    // Now negative should succeed
    exec(b, "INSERT INTO ck_drop VALUES (1, -5)");
    let r = exec(b, "SELECT COUNT(*) FROM ck_drop");
    match r.rows[0].get_by_idx(0) {
        Some(Value::Int8(1)) | Some(Value::Int4(1)) => {}
        other => panic!("expected 1 row, got {:?}", other),
    }
}

#[test]
fn test_alter_table_drop_constraint() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_alter_table_drop_constraint_body(&b);
}

crate::net_tests!(test_alter_table_drop_constraint);


fn test_alter_table_set_default_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE def_test (id INT, val INT)");
    exec(b, "ALTER TABLE def_test ALTER COLUMN val SET DEFAULT 42");

    if b.is_network() {
        // Verify functionally: insert without val column and check default is applied
        exec(b, "INSERT INTO def_test (id) VALUES (1)");
        let r = exec(b, "SELECT val FROM def_test WHERE id = 1");
        match r.rows[0].get_by_idx(0) {
            Some(Value::Int4(42)) | Some(Value::Int8(42)) => {}
            other => panic!("expected default value 42 after SET DEFAULT, got {:?}", other),
        }
    } else {
        let engine = b.as_engine().clone();
        let schema = engine.catalog.get_table("public", "def_test").unwrap();
        let col = schema.columns.iter().find(|c| c.name == "val").unwrap();
        assert!(col.has_default, "column should have a default after SET DEFAULT");
        assert_eq!(col.default_expr.as_deref(), Some("42"));
    }
}

#[test]
fn test_alter_table_set_default() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_alter_table_set_default_body(&b);
}

crate::net_tests!(test_alter_table_set_default);


fn test_alter_table_drop_default_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE def_drop (id INT, val INT DEFAULT 99)");
    exec(b, "ALTER TABLE def_drop ALTER COLUMN val DROP DEFAULT");

    if b.is_network() {
        // Verify functionally: insert without val column — val should be NULL (no default)
        exec(b, "INSERT INTO def_drop (id) VALUES (1)");
        let r = exec(b, "SELECT val FROM def_drop WHERE id = 1");
        match r.rows[0].get_by_idx(0) {
            Some(Value::Null) => {}
            other => panic!("expected NULL after DROP DEFAULT, got {:?}", other),
        }
    } else {
        let engine = b.as_engine().clone();
        let schema = engine.catalog.get_table("public", "def_drop").unwrap();
        let col = schema.columns.iter().find(|c| c.name == "val").unwrap();
        assert!(!col.has_default, "column should have no default after DROP DEFAULT");
    }
}

#[test]
fn test_alter_table_drop_default() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_alter_table_drop_default_body(&b);
}

crate::net_tests!(test_alter_table_drop_default);

