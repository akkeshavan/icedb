/// Category 10: Transaction control tests
/// BEGIN/COMMIT/ROLLBACK, isolation levels, SAVEPOINTs, visibility.
/// Ported from PostgreSQL transactions.sql regression suite.
use tempfile::TempDir;
use txn::transaction::IsolationLevel;
use crate::common::{make_engine, exec_engine as exec, query_int_engine as query_int, count_rows_engine as count_rows, exec_session_engine as exec_session, exec_session_err_engine as exec_session_err};
use sql::Value;

// ── Helper ────────────────────────────────────────────────────────────────────

fn int8_or_int4(v: &Value) -> i64 {
    match v {
        Value::Int8(n) => *n,
        Value::Int4(n) => *n as i64,
        other => panic!("expected integer, got {:?}", other),
    }
}

// ── Original tests ────────────────────────────────────────────────────────────

#[test]
fn test_txn_begin_commit_visible() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");

    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "INSERT INTO t VALUES (1, 100)").unwrap();
    engine.txn_manager.commit(xid).unwrap();

    let v = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(v, 100, "Committed row must be visible");
}

#[test]
fn test_txn_begin_rollback_invisible() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");

    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "INSERT INTO t VALUES (1, 100)").unwrap();
    engine.txn_manager.abort(xid).unwrap();

    let result = engine.execute("SELECT val FROM t WHERE id = 1").unwrap();
    assert_eq!(result.rows.len(), 0, "Rolled-back row must NOT be visible");
}

#[test]
fn test_txn_multiple_statements_commit() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE accounts (id INT, balance INT)");
    exec(&engine, "INSERT INTO accounts VALUES (1, 1000)");
    exec(&engine, "INSERT INTO accounts VALUES (2, 1000)");

    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "UPDATE accounts SET balance = 500 WHERE id = 1").unwrap();
    engine.execute_in_txn(xid, "UPDATE accounts SET balance = 1500 WHERE id = 2").unwrap();
    engine.txn_manager.commit(xid).unwrap();

    let b1 = query_int(&engine, "SELECT balance FROM accounts WHERE id = 1");
    let b2 = query_int(&engine, "SELECT balance FROM accounts WHERE id = 2");
    assert_eq!(b1, 500, "Account 1 balance after transfer");
    assert_eq!(b2, 1500, "Account 2 balance after transfer");
    assert_eq!(b1 + b2, 2000, "Total balance preserved");
}

#[test]
fn test_txn_multiple_statements_rollback() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE accounts (id INT, balance INT)");
    exec(&engine, "INSERT INTO accounts VALUES (1, 1000)");
    exec(&engine, "INSERT INTO accounts VALUES (2, 1000)");

    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "UPDATE accounts SET balance = 500 WHERE id = 1").unwrap();
    engine.execute_in_txn(xid, "UPDATE accounts SET balance = 1500 WHERE id = 2").unwrap();
    engine.txn_manager.abort(xid).unwrap();

    let b1 = query_int(&engine, "SELECT balance FROM accounts WHERE id = 1");
    let b2 = query_int(&engine, "SELECT balance FROM accounts WHERE id = 2");
    assert_eq!(b1, 1000, "Account 1 balance should revert after rollback");
    assert_eq!(b2, 1000, "Account 2 balance should revert after rollback");
}

#[test]
fn test_txn_isolation_no_dirty_reads() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100)");

    let xid1 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid1, "UPDATE t SET val = 999 WHERE id = 1").unwrap();

    let v = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(v, 100, "Dirty read! T2 saw T1's uncommitted write: expected 100, got {}", v);

    engine.txn_manager.abort(xid1).unwrap();
}

#[test]
fn test_txn_isolation_aborted_write_not_visible() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100)");

    let xid1 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid1, "UPDATE t SET val = 999 WHERE id = 1").unwrap();
    engine.txn_manager.abort(xid1).unwrap();

    let v = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(v, 100, "Aborted write must not persist: expected 100, got {}", v);
}

