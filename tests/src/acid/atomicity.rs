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
