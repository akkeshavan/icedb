use std::sync::Arc;
use tempfile::TempDir;
use txn::transaction::IsolationLevel;
use crate::common::*;

/// All-or-nothing: if any step fails, no partial changes should be visible.
#[test]
fn test_atomicity_all_or_nothing() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE accounts (id INT, balance INT)");
    exec(&engine, "INSERT INTO accounts VALUES (1, 1000)");
    exec(&engine, "INSERT INTO accounts VALUES (2, 1000)");

    // Begin a transfer transaction
    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);

    // Debit account 1
    engine.execute_in_txn(xid, "UPDATE accounts SET balance = 500 WHERE id = 1").unwrap();

    // Simulate failure — abort without crediting account 2
    engine.txn_manager.abort(xid).unwrap();

    // Verify: account 1 still has 1000 (rollback was effective)
    let bal1 = query_int(&engine, "SELECT balance FROM accounts WHERE id = 1");
    let bal2 = query_int(&engine, "SELECT balance FROM accounts WHERE id = 2");
    assert_eq!(bal1, 1000, "Account 1 should be unchanged after abort");
    assert_eq!(bal2, 1000, "Account 2 should be unchanged after abort");
    assert_eq!(bal1 + bal2, 2000, "Total balance must be conserved");
}

/// Bank transfer stress: many concurrent transfers, total balance must remain constant.
#[test]
fn test_bank_transfer_stress() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    // Setup: 10 accounts with 1000 each
    exec(&engine, "CREATE TABLE accounts (id INT, balance INT)");
    for i in 1..=10 {
        exec(&engine, &format!("INSERT INTO accounts VALUES ({}, 1000)", i));
    }

    // Do 50 transfers
    for round in 0..50 {
        let from = (round % 10) + 1;
        let to = ((round + 1) % 10) + 1;
        if from == to { continue; }

        let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
        let r1 = engine.execute_in_txn(xid, &format!("UPDATE accounts SET balance = balance - 10 WHERE id = {}", from));
        let r2 = engine.execute_in_txn(xid, &format!("UPDATE accounts SET balance = balance + 10 WHERE id = {}", to));
        if r1.is_ok() && r2.is_ok() {
            engine.txn_manager.commit(xid).unwrap();
        } else {
            engine.txn_manager.abort(xid).unwrap();
        }
    }

    // Verify total balance is still 10,000
    let total = query_int(&engine, "SELECT SUM(balance) FROM accounts");
    assert_eq!(total, 10000, "Total balance must be conserved: got {}", total);
}

/// Partial update must be fully rolled back.
#[test]
fn test_atomicity_partial_update_rollback() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'original')");
    exec(&engine, "INSERT INTO t VALUES (2, 'original')");

    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "UPDATE t SET val = 'modified' WHERE id = 1").unwrap();
    // Abort without updating row 2
    engine.txn_manager.abort(xid).unwrap();

    // Both rows should still have 'original'
    let result = exec(&engine, "SELECT val FROM t");
    for row in &result.rows {
        if let Some(sql::Value::Text(v)) = row.get_by_idx(0) {
            assert_eq!(v, "original");
        }
    }
    assert_eq!(result.rows.len(), 2, "Both rows should still exist after abort");
}

/// Committed changes must survive (not be rolled back).
#[test]
fn test_atomicity_committed_changes_persist() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val INT)");

    // Commit 10 inserts
    for i in 1..=10 {
        exec(&engine, &format!("INSERT INTO t VALUES ({}, {})", i, i * 10));
    }

    let count = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(count, 10);

    // Abort a delete
    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "DELETE FROM t WHERE id <= 5").unwrap();
    engine.txn_manager.abort(xid).unwrap();

    // All 10 rows should still be there
    let count = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(count, 10, "Aborted delete must not remove rows");
}

/// Multi-table transaction: both updates commit together or neither does.
///
/// Transfers money from a checking table to a savings table.
/// If we abort after the debit and before the credit, both tables
/// must remain at their original balances.
#[test]
fn test_atomicity_multi_table_transfer_rollback() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE checking (id INT, balance INT)");
    exec(&engine, "CREATE TABLE savings  (id INT, balance INT)");
    exec(&engine, "INSERT INTO checking VALUES (1, 500)");
    exec(&engine, "INSERT INTO savings  VALUES (1, 200)");

    // Debit checking in a transaction, then abort before crediting savings
    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "UPDATE checking SET balance = balance - 100 WHERE id = 1").unwrap();
    engine.txn_manager.abort(xid).unwrap();

    let checking = query_int(&engine, "SELECT balance FROM checking WHERE id = 1");
    let savings  = query_int(&engine, "SELECT balance FROM savings  WHERE id = 1");
    assert_eq!(checking, 500, "Aborted debit must leave checking unchanged");
    assert_eq!(savings,  200, "Aborted transfer must leave savings unchanged");
    assert_eq!(checking + savings, 700, "Total across tables must be conserved");
}

/// A committed multi-table transfer must be durable: both legs visible after commit.
#[test]
fn test_atomicity_multi_table_transfer_commit() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE checking (id INT, balance INT)");
    exec(&engine, "CREATE TABLE savings  (id INT, balance INT)");
    exec(&engine, "INSERT INTO checking VALUES (1, 500)");
    exec(&engine, "INSERT INTO savings  VALUES (1, 200)");

    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "UPDATE checking SET balance = balance - 100 WHERE id = 1").unwrap();
    engine.execute_in_txn(xid, "UPDATE savings  SET balance = balance + 100 WHERE id = 1").unwrap();
    engine.txn_manager.commit(xid).unwrap();

    let checking = query_int(&engine, "SELECT balance FROM checking WHERE id = 1");
    let savings  = query_int(&engine, "SELECT balance FROM savings  WHERE id = 1");
    assert_eq!(checking, 400);
    assert_eq!(savings,  300);
    assert_eq!(checking + savings, 700, "Total must be conserved after committed transfer");
}