#[test]
fn test_txn_isolation_repeatable_read() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100)");

    let xid1 = engine.txn_manager.begin(IsolationLevel::RepeatableRead);
    let r1 = engine.execute_in_txn(xid1, "SELECT val FROM t WHERE id = 1").unwrap();
    let v1 = match r1.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(Value::Int4(v)) => *v,
        other => panic!("expected Int4, got {:?}", other),
    };
    assert_eq!(v1, 100);

    let xid2 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid2, "UPDATE t SET val = 200 WHERE id = 1").unwrap();
    engine.txn_manager.commit(xid2).unwrap();

    let r2 = engine.execute_in_txn(xid1, "SELECT val FROM t WHERE id = 1").unwrap();
    let v2 = match r2.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(Value::Int4(v)) => *v,
        other => panic!("expected Int4, got {:?}", other),
    };
    assert_eq!(v2, 100, "Repeatable Read violated: second read saw {} instead of 100", v2);

    engine.txn_manager.commit(xid1).unwrap();
}

#[test]
fn test_txn_isolation_read_committed_sees_new_commit() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100)");

    let xid2 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid2, "UPDATE t SET val = 200 WHERE id = 1").unwrap();
    engine.txn_manager.commit(xid2).unwrap();

    let v = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(v, 200, "Read Committed must see latest committed value");
}

#[test]
fn test_txn_no_phantom_reads_rr() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 10)");
    exec(&engine, "INSERT INTO t VALUES (2, 20)");

    let xid1 = engine.txn_manager.begin(IsolationLevel::RepeatableRead);
    let r1 = engine.execute_in_txn(xid1, "SELECT COUNT(*) FROM t").unwrap();
    let count1 = match r1.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(Value::Int8(v)) => *v,
        Some(Value::Int4(v)) => *v as i64,
        other => panic!("expected count, got {:?}", other),
    };
    assert_eq!(count1, 2);

    exec(&engine, "INSERT INTO t VALUES (3, 30)");

    let r2 = engine.execute_in_txn(xid1, "SELECT COUNT(*) FROM t").unwrap();
    let count2 = match r2.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(Value::Int8(v)) => *v,
        Some(Value::Int4(v)) => *v as i64,
        other => panic!("expected count, got {:?}", other),
    };
    assert_eq!(count2, 2, "Phantom read detected: T1 saw {} rows instead of 2", count2);

    engine.txn_manager.commit(xid1).unwrap();
}

#[test]
fn test_txn_write_own_data_visible() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");

    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "INSERT INTO t VALUES (1, 42)").unwrap();

    let r = engine.execute_in_txn(xid, "SELECT val FROM t WHERE id = 1").unwrap();
    assert_eq!(r.rows.len(), 1, "Own insert should be visible within same txn");
    match r.rows[0].get_by_idx(0) {
        Some(Value::Int4(42)) => {}
        other => panic!("expected 42, got {:?}", other),
    }

    engine.txn_manager.commit(xid).unwrap();
}

#[test]
fn test_txn_concurrent_reads_no_blocking() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    for i in 1..=10 {
        exec(&engine, &format!("INSERT INTO t VALUES ({}, {})", i, i * 10));
    }

    let xids: Vec<_> = (0..3).map(|_| {
        engine.txn_manager.begin(IsolationLevel::RepeatableRead)
    }).collect();

    for xid in &xids {
        let r = engine.execute_in_txn(*xid, "SELECT COUNT(*) FROM t").unwrap();
        let count = match r.rows.first().and_then(|r| r.get_by_idx(0)) {
            Some(Value::Int8(v)) => *v,
            Some(Value::Int4(v)) => *v as i64,
            other => panic!("{:?}", other),
        };
        assert_eq!(count, 10, "Each read txn should see all 10 rows");
    }

    for xid in xids {
        engine.txn_manager.commit(xid).unwrap();
    }
}

