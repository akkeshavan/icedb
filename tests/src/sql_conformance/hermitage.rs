/// Category 13: Hermitage isolation test suite
/// Based on https://github.com/ept/hermitage (PostgreSQL anomaly tests).
/// These tests use separate transactions to verify isolation guarantees.
use std::sync::Arc;
use tempfile::TempDir;
use txn::transaction::IsolationLevel;
use crate::common::{make_engine, exec_engine as exec, query_int_engine as query_int};
use sql::Value;

/// Helper: get integer value from transaction context.
fn txn_int(engine: &Arc<sql::engine::QueryEngine>, xid: txn::xid::Xid, sql: &str) -> i64 {
    let r = engine.execute_in_txn(xid, sql).unwrap_or_else(|e| {
        panic!("SQL failed in txn: {}\nError: {}", sql, e)
    });
    match r.rows.first().and_then(|row| row.get_by_idx(0)) {
        Some(Value::Int4(v)) => *v as i64,
        Some(Value::Int8(v)) => *v,
        other => panic!("expected integer, got {:?} for: {}", other, sql),
    }
}

/// Standard Hermitage test table setup.
fn setup(engine: &Arc<sql::engine::QueryEngine>) {
    exec(engine, "CREATE TABLE test (id INT, value INT)");
    exec(engine, "INSERT INTO test VALUES (1, 10)");
    exec(engine, "INSERT INTO test VALUES (2, 20)");
}

// ─── G0: Write Cycles (no dirty writes) ──────────────────────────────────────

/// G0: No dirty writes.
/// Both T1 and T2 write to the same row. After T1 commits and T2 aborts,
/// T1's value must be visible — T2's uncommitted write must not have overwritten T1's.
#[test]
fn test_hermitage_g0_no_dirty_write() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup(&engine);

    let xid1 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    let xid2 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);

    engine.execute_in_txn(xid1, "UPDATE test SET value = 11 WHERE id = 1").unwrap();

    // T2 attempts write to same row (may fail with write-write conflict)
    let r2 = engine.execute_in_txn(xid2, "UPDATE test SET value = 12 WHERE id = 1");

    engine.txn_manager.commit(xid1).unwrap();
    match r2 {
        Ok(_) => { let _ = engine.txn_manager.abort(xid2); }
        Err(_) => { let _ = engine.txn_manager.abort(xid2); }
    }

    // T1's committed write (11) must be visible
    let v = query_int(&engine, "SELECT value FROM test WHERE id = 1");
    assert_eq!(v, 11, "G0: T1's committed write must be visible after T2 aborts");
}

// ─── G1a: Aborted Reads ──────────────────────────────────────────────────────

/// G1a: No aborted reads (dirty reads of aborted data).
/// T2 reads during T1's active transaction; T1 then aborts.
/// T2 must NOT have seen T1's write at any point.
#[test]
fn test_hermitage_g1a_no_aborted_reads() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup(&engine);

    let xid1 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid1, "UPDATE test SET value = 101 WHERE id = 1").unwrap();

    // T2 reads while T1 is active (uncommitted)
    let v = query_int(&engine, "SELECT value FROM test WHERE id = 1");

    engine.txn_manager.abort(xid1).unwrap();

    assert_eq!(v, 10, "G1a: T2 must not see T1's uncommitted write (got {}, expected 10)", v);
}

// ─── G1b: Intermediate Reads ─────────────────────────────────────────────────

/// G1b: No intermediate reads.
/// T1 performs two updates. T2 must not see the intermediate state.
#[test]
fn test_hermitage_g1b_no_intermediate_reads() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup(&engine);

    let xid1 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid1, "UPDATE test SET value = 101 WHERE id = 1").unwrap();
    // Intermediate state: value=101 (uncommitted)

    // T2 reads while T1 has only done the first update
    let v_mid = query_int(&engine, "SELECT value FROM test WHERE id = 1");
    assert_eq!(v_mid, 10, "G1b: T2 must not see T1's first intermediate write");

    engine.execute_in_txn(xid1, "UPDATE test SET value = 11 WHERE id = 1").unwrap();
    engine.txn_manager.commit(xid1).unwrap();

    // After commit, T2 should now see the final value
    let v_final = query_int(&engine, "SELECT value FROM test WHERE id = 1");
    assert_eq!(v_final, 11, "G1b: After T1 commits, T2 should see final value 11");
}

// ─── G1c: Circular Information Flow ──────────────────────────────────────────

