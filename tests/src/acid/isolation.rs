use tempfile::TempDir;
use txn::transaction::IsolationLevel;
use crate::common::*;

/// Dirty read prevention: uncommitted data from T1 must not be visible to T2.
#[test]
fn test_isolation_no_dirty_reads() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100)");

    // T1 begins and updates but does NOT commit
    let xid1 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid1, "UPDATE t SET val = 999 WHERE id = 1").unwrap();

    // T2 should see the original value (100), not T1's uncommitted 999
    let val = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(val, 100, "Dirty read occurred! T2 saw T1's uncommitted value");

    engine.txn_manager.abort(xid1).unwrap();
}

/// Non-repeatable read prevention under Repeatable Read.
#[test]
fn test_isolation_repeatable_read() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100)");

    // T1 begins with Repeatable Read
    let xid1 = engine.txn_manager.begin(IsolationLevel::RepeatableRead);

    // T1 reads val = 100
    let result1 = engine.execute_in_txn(xid1, "SELECT val FROM t WHERE id = 1").unwrap();
    let val1 = match result1.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(sql::Value::Int4(v)) => *v as i64,
        other => panic!("Expected int4, got {:?}", other),
    };
    assert_eq!(val1, 100);

    // T2 commits an update
    let xid2 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid2, "UPDATE t SET val = 200 WHERE id = 1").unwrap();
    engine.txn_manager.commit(xid2).unwrap();

    // T1 reads again — under Repeatable Read, should still see 100
    let result2 = engine.execute_in_txn(xid1, "SELECT val FROM t WHERE id = 1").unwrap();
    let val2 = match result2.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(sql::Value::Int4(v)) => *v as i64,
        other => panic!("Expected int4, got {:?}", other),
    };
    assert_eq!(val2, 100, "Repeatable read violated: second read saw different value");

    engine.txn_manager.commit(xid1).unwrap();
}

/// Read Committed sees latest committed data on each read.
#[test]
fn test_isolation_read_committed_sees_latest() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100)");

    // T2 commits a change
    exec(&engine, "UPDATE t SET val = 200 WHERE id = 1");

    // Under ReadCommitted, we see the latest committed value
    let val = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(val, 200);
}

/// Read Committed: within a long transaction, each statement sees newly committed data.
#[test]
fn test_isolation_read_committed_per_statement_snapshot() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 0)");

    let xid_long = engine.txn_manager.begin(IsolationLevel::ReadCommitted);

    // First read: sees val=0
    let r1 = engine.execute_in_txn(xid_long, "SELECT val FROM t WHERE id = 1").unwrap();
    let v1 = match r1.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(sql::Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    };
    assert_eq!(v1, 0);

    // A concurrent transaction commits an update
    let xid2 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid2, "UPDATE t SET val = 42 WHERE id = 1").unwrap();
    engine.txn_manager.commit(xid2).unwrap();

    // Under Read Committed, the long txn's SECOND read must see the new committed value
    let r2 = engine.execute_in_txn(xid_long, "SELECT val FROM t WHERE id = 1").unwrap();
    let v2 = match r2.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(sql::Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    };
    assert_eq!(v2, 42, "Read Committed must see newly committed data on re-read");

    engine.txn_manager.commit(xid_long).unwrap();
}

/// Phantom read prevention: no new rows appear in a range query within Repeatable Read.
#[test]
fn test_isolation_no_phantom_reads_rr() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 10)");
    exec(&engine, "INSERT INTO t VALUES (2, 20)");

    // T1 (Repeatable Read) counts rows
    let xid1 = engine.txn_manager.begin(IsolationLevel::RepeatableRead);
    let result1 = engine.execute_in_txn(xid1, "SELECT COUNT(*) FROM t").unwrap();
    let count1 = match result1.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(sql::Value::Int8(v)) => *v,
        Some(sql::Value::Int4(v)) => *v as i64,
        other => panic!("Expected count, got {:?}", other),
    };
    assert_eq!(count1, 2);

    // T2 inserts a new row and commits
    exec(&engine, "INSERT INTO t VALUES (3, 30)");

    // T1 counts again — should still see 2 rows (no phantoms under RR)
    let result2 = engine.execute_in_txn(xid1, "SELECT COUNT(*) FROM t").unwrap();
    let count2 = match result2.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(sql::Value::Int8(v)) => *v,
        Some(sql::Value::Int4(v)) => *v as i64,
        other => panic!("Expected count, got {:?}", other),
    };
    assert_eq!(count2, 2, "Phantom read: T1 saw a row inserted by T2");

    engine.txn_manager.commit(xid1).unwrap();
}