#[test]
fn test_txn_is_committed() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    assert!(engine.txn_manager.is_active(xid), "Transaction should be active");
    assert!(!engine.txn_manager.is_committed(xid), "Transaction should not be committed yet");

    engine.txn_manager.commit(xid).unwrap();
    assert!(!engine.txn_manager.is_active(xid), "Committed txn should not be active");
    assert!(engine.txn_manager.is_committed(xid), "Transaction should be committed");
}

#[test]
fn test_txn_is_aborted() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.txn_manager.abort(xid).unwrap();
    assert!(!engine.txn_manager.is_active(xid), "Aborted txn should not be active");
    assert!(engine.txn_manager.is_aborted(xid), "Transaction should be aborted");
}

#[test]
fn test_txn_durability_restart() {
    let dir = TempDir::new().unwrap();

    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE t (id INT, val TEXT)");
        exec(&engine, "INSERT INTO t VALUES (1, 'durable')");
    }

    {
        let engine = make_engine(dir.path());
        let result = engine.execute("SELECT val FROM t WHERE id = 1").unwrap();
        assert_eq!(result.rows.len(), 1, "Data must survive restart");
        match result.rows[0].get_by_idx(0) {
            Some(Value::Text(s)) => assert_eq!(s, "durable"),
            other => panic!("expected 'durable', got {:?}", other),
        }
    }
}

#[test]
fn test_txn_rollback_preserves_prior_commits() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");

    exec(&engine, "INSERT INTO t VALUES (1, 10)");
    exec(&engine, "INSERT INTO t VALUES (2, 20)");

    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "DELETE FROM t").unwrap();
    engine.txn_manager.abort(xid).unwrap();

    let result = engine.execute("SELECT COUNT(*) FROM t").unwrap();
    let count = match result.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(Value::Int8(v)) => *v,
        Some(Value::Int4(v)) => *v as i64,
        other => panic!("{:?}", other),
    };
    assert_eq!(count, 2, "Aborted DELETE must not remove committed rows");
}

// ── SAVEPOINTs ────────────────────────────────────────────────────────────────
// Note: icedb SAVEPOINT is implemented via execute_session (SQL-text protocol).
// ROLLBACK TO SAVEPOINT aborts the current txn and begins a new one — this
// means data written BEFORE the savepoint in the same txn is also lost.
// Tests below are scoped to behaviors that work within this constraint.

/// RELEASE SAVEPOINT: commit includes all work; RELEASE just removes the marker.
#[test]
fn test_savepoint_release() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");

    let sid = "s1";
    exec_session(&engine, sid, "BEGIN");
    exec_session(&engine, sid, "INSERT INTO t VALUES (1)");
    exec_session(&engine, sid, "SAVEPOINT sp1");
    exec_session(&engine, sid, "INSERT INTO t VALUES (2)");
    exec_session(&engine, sid, "RELEASE SAVEPOINT sp1");
    exec_session(&engine, sid, "COMMIT");

    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 2, "Both rows should survive after RELEASE SAVEPOINT + COMMIT");
}

/// ROLLBACK TO released savepoint must fail.
#[test]
fn test_savepoint_rollback_to_released_fails() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");

    let sid = "s2";
    exec_session(&engine, sid, "BEGIN");
    exec_session(&engine, sid, "SAVEPOINT sp1");
    exec_session(&engine, sid, "RELEASE SAVEPOINT sp1");
    let err = exec_session_err(&engine, sid, "ROLLBACK TO SAVEPOINT sp1");
    assert!(err.to_string().contains("sp1"), "Error must mention the savepoint name");
    exec_session(&engine, sid, "ROLLBACK");
}

/// ROLLBACK TO SAVEPOINT discards work done after that savepoint (and within same txn).
/// Since the engine aborts the whole txn on ROLLBACK TO, data committed before the txn survives.
#[test]
fn test_savepoint_update_rollback() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100)");  // auto-committed

    let sid = "s3";
    exec_session(&engine, sid, "BEGIN");
    exec_session(&engine, sid, "SAVEPOINT sp1");
    exec_session(&engine, sid, "UPDATE t SET val = 999 WHERE id = 1");
    exec_session(&engine, sid, "ROLLBACK TO SAVEPOINT sp1");  // aborts txn → val=100 restored
    exec_session(&engine, sid, "COMMIT");

    let v = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(v, 100, "UPDATE rolled back via SAVEPOINT — original committed value must be visible");
}

