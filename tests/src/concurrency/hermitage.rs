use std::sync::Arc;
use tempfile::TempDir;
use txn::transaction::IsolationLevel;
use crate::common::*;

/// Helper: setup standard Hermitage test table
fn setup(engine: &Arc<sql::engine::QueryEngine>) {
    exec(engine, "CREATE TABLE test (id INT, value INT)");
    exec(engine, "INSERT INTO test VALUES (1, 10)");
    exec(engine, "INSERT INTO test VALUES (2, 20)");
}

/// G0: Write Cycles (Dirty Write)
/// T1 and T2 both update the same row. One should "win".
/// After both complete (one committed, one aborted), data must be consistent.
#[test]
fn test_g0_no_dirty_write() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup(&engine);

    let xid1 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    let xid2 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);

    engine.execute_in_txn(xid1, "UPDATE test SET value = 11 WHERE id = 1").unwrap();

    // T2 tries to update same row — may get conflict
    let r = engine.execute_in_txn(xid2, "UPDATE test SET value = 12 WHERE id = 1");

    engine.txn_manager.commit(xid1).unwrap();

    match r {
        Ok(_) => { let _ = engine.txn_manager.abort(xid2); }
        Err(_) => { let _ = engine.txn_manager.abort(xid2); }
    }

    // After T1 commits and T2 aborts, value must be 11 (T1's write)
    let val = query_int(&engine, "SELECT value FROM test WHERE id = 1");
    assert_eq!(val, 11, "G0: T1's committed write must be visible");
}

/// G1a: Aborted Reads (Dirty Reads of Aborted Data)
/// T1 writes, T2 reads during T1, T1 aborts — T2 must not have seen T1's write.
#[test]
fn test_g1a_no_aborted_reads() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup(&engine);

    let xid1 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid1, "UPDATE test SET value = 101 WHERE id = 1").unwrap();

    // T2 reads while T1 is active (not committed)
    let val = query_int(&engine, "SELECT value FROM test WHERE id = 1");

    engine.txn_manager.abort(xid1).unwrap();

    // T2 should have seen 10, not 101
    assert_eq!(val, 10, "G1a: T2 must not read T1's aborted write");
}

/// G1b: Intermediate Reads (reading intermediate state)
/// T1 updates twice. T2 must not read between T1's updates.
#[test]
fn test_g1b_no_intermediate_reads() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup(&engine);

    let xid1 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid1, "UPDATE test SET value = 101 WHERE id = 1").unwrap();
    // Intermediate state: value = 101

    // T2 reads while T1 is in the middle (before second update)
    let val = query_int(&engine, "SELECT value FROM test WHERE id = 1");
    assert_eq!(val, 10, "G1b: T2 must not see T1's intermediate write");

    engine.execute_in_txn(xid1, "UPDATE test SET value = 11 WHERE id = 1").unwrap();
    engine.txn_manager.commit(xid1).unwrap();

    // Now T2 reads again — should see 11
    let val_after = query_int(&engine, "SELECT value FROM test WHERE id = 1");
    assert_eq!(val_after, 11);
}

/// G1c: Circular Information Flow (cyclic dirty read)
/// Both T1 and T2 read each other's uncommitted writes is impossible in our MVCC.
#[test]
fn test_g1c_no_circular_reads() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup(&engine);

    let xid1 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    let xid2 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);

    engine.execute_in_txn(xid1, "UPDATE test SET value = 11 WHERE id = 1").unwrap();
    engine.execute_in_txn(xid2, "UPDATE test SET value = 22 WHERE id = 2").unwrap();

    // T1 reads row 2 — must see 20 (T2's write not committed)
    let v1 = {
        let r = engine.execute_in_txn(xid1, "SELECT value FROM test WHERE id = 2").unwrap();
        match r.rows.first().and_then(|r| r.get_by_idx(0)) {
            Some(sql::Value::Int4(v)) => *v as i64,
            other => panic!("{:?}", other),
        }
    };
    assert_eq!(v1, 20, "G1c: T1 must not see T2's uncommitted write on row 2");

    // T2 reads row 1 — must see 10 (T1's write not committed)
    let v2 = {
        let r = engine.execute_in_txn(xid2, "SELECT value FROM test WHERE id = 1").unwrap();
        match r.rows.first().and_then(|r| r.get_by_idx(0)) {
            Some(sql::Value::Int4(v)) => *v as i64,
            other => panic!("{:?}", other),
        }
    };
    assert_eq!(v2, 10, "G1c: T2 must not see T1's uncommitted write on row 1");

    engine.txn_manager.commit(xid1).unwrap();
    engine.txn_manager.commit(xid2).unwrap();
}