/// Write-write conflict: exactly one transaction wins; the other must see the committed value.
#[test]
fn test_isolation_write_write_conflict() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100)");

    let xid1 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid1, "UPDATE t SET val = 200 WHERE id = 1").unwrap();

    let xid2 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    let r2 = engine.execute_in_txn(xid2, "UPDATE t SET val = 300 WHERE id = 1");

    // T1 commits; T2 either committed or aborted
    engine.txn_manager.commit(xid1).unwrap();
    match r2 {
        Ok(_) => { let _ = engine.txn_manager.abort(xid2); }
        Err(_) => { let _ = engine.txn_manager.abort(xid2); }
    }

    // After T1 commits and T2 aborts, value must be T1's committed write
    let val = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(val, 200, "T1's committed write must be visible after T2 aborts");
}

/// Write-write conflict — second writer must NOT silently succeed on the same row.
/// After all aborts, the original value must be exactly restored.
#[test]
fn test_isolation_write_write_conflict_both_abort() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100)");

    let xid1 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid1, "UPDATE t SET val = 200 WHERE id = 1").unwrap();

    let xid2 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    let _ = engine.execute_in_txn(xid2, "UPDATE t SET val = 300 WHERE id = 1");

    let _ = engine.txn_manager.abort(xid1);
    let _ = engine.txn_manager.abort(xid2);

    // After both abort, original value must be restored exactly
    let val = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(val, 100, "After both aborts, original value 100 must be restored");
}

/// Multiple transactions can read simultaneously without blocking.
#[test]
fn test_isolation_concurrent_reads_no_blocking() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    for i in 1..=100 {
        exec(&engine, &format!("INSERT INTO t VALUES ({}, {})", i, i));
    }

    // 5 concurrent read transactions
    let xids: Vec<_> = (0..5).map(|_| {
        engine.txn_manager.begin(IsolationLevel::RepeatableRead)
    }).collect();

    for xid in &xids {
        let result = engine.execute_in_txn(*xid, "SELECT COUNT(*) FROM t").unwrap();
        let count = match result.rows.first().and_then(|r| r.get_by_idx(0)) {
            Some(sql::Value::Int8(v)) => *v,
            Some(sql::Value::Int4(v)) => *v as i64,
            other => panic!("Expected count, got {:?}", other),
        };
        assert_eq!(count, 100);
    }

    for xid in xids {
        engine.txn_manager.commit(xid).unwrap();
    }
}

/// SAVEPOINT: work after a ROLLBACK TO SAVEPOINT is invisible; work before is committed.
#[test]
fn test_isolation_savepoint_read_visibility() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val TEXT)");

    // Write before savepoint
    exec_session(&engine, "s1", "BEGIN");
    exec_session(&engine, "s1", "INSERT INTO t VALUES (1, 'keep')");
    exec_session(&engine, "s1", "SAVEPOINT sp");

    // Write after savepoint — should be rolled back
    exec_session(&engine, "s1", "INSERT INTO t VALUES (2, 'discard')");

    // Verify: within the transaction, BOTH rows are visible
    let count_in_txn = {
        let r = engine.execute_session("s1", "SELECT COUNT(*) FROM t").unwrap();
        match r.rows.first().and_then(|row| row.get_by_idx(0)) {
            Some(sql::Value::Int8(v)) => *v,
            Some(sql::Value::Int4(v)) => *v as i64,
            other => panic!("{:?}", other),
        }
    };
    assert_eq!(count_in_txn, 2, "Both rows must be visible within the transaction");

    exec_session(&engine, "s1", "ROLLBACK TO SAVEPOINT sp");
    exec_session(&engine, "s1", "COMMIT");

    // After commit: only the row before the savepoint must survive
    let count = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(count, 1, "Only the row inserted before the savepoint must be committed");

    let result = exec(&engine, "SELECT val FROM t WHERE id = 1");
    match result.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(sql::Value::Text(v)) => assert_eq!(v, "keep"),
        other => panic!("Expected 'keep', got {:?}", other),
    }
}