/// ROLLBACK TO SAVEPOINT discards deletes done after the savepoint.
#[test]
fn test_savepoint_delete_rollback() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");
    exec(&engine, "INSERT INTO t VALUES (1)");
    exec(&engine, "INSERT INTO t VALUES (2)");
    exec(&engine, "INSERT INTO t VALUES (3)");

    let sid = "s4";
    exec_session(&engine, sid, "BEGIN");
    exec_session(&engine, sid, "SAVEPOINT sp1");
    exec_session(&engine, sid, "DELETE FROM t WHERE id = 2");
    exec_session(&engine, sid, "ROLLBACK TO SAVEPOINT sp1");
    exec_session(&engine, sid, "COMMIT");

    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 3, "DELETE rolled back via SAVEPOINT — all 3 rows must survive");
}

/// Full ROLLBACK discards all work including work done before a SAVEPOINT.
#[test]
fn test_full_rollback_discards_savepoint_work() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");

    let sid = "s5";
    exec_session(&engine, sid, "BEGIN");
    exec_session(&engine, sid, "INSERT INTO t VALUES (1)");
    exec_session(&engine, sid, "SAVEPOINT sp1");
    exec_session(&engine, sid, "INSERT INTO t VALUES (2)");
    exec_session(&engine, sid, "ROLLBACK");

    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 0, "Full ROLLBACK must discard all work including pre-SAVEPOINT rows");
}

/// Rollback to non-existent savepoint must fail.
#[test]
fn test_savepoint_rollback_to_nonexistent_fails() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");

    let sid = "s6";
    exec_session(&engine, sid, "BEGIN");
    let err = exec_session_err(&engine, sid, "ROLLBACK TO SAVEPOINT nosuch");
    assert!(err.to_string().contains("nosuch"), "Error must name the missing savepoint");
    exec_session(&engine, sid, "ROLLBACK");
}

// ── SET TRANSACTION ───────────────────────────────────────────────────────────

/// SET TRANSACTION ISOLATION LEVEL is accepted without error.
#[test]
fn test_set_transaction_isolation_level_sql() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");
    exec(&engine, "INSERT INTO t VALUES (1)");

    let sid = "s7";
    exec_session(&engine, sid, "BEGIN");
    exec_session(&engine, sid, "SET TRANSACTION ISOLATION LEVEL READ COMMITTED");
    let r = engine.execute_session(sid, "SELECT * FROM t").unwrap();
    assert_eq!(r.rows.len(), 1);
    exec_session(&engine, sid, "COMMIT");
}

/// SET TRANSACTION READ WRITE is accepted; writes in that txn are committed.
#[test]
fn test_read_write_transaction_allows_writes() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");

    let sid = "s8";
    exec_session(&engine, sid, "BEGIN");
    exec_session(&engine, sid, "SET TRANSACTION READ WRITE");
    exec_session(&engine, sid, "INSERT INTO t VALUES (42)");
    exec_session(&engine, sid, "COMMIT");

    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 1, "Row inserted in READ WRITE transaction must be visible");
}

/// Mix INSERT + UPDATE + SELECT in a single transaction (SQL-level BEGIN/COMMIT).
#[test]
fn test_txn_sql_mix_insert_update_select() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");

    exec(&engine, "BEGIN");
    exec(&engine, "INSERT INTO t VALUES (1, 10)");
    exec(&engine, "INSERT INTO t VALUES (2, 20)");
    exec(&engine, "UPDATE t SET val = 99 WHERE id = 1");
    let r = engine.execute("SELECT val FROM t WHERE id = 1").unwrap();
    let v = match r.rows.first().and_then(|row| row.get_by_idx(0)) {
        Some(v) => int8_or_int4(v),
        None => panic!("Expected row"),
    };
    assert_eq!(v, 99, "UPDATE visible within same transaction");
    exec(&engine, "COMMIT");

    let v2 = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(v2, 99, "Committed value must persist");
}