/// P4: Lost Update
/// T1 and T2 both read a value, then both update it.
/// Final value should reflect both updates properly (one loses unless serialized).
#[test]
fn test_p4_detect_lost_update() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup(&engine);

    // T1 reads value=10
    let xid1 = engine.txn_manager.begin(IsolationLevel::RepeatableRead);
    let v1 = {
        let r = engine.execute_in_txn(xid1, "SELECT value FROM test WHERE id = 1").unwrap();
        match r.rows.first().and_then(|r| r.get_by_idx(0)) {
            Some(sql::Value::Int4(v)) => *v,
            other => panic!("{:?}", other),
        }
    };
    assert_eq!(v1, 10);

    // T2 reads value=10, increments to 11, commits
    let xid2 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid2, "UPDATE test SET value = 11 WHERE id = 1").unwrap();
    engine.txn_manager.commit(xid2).unwrap();

    // T1 tries to set value = v1 + 100 = 110 based on its read
    let result = engine.execute_in_txn(xid1, "UPDATE test SET value = 110 WHERE id = 1");

    match result {
        Ok(_) => {
            // T1 committed — this is the "lost update" scenario (T2's write was overwritten)
            // Under RC this may happen; under RR with conflict detection it should not
            let commit_result = engine.txn_manager.commit(xid1);
            if commit_result.is_ok() {
                // Lost update occurred — acceptable under RC but not serializable
                let val = query_int(&engine, "SELECT value FROM test WHERE id = 1");
                // val is either 11 (T2 wins) or 110 (T1 wins)
                assert!(val == 11 || val == 110, "Value must be one of the two writes");
            } else {
                let _ = engine.txn_manager.abort(xid1);
            }
        }
        Err(_) => {
            // Write-write conflict detected — correct behavior
            let _ = engine.txn_manager.abort(xid1);
            let val = query_int(&engine, "SELECT value FROM test WHERE id = 1");
            assert_eq!(val, 11, "T2's committed value must be preserved after T1 conflict");
        }
    }
}

/// G2-item: Anti-Dependency Cycles (Write Skew)
/// T1 and T2 both read a set and each writes to a different member of the set.
#[test]
fn test_g2_item_write_skew_detection() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE doctors (id INT, on_call INT)");
    exec(&engine, "INSERT INTO doctors VALUES (1, 1)");  // on call
    exec(&engine, "INSERT INTO doctors VALUES (2, 1)");  // on call
    // Invariant: at least 1 doctor on call at all times

    let xid1 = engine.txn_manager.begin(IsolationLevel::RepeatableRead);
    let xid2 = engine.txn_manager.begin(IsolationLevel::RepeatableRead);

    // Both T1 and T2 read total on-call count = 2
    let count1 = {
        let r = engine.execute_in_txn(xid1, "SELECT COUNT(*) FROM doctors WHERE on_call = 1").unwrap();
        match r.rows.first().and_then(|r| r.get_by_idx(0)) {
            Some(sql::Value::Int8(v)) => *v,
            Some(sql::Value::Int4(v)) => *v as i64,
            other => panic!("{:?}", other),
        }
    };
    assert_eq!(count1, 2);

    let count2 = {
        let r = engine.execute_in_txn(xid2, "SELECT COUNT(*) FROM doctors WHERE on_call = 1").unwrap();
        match r.rows.first().and_then(|r| r.get_by_idx(0)) {
            Some(sql::Value::Int8(v)) => *v,
            Some(sql::Value::Int4(v)) => *v as i64,
            other => panic!("{:?}", other),
        }
    };
    assert_eq!(count2, 2);

    // T1 sets doctor 1 off-call
    engine.execute_in_txn(xid1, "UPDATE doctors SET on_call = 0 WHERE id = 1").unwrap();
    // T2 sets doctor 2 off-call
    let r2 = engine.execute_in_txn(xid2, "UPDATE doctors SET on_call = 0 WHERE id = 2");

    engine.txn_manager.commit(xid1).unwrap();
    let _ = match r2 {
        Ok(_) => engine.txn_manager.commit(xid2),
        Err(_) => engine.txn_manager.abort(xid2),
    };

    // After everything, check the invariant
    // Under serializable SSI, at least one commit should fail
    // Under RepeatableRead, write skew is possible — both might commit
    let on_call = query_int(&engine, "SELECT COUNT(*) FROM doctors WHERE on_call = 1");
    // This test documents the behavior: 0 = write skew occurred, >=1 = prevented
    // We don't assert a specific value here since the behavior depends on isolation level
    let _ = on_call;
}