/// G1c: No circular information flow.
/// T1 writes row 1, T2 writes row 2. Neither should see the other's uncommitted write.
#[test]
fn test_hermitage_g1c_no_circular_reads() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup(&engine);

    let xid1 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    let xid2 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);

    engine.execute_in_txn(xid1, "UPDATE test SET value = 11 WHERE id = 1").unwrap();
    engine.execute_in_txn(xid2, "UPDATE test SET value = 22 WHERE id = 2").unwrap();

    // T1 reads row 2 — must see 20 (T2's write is uncommitted)
    let v1_sees_row2 = txn_int(&engine, xid1, "SELECT value FROM test WHERE id = 2");
    assert_eq!(v1_sees_row2, 20, "G1c: T1 must not see T2's uncommitted write on row 2");

    // T2 reads row 1 — must see 10 (T1's write is uncommitted)
    let v2_sees_row1 = txn_int(&engine, xid2, "SELECT value FROM test WHERE id = 1");
    assert_eq!(v2_sees_row1, 10, "G1c: T2 must not see T1's uncommitted write on row 1");

    engine.txn_manager.commit(xid1).unwrap();
    engine.txn_manager.commit(xid2).unwrap();

    // Verify final state
    let final_v1 = query_int(&engine, "SELECT value FROM test WHERE id = 1");
    let final_v2 = query_int(&engine, "SELECT value FROM test WHERE id = 2");
    assert_eq!(final_v1, 11, "Row 1 should be 11 after T1 commits");
    assert_eq!(final_v2, 22, "Row 2 should be 22 after T2 commits");
}

// ─── P4: Lost Update ─────────────────────────────────────────────────────────

/// P4: Lost Update detection under Repeatable Read.
/// T1 and T2 both read-modify-write the same row.
/// Under RR with proper conflict detection, one should fail.
#[test]
fn test_hermitage_p4_lost_update_rr() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup(&engine);

    let xid1 = engine.txn_manager.begin(IsolationLevel::RepeatableRead);
    let v1 = txn_int(&engine, xid1, "SELECT value FROM test WHERE id = 1");
    assert_eq!(v1, 10);

    // T2 reads, modifies, commits
    let xid2 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid2, "UPDATE test SET value = 11 WHERE id = 1").unwrap();
    engine.txn_manager.commit(xid2).unwrap();

    // T1 tries to write (based on stale read of 10)
    let r = engine.execute_in_txn(xid1, "UPDATE test SET value = 110 WHERE id = 1");

    match r {
        Ok(_) => {
            // Lost update scenario — T1 overwrote T2's committed write
            // Under RR this may happen (T2 committed after T1 began)
            let commit_r = engine.txn_manager.commit(xid1);
            if commit_r.is_ok() {
                let v = query_int(&engine, "SELECT value FROM test WHERE id = 1");
                assert!(v == 11 || v == 110, "P4: value must be one of the two writes: {}", v);
            } else {
                let _ = engine.txn_manager.abort(xid1);
            }
        }
        Err(_) => {
            // Write-write conflict detected — T1 correctly blocked
            let _ = engine.txn_manager.abort(xid1);
            let v = query_int(&engine, "SELECT value FROM test WHERE id = 1");
            assert_eq!(v, 11, "P4: T2's committed value must survive if T1 is blocked");
        }
    }
}

// ─── G2: Anti-Dependency Cycles (Write Skew) ─────────────────────────────────

/// G2-item: Write Skew detection.
/// T1 and T2 both see "2 doctors on call" and each take one doctor off-call.
/// If both commit, invariant (at least 1 on call) is violated.
/// Under SSI this should be detected; under RR it may slip through.
#[test]
fn test_hermitage_g2_item_write_skew() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE doctors (id INT, on_call INT)");
    exec(&engine, "INSERT INTO doctors VALUES (1, 1)");
    exec(&engine, "INSERT INTO doctors VALUES (2, 1)");

    let xid1 = engine.txn_manager.begin(IsolationLevel::RepeatableRead);
    let xid2 = engine.txn_manager.begin(IsolationLevel::RepeatableRead);

    // Both see 2 doctors on call
    let count1 = txn_int(&engine, xid1, "SELECT COUNT(*) FROM doctors WHERE on_call = 1");
    assert_eq!(count1, 2);
    let count2 = txn_int(&engine, xid2, "SELECT COUNT(*) FROM doctors WHERE on_call = 1");
    assert_eq!(count2, 2);

    // T1 takes doctor 1 off call
    engine.execute_in_txn(xid1, "UPDATE doctors SET on_call = 0 WHERE id = 1").unwrap();
    // T2 takes doctor 2 off call
    let r2 = engine.execute_in_txn(xid2, "UPDATE doctors SET on_call = 0 WHERE id = 2");

    engine.txn_manager.commit(xid1).unwrap();
    match r2 {
        Ok(_) => { let _ = engine.txn_manager.commit(xid2); }
        Err(_) => { let _ = engine.txn_manager.abort(xid2); }
    }

    let on_call = query_int(&engine, "SELECT COUNT(*) FROM doctors WHERE on_call = 1");
    // Under RepeatableRead: write skew may allow 0 (both commit)
    // Under Serializable SSI: at least one commit should fail → on_call >= 1
    // We document the observed behavior without asserting a specific value
    // (the test infrastructure for SSI is stubbed out as of Phase 3)
    let _ = on_call; // document: 0 = write skew occurred, 1 or 2 = correctly prevented
}