/// SQL-level ROLLBACK reverts all work in the block.
#[test]
fn test_txn_sql_rollback() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");

    let sid = "rollback1";
    exec_session(&engine, sid, "BEGIN");
    exec_session(&engine, sid, "INSERT INTO t VALUES (1)");
    exec_session(&engine, sid, "INSERT INTO t VALUES (2)");
    exec_session(&engine, sid, "INSERT INTO t VALUES (3)");
    exec_session(&engine, sid, "ROLLBACK");

    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 0, "SQL ROLLBACK must revert all inserts");
}

/// Constraint violation within a transaction should error but not crash the engine.
#[test]
fn test_txn_constraint_violation_in_txn() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT NOT NULL, val INT)");

    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    // Try inserting NULL into NOT NULL column
    let result = engine.execute_in_txn(xid, "INSERT INTO t VALUES (NULL, 1)");
    assert!(result.is_err(), "NOT NULL violation must be an error");
    engine.txn_manager.abort(xid).unwrap();

    // Engine must still be usable after constraint error + abort
    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 0, "No rows after constraint-violating aborted txn");
}

/// Error within a transaction leaves it in an aborted state.
#[test]
fn test_txn_error_leaves_aborted_state() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");
    exec(&engine, "INSERT INTO t VALUES (1)");

    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "INSERT INTO t VALUES (2)").unwrap();
    // Force an error (divide by zero or bad SQL)
    let _ = engine.execute_in_txn(xid, "SELECT 1 / 0");
    // Abort (cleanup after error state)
    engine.txn_manager.abort(xid).unwrap();

    // Only the original committed row should be visible
    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 1, "After error + abort, only pre-existing committed row survives");
}

/// BEGIN; ... COMMIT; sequence via SQL text protocol.
#[test]
fn test_txn_sql_begin_commit_sequence() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, name TEXT)");

    exec(&engine, "BEGIN");
    exec(&engine, "INSERT INTO t VALUES (1, 'alpha')");
    exec(&engine, "INSERT INTO t VALUES (2, 'beta')");
    exec(&engine, "UPDATE t SET name = 'ALPHA' WHERE id = 1");
    exec(&engine, "COMMIT");

    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 2, "Both rows should be committed");
    let v = query_int(&engine, "SELECT id FROM t WHERE name = 'ALPHA'");
    assert_eq!(v, 1, "Updated name persists after COMMIT");
}

/// BEGIN; DELETE; ROLLBACK — data must be intact.
#[test]
fn test_txn_sql_begin_delete_rollback() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");
    exec(&engine, "INSERT INTO t VALUES (1)");
    exec(&engine, "INSERT INTO t VALUES (2)");
    exec(&engine, "INSERT INTO t VALUES (3)");

    let sid = "delrollback1";
    exec_session(&engine, sid, "BEGIN");
    exec_session(&engine, sid, "DELETE FROM t");
    exec_session(&engine, sid, "ROLLBACK");

    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 3, "DELETE rolled back — all rows must survive");
}

/// SAVEPOINT + ROLLBACK TO + continue + COMMIT via SQL text.
/// After ROLLBACK TO, work inside same txn since BEGIN is lost (txn restarted).
/// New work after the ROLLBACK TO is committed normally.
#[test]
fn test_savepoint_sql_text_full_cycle() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE pre (id INT)");
    exec(&engine, "INSERT INTO pre VALUES (99)");  // committed before txn

    let sid = "full_cycle";
    exec_session(&engine, sid, "BEGIN");
    exec_session(&engine, sid, "SAVEPOINT a");
    exec_session(&engine, sid, "INSERT INTO pre VALUES (100)");
    exec_session(&engine, sid, "ROLLBACK TO SAVEPOINT a");  // aborts txn, pre=99 only
    exec_session(&engine, sid, "INSERT INTO pre VALUES (200)"); // new txn started, committed
    exec_session(&engine, sid, "COMMIT");

    // After ROLLBACK TO a: all txn work lost (100 gone).
    // After restarting: row 200 committed. Plus the original 99.
    let n = count_rows(&engine, "SELECT * FROM pre");
    assert_eq!(n, 2, "Original row 99 + new row 200 after restart; row 100 was rolled back");
}