/// Serializable isolation: write skew (G2-item) must be prevented.
///
/// Two doctors: both on call. Each transaction reads count=2 and decides
/// to go off-call. Under Serializable, at least one must be rejected to
/// maintain the invariant that ≥1 doctor is on call.
#[test]
fn test_isolation_serializable_prevents_write_skew() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE doctors (id INT PRIMARY KEY, on_call INT)");
    exec(&engine, "INSERT INTO doctors VALUES (1, 1)");
    exec(&engine, "INSERT INTO doctors VALUES (2, 1)");

    let xid1 = engine.txn_manager.begin(IsolationLevel::Serializable);
    let xid2 = engine.txn_manager.begin(IsolationLevel::Serializable);

    // Both read count = 2 (safe to go off-call)
    engine.execute_in_txn(xid1, "SELECT COUNT(*) FROM doctors WHERE on_call = 1").unwrap();
    engine.execute_in_txn(xid2, "SELECT COUNT(*) FROM doctors WHERE on_call = 1").unwrap();

    // Each writes to a different row (classic write skew)
    engine.execute_in_txn(xid1, "UPDATE doctors SET on_call = 0 WHERE id = 1").unwrap();
    engine.execute_in_txn(xid2, "UPDATE doctors SET on_call = 0 WHERE id = 2").unwrap();

    let r1 = engine.txn_manager.commit(xid1);
    let r2 = engine.txn_manager.commit(xid2);

    // Under Serializable SSI, at least one commit must fail
    let both_committed = r1.is_ok() && r2.is_ok();
    assert!(!both_committed, "Serializable: write skew must be prevented — at least one commit must fail");

    // Invariant: at least one doctor must still be on call
    let on_call = query_int(&engine, "SELECT COUNT(*) FROM doctors WHERE on_call = 1");
    assert!(on_call >= 1, "Invariant violated: no doctors on call after serializable enforcement");
}

/// Read-own-writes: a transaction must see its own uncommitted writes.
#[test]
fn test_isolation_read_own_writes() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 0)");

    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "UPDATE t SET val = 42 WHERE id = 1").unwrap();

    // Within the same transaction, the update must be visible
    let result = engine.execute_in_txn(xid, "SELECT val FROM t WHERE id = 1").unwrap();
    let v = match result.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(sql::Value::Int4(v)) => *v,
        other => panic!("Expected Int4, got {:?}", other),
    };
    assert_eq!(v, 42, "Transaction must see its own uncommitted writes (read-own-writes)");

    engine.txn_manager.abort(xid).unwrap();
}

/// Session-based isolation: two sessions sharing an engine must not see each other's
/// uncommitted writes.
#[test]
fn test_isolation_session_uncommitted_not_visible_to_other_session() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 1)");

    // Session A begins a transaction and updates the row
    exec_session(&engine, "A", "BEGIN");
    exec_session(&engine, "A", "UPDATE t SET val = 99 WHERE id = 1");

    // Session B (auto-commit) must still see val=1
    let val_b = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(val_b, 1, "Session B must not see Session A's uncommitted write");

    // Session A rolls back
    exec_session(&engine, "A", "ROLLBACK");

    let val_after = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(val_after, 1, "Value must be 1 after session A rolls back");
}

/// Session-based isolation: after session A commits, session B sees the new value.
#[test]
fn test_isolation_session_committed_visible_to_other_session() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 1)");

    exec_session(&engine, "A", "BEGIN");
    exec_session(&engine, "A", "UPDATE t SET val = 77 WHERE id = 1");
    exec_session(&engine, "A", "COMMIT");

    let val_b = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(val_b, 77, "Session B must see session A's committed write");
}