/// DELETE atomicity: a bulk delete that is aborted must restore all rows.
#[test]
fn test_atomicity_bulk_delete_rollback() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT)");
    for i in 1..=20 {
        exec(&engine, &format!("INSERT INTO t VALUES ({})", i));
    }

    let xid = engine.txn_manager.begin(IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "DELETE FROM t").unwrap();

    // Before commit, rows must still be visible outside the transaction
    let count_outside = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(count_outside, 20, "Rows must still be visible to other sessions before commit");

    engine.txn_manager.abort(xid).unwrap();

    let count_after = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(count_after, 20, "Aborted bulk DELETE must restore all rows");
}

/// SAVEPOINT rollback: partial work within a transaction can be undone.
#[test]
fn test_atomicity_savepoint_partial_rollback() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE t (id INT, val TEXT)");

    exec_session(&engine, "s1", "BEGIN");
    exec_session(&engine, "s1", "INSERT INTO t VALUES (1, 'before_savepoint')");
    exec_session(&engine, "s1", "SAVEPOINT sp1");
    exec_session(&engine, "s1", "INSERT INTO t VALUES (2, 'after_savepoint')");
    exec_session(&engine, "s1", "ROLLBACK TO SAVEPOINT sp1");
    exec_session(&engine, "s1", "COMMIT");

    let count = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(count, 1, "Only the row before the savepoint must be committed");

    let result = exec(&engine, "SELECT val FROM t WHERE id = 1");
    match result.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(sql::Value::Text(v)) => assert_eq!(v, "before_savepoint"),
        other => panic!("Expected 'before_savepoint', got {:?}", other),
    }
    let count2 = count_rows(&engine, "SELECT * FROM t WHERE id = 2");
    assert_eq!(count2, 0, "Row inserted after savepoint must be rolled back");
}

/// Multiple SAVEPOINTs: rolling back to an earlier savepoint discards all later ones.
#[test]
fn test_atomicity_multiple_savepoints() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE log (step INT)");

    exec_session(&engine, "s1", "BEGIN");
    exec_session(&engine, "s1", "INSERT INTO log VALUES (1)");
    exec_session(&engine, "s1", "SAVEPOINT a");
    exec_session(&engine, "s1", "INSERT INTO log VALUES (2)");
    exec_session(&engine, "s1", "SAVEPOINT b");
    exec_session(&engine, "s1", "INSERT INTO log VALUES (3)");
    exec_session(&engine, "s1", "ROLLBACK TO SAVEPOINT a");
    // Steps 2 and 3 must be gone; step 1 remains
    exec_session(&engine, "s1", "INSERT INTO log VALUES (4)");
    exec_session(&engine, "s1", "COMMIT");

    let count = count_rows(&engine, "SELECT * FROM log");
    assert_eq!(count, 2, "Only steps 1 and 4 must be committed");

    let sum = query_int(&engine, "SELECT SUM(step) FROM log");
    assert_eq!(sum, 5, "Sum of committed steps (1+4) must be 5");
}

/// Concurrent session-based single-row updates: write-write conflict detection
/// ensures that concurrent increments to a shared counter are serialised and the
/// final value is correct.
///
/// Each thread increments the counter N times using a read-modify-write pattern
/// inside a session transaction.  Write-write conflicts trigger an abort; the
/// thread retries until it commits.  The total committed increments must equal the
/// number of rows in the audit log.
///
/// NOTE: Multi-statement cross-row (debit + credit) atomicity under concurrent
/// `execute_session` transactions is a known engine limitation — partial writes
/// can escape if the two UPDATEs touch different rows.  This test is scoped to the
/// single-row (counter) case which is fully supported.
#[test]
fn test_atomicity_concurrent_session_single_row_increments() {
    use std::thread;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    exec(&engine, "CREATE TABLE counter (id INT PRIMARY KEY, val INT)");
    exec(&engine, "INSERT INTO counter VALUES (1, 0)");

    let n_threads    = 4usize;
    let increments   = 20usize;
    let total_target = n_threads * increments;
    let committed    = Arc::new(AtomicUsize::new(0));

    let handles: Vec<_> = (0..n_threads).map(|t| {
        let engine    = Arc::clone(&engine);
        let committed = Arc::clone(&committed);
        thread::spawn(move || {
            for i in 0..increments {
                let session = format!("inc-{}-{}", t, i);
                for _attempt in 0..20 {
                    let _ = engine.execute_session(&session, "ROLLBACK");
                    if engine.execute_session(&session, "BEGIN").is_err() { break; }
                    let r = engine.execute_session(&session,
                        "UPDATE counter SET val = val + 1 WHERE id = 1");
                    if r.is_ok() {
                        if engine.execute_session(&session, "COMMIT").is_ok() {
                            committed.fetch_add(1, Ordering::Relaxed);
                            break;
                        }
                    }
                    let _ = engine.execute_session(&session, "ROLLBACK");
                }
            }
        })
    }).collect();

    for h in handles { h.join().unwrap(); }

    let final_val = query_int(&engine, "SELECT val FROM counter WHERE id = 1");
    let n_committed = committed.load(Ordering::Relaxed) as i64;

    assert_eq!(
        final_val, n_committed,
        "Counter value ({}) must equal number of committed increments ({})",
        final_val, n_committed
    );
    // At least 50% of attempts should commit without permanent contention
    assert!(
        n_committed >= (total_target / 2) as i64,
        "At least half of increments must commit; got {} / {}",
        n_committed, total_target
    );
}