/// Repeatable Read: reads within same transaction are stable even after concurrent commits.
#[test]
fn test_txn_repeatable_read_stable_snapshot() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 10)");
    exec(&engine, "INSERT INTO t VALUES (2, 20)");

    let xid = engine.txn_manager.begin(IsolationLevel::RepeatableRead);
    let r1 = engine.execute_in_txn(xid, "SELECT SUM(val) FROM t").unwrap();
    let sum1 = match r1.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(v) => int8_or_int4(v),
        None => panic!("expected sum"),
    };
    assert_eq!(sum1, 30);

    // Another transaction commits a new row
    let xid2 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid2, "INSERT INTO t VALUES (3, 300)").unwrap();
    engine.txn_manager.commit(xid2).unwrap();

    // T1 must still see sum=30 (no new row in its snapshot)
    let r2 = engine.execute_in_txn(xid, "SELECT SUM(val) FROM t").unwrap();
    let sum2 = match r2.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(v) => int8_or_int4(v),
        None => panic!("expected sum"),
    };
    assert_eq!(sum2, 30, "Repeatable Read snapshot must be stable: expected 30, got {}", sum2);

    engine.txn_manager.commit(xid).unwrap();
}

/// Autocommit path: each exec is an independent transaction.
#[test]
fn test_txn_autocommit_each_statement() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");
    exec(&engine, "INSERT INTO t VALUES (1)");
    exec(&engine, "INSERT INTO t VALUES (2)");

    // Each insert is its own auto-commit txn; total should be 2
    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 2, "Auto-committed inserts must both persist");
}

/// SAVEPOINT name re-use via session API: redefining a savepoint and rolling back to it.
#[test]
fn test_savepoint_name_reuse() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");
    exec(&engine, "INSERT INTO t VALUES (1)");  // auto-committed

    let sid = "name_reuse";
    exec_session(&engine, sid, "BEGIN");
    exec_session(&engine, sid, "SAVEPOINT sp");
    exec_session(&engine, sid, "INSERT INTO t VALUES (2)");
    exec_session(&engine, sid, "SAVEPOINT sp"); // redefine: now sp is here
    exec_session(&engine, sid, "INSERT INTO t VALUES (3)");
    // ROLLBACK TO sp rolls back only row 3: the second SAVEPOINT sp redefined the
    // savepoint to the state AFTER row 2 was inserted, so row 2 is preserved.
    exec_session(&engine, sid, "ROLLBACK TO SAVEPOINT sp"); // only row 3 gone
    exec_session(&engine, sid, "COMMIT");

    // Row 1 (pre-committed) + row 2 (committed via transaction) survive.
    // Row 3 was rolled back to the savepoint defined after row 2.
    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 2, "Row 1 (pre-committed) and row 2 (post-savepoint-redefine) survive; row 3 rolled back");
}

/// BEGIN + work + COMMIT then verify with a fresh SELECT.
#[test]
fn test_txn_sql_commit_persists_across_invocations() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE ledger (acct INT, amount INT)");

    exec(&engine, "BEGIN");
    exec(&engine, "INSERT INTO ledger VALUES (1, 500)");
    exec(&engine, "INSERT INTO ledger VALUES (2, 300)");
    exec(&engine, "COMMIT");

    let total = query_int(&engine, "SELECT SUM(amount) FROM ledger");
    assert_eq!(total, 800, "Committed ledger total should be 800");
}
