/// Tests for Tier 2 advanced features:
/// - Autovacuum daemon API
/// - LISTEN / NOTIFY
/// - CREATE FUNCTION (SQL language)
/// - Cost-based optimizer (IndexScan selection)
/// - pg_dump / pg_restore
use tempfile::TempDir;

use crate::common::{make_engine, exec};
use sql::Value;
#[allow(unused_imports)]
use catalog;

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

// ── Issue 16: SET TRANSACTION ISOLATION LEVEL ─────────────────────────────────

#[test]
fn test_set_transaction_isolation_level_serializable() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE iso_test (id INT, val INT)");
    exec(&engine, "INSERT INTO iso_test VALUES (1, 10)");

    // SET TRANSACTION ISOLATION LEVEL SERIALIZABLE followed by BEGIN should work
    engine.execute_session("s1", "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE").unwrap();
    engine.execute_session("s1", "BEGIN").unwrap();
    engine.execute_session("s1", "INSERT INTO iso_test VALUES (2, 20)").unwrap();
    engine.execute_session("s1", "COMMIT").unwrap();

    let r = exec(&engine, "SELECT COUNT(*) FROM iso_test");
    assert_eq!(r.rows.len(), 1);
    match r.rows[0].get_by_idx(0) {
        Some(Value::Int8(2)) | Some(Value::Int4(2)) => {}
        other => panic!("expected count 2, got {:?}", other),
    }
}

#[test]
fn test_set_transaction_isolation_level_repeatable_read() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE iso_rr (id INT)");
    exec(&engine, "INSERT INTO iso_rr VALUES (1)");

    // SET TRANSACTION ISOLATION LEVEL REPEATABLE READ should be accepted without error
    engine.execute_session("s1", "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ").unwrap();
    engine.execute_session("s1", "BEGIN").unwrap();
    let r = engine.execute_session("s1", "SELECT id FROM iso_rr").unwrap();
    assert_eq!(r.rows.len(), 1);
    engine.execute_session("s1", "COMMIT").unwrap();
}

#[test]
fn test_set_session_characteristics_isolation() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE iso_sc (x INT)");
    exec(&engine, "INSERT INTO iso_sc VALUES (42)");

    // SET SESSION CHARACTERISTICS AS TRANSACTION ISOLATION LEVEL SERIALIZABLE
    engine.execute_session("s1", "SET SESSION CHARACTERISTICS AS TRANSACTION ISOLATION LEVEL SERIALIZABLE").unwrap();
    engine.execute_session("s1", "BEGIN").unwrap();
    let r = engine.execute_session("s1", "SELECT x FROM iso_sc").unwrap();
    assert_eq!(r.rows.len(), 1);
    engine.execute_session("s1", "COMMIT").unwrap();
}

#[test]
fn test_set_transaction_read_committed() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE iso_rc (id INT)");

    // READ COMMITTED is the default, but explicit setting should work
    engine.execute_session("s1", "SET TRANSACTION ISOLATION LEVEL READ COMMITTED").unwrap();
    engine.execute_session("s1", "BEGIN").unwrap();
    engine.execute_session("s1", "INSERT INTO iso_rc VALUES (1)").unwrap();
    engine.execute_session("s1", "COMMIT").unwrap();

    let r = exec(&engine, "SELECT COUNT(*) FROM iso_rc");
    match r.rows[0].get_by_idx(0) {
        Some(Value::Int8(1)) | Some(Value::Int4(1)) => {}
        other => panic!("expected 1, got {:?}", other),
    }
}

// ── Issue 7: FK ON UPDATE CASCADE ─────────────────────────────────────────────

#[test]
fn test_fk_on_update_cascade() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE parent (id INT PRIMARY KEY, name TEXT)");
    exec(&engine, "CREATE TABLE child (id INT, parent_id INT REFERENCES parent(id) ON UPDATE CASCADE ON DELETE CASCADE)");

    exec(&engine, "INSERT INTO parent VALUES (1, 'Alice')");
    exec(&engine, "INSERT INTO child VALUES (10, 1)");
    exec(&engine, "INSERT INTO child VALUES (11, 1)");

    // Update the parent PK — child should cascade
    exec(&engine, "UPDATE parent SET id = 99 WHERE id = 1");

    // Child rows should now reference 99
    let r = exec(&engine, "SELECT parent_id FROM child ORDER BY id");
    assert_eq!(r.rows.len(), 2);
    for row in &r.rows {
        match row.get_by_idx(0) {
            Some(Value::Int4(99)) | Some(Value::Int8(99)) => {}
            other => panic!("expected parent_id=99 after cascade update, got {:?}", other),
        }
    }
}