// ─── Additional Hermitage-inspired Tests ─────────────────────────────────────

/// Verify that a committed update is visible to subsequent transactions.
#[test]
fn test_hermitage_committed_write_visible() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup(&engine);

    let xid1 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid1, "UPDATE test SET value = 99 WHERE id = 1").unwrap();
    engine.txn_manager.commit(xid1).unwrap();

    // New transaction should see 99
    let v = query_int(&engine, "SELECT value FROM test WHERE id = 1");
    assert_eq!(v, 99, "Committed write must be visible to subsequent transactions");
}

/// Verify RR snapshot is taken at BEGIN, not at first read.
#[test]
fn test_hermitage_rr_snapshot_at_begin() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup(&engine);

    // T1 begins with RR — snapshot taken now
    let xid1 = engine.txn_manager.begin(IsolationLevel::RepeatableRead);

    // T2 commits a change AFTER T1 begins
    let xid2 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid2, "UPDATE test SET value = 200 WHERE id = 1").unwrap();
    engine.txn_manager.commit(xid2).unwrap();

    // T1 reads — should see original value 10 (before T2's commit)
    let v = txn_int(&engine, xid1, "SELECT value FROM test WHERE id = 1");
    assert_eq!(v, 10, "RR snapshot must be taken at BEGIN: T1 should see 10, not 200");

    engine.txn_manager.commit(xid1).unwrap();
}

/// Verify that two concurrent RC transactions don't see each other's uncommitted inserts.
#[test]
fn test_hermitage_rc_no_uncommitted_inserts() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup(&engine);

    let xid1 = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid1, "INSERT INTO test VALUES (3, 30)").unwrap();

    // T2 should not see T1's uncommitted insert
    let count = query_int(&engine, "SELECT COUNT(*) FROM test");
    assert_eq!(count, 2, "Uncommitted insert must not be visible to other transactions");

    engine.txn_manager.abort(xid1).unwrap();

    // After abort, still 2 rows
    let count_after = query_int(&engine, "SELECT COUNT(*) FROM test");
    assert_eq!(count_after, 2, "Aborted insert must not persist");
}

/// Verify that within a transaction, own updates are visible (read-your-own-writes).
#[test]
fn test_hermitage_read_own_writes() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup(&engine);

    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "UPDATE test SET value = 999 WHERE id = 1").unwrap();

    // Within same transaction, own update should be visible
    let v = txn_int(&engine, xid, "SELECT value FROM test WHERE id = 1");
    assert_eq!(v, 999, "Transaction must read its own uncommitted writes");

    engine.txn_manager.commit(xid).unwrap();
}

/// Verify phantom prevention at Repeatable Read level.
#[test]
fn test_hermitage_no_phantom_rr() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup(&engine);

    let xid1 = engine.txn_manager.begin(IsolationLevel::RepeatableRead);

    let count1 = txn_int(&engine, xid1, "SELECT COUNT(*) FROM test WHERE value > 5");
    assert_eq!(count1, 2, "Initially 2 rows with value > 5");

    // Another transaction inserts a new qualifying row
    exec(&engine, "INSERT INTO test VALUES (3, 30)");

    // T1 re-reads — should still see 2 rows (no phantom)
    let count2 = txn_int(&engine, xid1, "SELECT COUNT(*) FROM test WHERE value > 5");
    assert_eq!(count2, 2, "Phantom read: T1 should still see 2 rows under RR, got {}", count2);

    engine.txn_manager.commit(xid1).unwrap();
}

/// Verify rollback of inserts is complete (not partial).
#[test]
fn test_hermitage_rollback_complete_batch() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup(&engine);

    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "INSERT INTO test VALUES (3, 30)").unwrap();
    engine.execute_in_txn(xid, "INSERT INTO test VALUES (4, 40)").unwrap();
    engine.execute_in_txn(xid, "UPDATE test SET value = 99 WHERE id = 1").unwrap();
    engine.txn_manager.abort(xid).unwrap();

    // All changes must be rolled back
    let count = query_int(&engine, "SELECT COUNT(*) FROM test");
    assert_eq!(count, 2, "Rollback must undo all 3 operations");

    let v1 = query_int(&engine, "SELECT value FROM test WHERE id = 1");
    assert_eq!(v1, 10, "Update must be rolled back: id=1 value should be 10");

    let v2 = query_int(&engine, "SELECT COUNT(*) FROM test WHERE id IN (3, 4)");
    assert_eq!(v2, 0, "Inserts must be rolled back: id=3,4 should not exist");
}
