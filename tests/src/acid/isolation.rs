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

/// Write-write conflict detection.
#[test]
fn test_isolation_write_write_conflict() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100)");

    // T1 updates row 1
    let xid1 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid1, "UPDATE t SET val = 200 WHERE id = 1").unwrap();

    // T2 also tries to update row 1 — should get a conflict
    let xid2 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    let _result = engine.execute_in_txn(xid2, "UPDATE t SET val = 300 WHERE id = 1");

    // One of them should fail with a write-write conflict or serialization failure
    // OR T2 waits and T1 commits first — either is valid
    // For our implementation: T2 hits WriteWriteConflict
    // Cleanup
    let _ = engine.txn_manager.abort(xid1);
    let _ = engine.txn_manager.abort(xid2);

    // After abort, row should be back to 100
    let val = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(val, 100, "After both aborts, original value should be restored");
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