#[test]
fn test_fk_on_update_restrict() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE par2 (id INT PRIMARY KEY)");
    exec(&engine, "CREATE TABLE chd2 (id INT, par_id INT REFERENCES par2(id) ON UPDATE RESTRICT)");

    exec(&engine, "INSERT INTO par2 VALUES (1)");
    exec(&engine, "INSERT INTO chd2 VALUES (10, 1)");

    // Update the parent PK should fail (restrict)
    let err = engine.execute("UPDATE par2 SET id = 2 WHERE id = 1");
    assert!(err.is_err(), "expected FK violation on UPDATE with RESTRICT");
}

#[test]
fn test_fk_on_update_set_null() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE par3 (id INT PRIMARY KEY)");
    exec(&engine, "CREATE TABLE chd3 (id INT, par_id INT REFERENCES par3(id) ON UPDATE SET NULL)");

    exec(&engine, "INSERT INTO par3 VALUES (5)");
    exec(&engine, "INSERT INTO chd3 VALUES (1, 5)");

    exec(&engine, "UPDATE par3 SET id = 99 WHERE id = 5");

    let r = exec(&engine, "SELECT par_id FROM chd3");
    assert_eq!(r.rows.len(), 1);
    match r.rows[0].get_by_idx(0) {
        Some(Value::Null) => {}
        other => panic!("expected NULL after SET NULL cascade, got {:?}", other),
    }
}

// ── Issue 11: CREATE SEQUENCE standalone ─────────────────────────────────────

#[test]
fn test_create_sequence_nextval() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE SEQUENCE myseq");

    // nextval should return sequential values starting at 1
    let v1 = match exec(&engine, "SELECT nextval('myseq')").rows[0].get_by_idx(0) {
        Some(Value::Int8(n)) => *n,
        other => panic!("expected Int8, got {:?}", other),
    };
    let v2 = match exec(&engine, "SELECT nextval('myseq')").rows[0].get_by_idx(0) {
        Some(Value::Int8(n)) => *n,
        other => panic!("expected Int8, got {:?}", other),
    };
    assert_eq!(v1, 1);
    assert_eq!(v2, 2);
}

#[test]
fn test_create_sequence_currval() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE SEQUENCE seqcurr START 10");
    exec(&engine, "SELECT nextval('seqcurr')"); // must call nextval first

    let cv = match exec(&engine, "SELECT currval('seqcurr')").rows[0].get_by_idx(0) {
        Some(Value::Int8(n)) => *n,
        other => panic!("expected Int8, got {:?}", other),
    };
    assert_eq!(cv, 10, "currval should return last nextval result");
}

#[test]
fn test_create_sequence_setval() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE SEQUENCE seqset");
    exec(&engine, "SELECT setval('seqset', 100)");

    let v = match exec(&engine, "SELECT nextval('seqset')").rows[0].get_by_idx(0) {
        Some(Value::Int8(n)) => *n,
        other => panic!("expected Int8, got {:?}", other),
    };
    // After setval(seq, 100), next nextval should return 100
    assert_eq!(v, 100);
}

#[test]
fn test_create_sequence_if_not_exists() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE SEQUENCE idempseq START 5");
    // Should not error on second creation with IF NOT EXISTS
    exec(&engine, "CREATE SEQUENCE IF NOT EXISTS idempseq START 5");

    let v = match exec(&engine, "SELECT nextval('idempseq')").rows[0].get_by_idx(0) {
        Some(Value::Int8(n)) => *n,
        other => panic!("expected Int8, got {:?}", other),
    };
    assert_eq!(v, 5);
}

#[test]
fn test_drop_sequence() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE SEQUENCE dropseq");
    exec(&engine, "SELECT nextval('dropseq')"); // use it
    exec(&engine, "DROP SEQUENCE dropseq");

    // nextval should now fail
    let err = engine.execute("SELECT nextval('dropseq')");
    assert!(err.is_err(), "expected error after DROP SEQUENCE");
}

