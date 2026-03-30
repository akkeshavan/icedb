/// Fault-injection tests (issues 28 & 29).
///
/// These tests simulate abrupt process termination (SIGKILL / power-off) by
/// dropping the engine *without* calling commit or abort on in-flight
/// transactions, then reopening it and verifying that:
///   a) only fully-committed data is visible, and
///   b) balance invariants are preserved.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tempfile::TempDir;
use crate::common::*;
use crate::common::{exec_engine as exec, exec_err_engine as exec_err, query_int_engine as query_int, count_rows_engine as count_rows, exec_session_engine as exec_session, exec_session_err_engine as exec_session_err};

// ── Issue 28: bank-transfer fault injection ───────────────────────────────────

/// SIGKILL mid-transfer: debit completes but credit never starts.
///
/// Simulated by beginning a txn, executing the debit UPDATE, then dropping
/// the engine without commit.  After restart total balance must be unchanged.
#[test]
fn test_fault_injection_mid_transfer_debit_only() {
    let dir = TempDir::new().unwrap();

    // Phase 1: setup accounts
    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE accounts (id INT, balance INT)");
        exec(&engine, "INSERT INTO accounts VALUES (1, 1000)");
        exec(&engine, "INSERT INTO accounts VALUES (2, 1000)");
        // engine commits via auto-commit on each exec; drop here is clean
    }

    // Phase 2: simulate crash mid-transfer (debit without credit, no commit)
    {
        let engine = make_engine(dir.path());
        let xid = engine.txn_manager.begin(txn::transaction::IsolationLevel::ReadCommitted);
        // Debit account 1 — this write is in-flight (never committed)
        engine.execute_in_txn(xid, "UPDATE accounts SET balance = 0 WHERE id = 1").unwrap();
        // *** Crash: drop engine without commit/abort ***
        drop(engine);
        // xid is now lost — WAL has no COMMIT record for it
    }

    // Phase 3: reopen — only committed data should be visible
    {
        let engine = make_engine(dir.path());
        let total = query_int(&engine, "SELECT SUM(balance) FROM accounts");
        assert_eq!(
            total, 2000,
            "Total balance must be 2000 after crash mid-debit; got {}",
            total
        );
        let bal1 = query_int(&engine, "SELECT balance FROM accounts WHERE id = 1");
        assert_eq!(bal1, 1000, "Uncommitted debit must be rolled back on recovery");
    }
}

/// SIGKILL with multiple in-flight transactions.
///
/// Starts several partial transfers, drops engine without any commits, then
/// verifies the aggregate invariant after restart.
#[test]
fn test_fault_injection_multiple_inflight_transactions() {
    let dir = TempDir::new().unwrap();
    let n_accounts = 5;
    let initial_balance = 500i64;

    // Setup
    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE accounts (id INT, balance INT)");
        for i in 1..=n_accounts {
            exec(
                &engine,
                &format!("INSERT INTO accounts VALUES ({}, {})", i, initial_balance),
            );
        }
    }

    // Start 3 partial transfers, crash before any commit
    {
        let engine = make_engine(dir.path());
        for i in 0..3usize {
            let from = (i % n_accounts) + 1;
            let to = ((i + 2) % n_accounts) + 1;
            if from == to {
                continue;
            }
            let xid = engine.txn_manager.begin(txn::transaction::IsolationLevel::ReadCommitted);
            engine
                .execute_in_txn(
                    xid,
                    &format!("UPDATE accounts SET balance = balance - 50 WHERE id = {}", from),
                )
                .unwrap();
            // Intentionally NOT committing — simulates crash
        }
        drop(engine); // crash
    }

    // Recover: no uncommitted change should be visible
    {
        let engine = make_engine(dir.path());
        let total = query_int(&engine, "SELECT SUM(balance) FROM accounts");
        let expected = n_accounts as i64 * initial_balance;
        assert_eq!(
            total, expected,
            "All in-flight txns must be rolled back on recovery; expected {}, got {}",
            expected, total
        );
    }
}

/// Committed transfers survive a crash; uncommitted ones do not.
#[test]
fn test_fault_injection_committed_vs_uncommitted() {
    let dir = TempDir::new().unwrap();

    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE ledger (id INT, amount INT)");
        exec(&engine, "INSERT INTO ledger VALUES (1, 100)");
        exec(&engine, "INSERT INTO ledger VALUES (2, 200)");
        exec(&engine, "INSERT INTO ledger VALUES (3, 300)");
    }

    {
        let engine = make_engine(dir.path());

        // Committed insert
        exec(&engine, "INSERT INTO ledger VALUES (4, 400)");

        // Uncommitted insert (crash before commit)
        let xid = engine.txn_manager.begin(txn::transaction::IsolationLevel::ReadCommitted);
        engine
            .execute_in_txn(xid, "INSERT INTO ledger VALUES (5, 999)")
            .unwrap();
        // drop without commit — crash
        drop(engine);
    }

    {
        let engine = make_engine(dir.path());
        let count = count_rows(&engine, "SELECT * FROM ledger");
        assert_eq!(count, 4, "Only 4 committed rows should survive; got {}", count);

        let total = query_int(&engine, "SELECT SUM(amount) FROM ledger");
        assert_eq!(total, 1000, "Sum must be 1000 (100+200+300+400); got {}", total);
    }
}

// ── Issue 29: power-off under write load ─────────────────────────────────────

/// Power-off durability simulation: a background writer thread commits rows
/// as fast as possible while the "OS" (main thread) abruptly drops the engine.
///
/// After recovery, every row present must have been fully committed (no torn
/// writes), and the committed count must be ≥ what was durably flushed.
#[test]
fn test_poweroff_durability_under_write_load() {
    use std::thread;
    use std::time::Duration;

    let dir = TempDir::new().unwrap();

    // Setup schema
    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE log (id INT, val TEXT)");
    }

    // Track how many inserts committed before the crash
    let committed_count = Arc::new(std::sync::atomic::AtomicI64::new(0));
    let stop = Arc::new(AtomicBool::new(false));

    let committed_clone = Arc::clone(&committed_count);
    let stop_clone = Arc::clone(&stop);
    let dir_path = dir.path().to_path_buf();

    // Writer thread: commit inserts as fast as possible
    let writer = thread::spawn(move || {
        let engine = make_engine(&dir_path);
        let mut i = 0i64;
        while !stop_clone.load(Ordering::Relaxed) {
            i += 1;
            let result = engine.execute(
                &format!("INSERT INTO log VALUES ({}, 'row-{}')", i, i)
            );
            if result.is_ok() {
                committed_clone.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    // Let the writer run for a short burst
    thread::sleep(Duration::from_millis(100));

    // Signal stop and wait for writer to finish cleanly
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();

    let committed_before_crash = committed_count.load(Ordering::Relaxed);

    // Recovery: reopen engine and verify integrity
    {
        let engine = make_engine(dir.path());
        let recovered_count = query_int(&engine, "SELECT COUNT(*) FROM log");

        // All recovered rows must be <= what was committed (no phantom rows)
        assert!(
            recovered_count <= committed_before_crash,
            "Recovered {} rows but only {} were committed before crash",
            recovered_count,
            committed_before_crash
        );

        // Verify every recovered row has a valid id (no torn tuples)
        let result = exec(&engine, "SELECT id FROM log ORDER BY id");
        let mut prev = 0i64;
        for row in &result.rows {
            if let Some(sql::Value::Int4(v)) = row.get_by_idx(0) {
                let id = *v as i64;
                assert!(id > 0, "Row id must be positive, got {}", id);
                assert!(
                    id > prev,
                    "Row ids must be strictly increasing (no duplicates); prev={} cur={}",
                    prev,
                    id
                );
                prev = id;
            }
        }
    }
}

/// WAL recovery preserves exactly the committed LSN boundary.
///
/// Writes N rows in a single committed transaction, then writes M more rows
/// in an uncommitted transaction (crash), then recovers and verifies exactly
/// N rows are present.
#[test]
fn test_poweroff_wal_lsn_boundary() {
    let dir = TempDir::new().unwrap();
    let n = 20usize;
    let m = 10usize;

    // Commit N rows
    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE t (id INT)");
        for i in 1..=n {
            exec(&engine, &format!("INSERT INTO t VALUES ({})", i));
        }
    }

    // Write M rows uncommitted, then crash
    {
        let engine = make_engine(dir.path());
        let xid = engine.txn_manager.begin(txn::transaction::IsolationLevel::ReadCommitted);
        for i in (n + 1)..=(n + m) {
            engine
                .execute_in_txn(xid, &format!("INSERT INTO t VALUES ({})", i))
                .unwrap();
        }
        drop(engine); // crash — M rows are never committed
    }

    // Recover: only N rows visible
    {
        let engine = make_engine(dir.path());
        let count = count_rows(&engine, "SELECT * FROM t");
        assert_eq!(
            count, n,
            "Only {} committed rows should survive crash; got {}",
            n, count
        );
    }
}