#[test]
fn test_sequence_schema_qualified() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE SEQUENCE public.schseq START 42");
    let v = match exec(&engine, "SELECT nextval('public.schseq')").rows[0].get_by_idx(0) {
        Some(Value::Int8(n)) => *n,
        other => panic!("expected Int8, got {:?}", other),
    };
    assert_eq!(v, 42);
}

// ── Issue 15: ALTER TABLE advanced ops ───────────────────────────────────────

#[test]
fn test_alter_table_set_not_null() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE nn_test (id INT, name TEXT)");
    exec(&engine, "ALTER TABLE nn_test ALTER COLUMN name SET NOT NULL");

    // Verify column is now marked not null in schema
    let schema = engine.catalog.get_table("public", "nn_test").unwrap();
    let col = schema.columns.iter().find(|c| c.name == "name").unwrap();
    assert!(col.not_null, "column should be NOT NULL after ALTER");
}

#[test]
fn test_alter_table_drop_not_null() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE nn_drop (id INT, name TEXT NOT NULL)");
    exec(&engine, "ALTER TABLE nn_drop ALTER COLUMN name DROP NOT NULL");

    let schema = engine.catalog.get_table("public", "nn_drop").unwrap();
    let col = schema.columns.iter().find(|c| c.name == "name").unwrap();
    assert!(!col.not_null, "column should be nullable after DROP NOT NULL");
}

#[test]
fn test_alter_table_set_column_type() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE type_test (id INT, val TEXT)");
    exec(&engine, "ALTER TABLE type_test ALTER COLUMN val TYPE BIGINT");

    let schema = engine.catalog.get_table("public", "type_test").unwrap();
    let col = schema.columns.iter().find(|c| c.name == "val").unwrap();
    assert_eq!(col.data_type, catalog::DataType::Int8, "column type should be BIGINT");
}

#[test]
fn test_alter_table_add_check_constraint() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE ck_test (id INT, age INT)");
    exec(&engine, "ALTER TABLE ck_test ADD CONSTRAINT age_positive CHECK (age > 0)");

    // Insert valid row
    exec(&engine, "INSERT INTO ck_test VALUES (1, 25)");

    // Insert invalid row should fail
    let err = engine.execute("INSERT INTO ck_test VALUES (2, -1)");
    assert!(err.is_err(), "expected CHECK constraint violation");
}

#[test]
fn test_alter_table_drop_constraint() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE ck_drop (id INT, score INT)");
    exec(&engine, "ALTER TABLE ck_drop ADD CONSTRAINT score_pos CHECK (score > 0)");

    // Constraint in place: should reject negative
    assert!(engine.execute("INSERT INTO ck_drop VALUES (1, -5)").is_err());

    // Drop the constraint
    exec(&engine, "ALTER TABLE ck_drop DROP CONSTRAINT score_pos");

    // Now negative should succeed
    exec(&engine, "INSERT INTO ck_drop VALUES (1, -5)");
    let r = exec(&engine, "SELECT COUNT(*) FROM ck_drop");
    match r.rows[0].get_by_idx(0) {
        Some(Value::Int8(1)) | Some(Value::Int4(1)) => {}
        other => panic!("expected 1 row, got {:?}", other),
    }
}

#[test]
fn test_alter_table_set_default() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE def_test (id INT, val INT)");
    exec(&engine, "ALTER TABLE def_test ALTER COLUMN val SET DEFAULT 42");

    let schema = engine.catalog.get_table("public", "def_test").unwrap();
    let col = schema.columns.iter().find(|c| c.name == "val").unwrap();
    assert!(col.has_default, "column should have a default after SET DEFAULT");
    assert_eq!(col.default_expr.as_deref(), Some("42"));
}

#[test]
fn test_alter_table_drop_default() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE def_drop (id INT, val INT DEFAULT 99)");
    exec(&engine, "ALTER TABLE def_drop ALTER COLUMN val DROP DEFAULT");

    let schema = engine.catalog.get_table("public", "def_drop").unwrap();
    let col = schema.columns.iter().find(|c| c.name == "val").unwrap();
    assert!(!col.has_default, "column should have no default after DROP DEFAULT");
}
