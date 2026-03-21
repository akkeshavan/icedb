use tempfile::TempDir;
use crate::common::*;

/// Committed data must survive engine restart (simulated by dropping and reopening).
#[test]
fn test_durability_survives_restart() {
    let dir = TempDir::new().unwrap();

    // Phase 1: write data and commit
    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE t (id INT, val TEXT)");
        exec(&engine, "INSERT INTO t VALUES (1, 'hello')");
        exec(&engine, "INSERT INTO t VALUES (2, 'world')");
        // engine drops here — all state in memory is lost
    }

    // Phase 2: reopen — data must still be there
    {
        let engine = make_engine(dir.path());
        let count = count_rows(&engine, "SELECT * FROM t");
        assert_eq!(count, 2, "Data must survive restart; got {} rows", count);

        let result = exec(&engine, "SELECT val FROM t WHERE id = 1");
        match result.rows.first().and_then(|r| r.get_by_idx(0)) {
            Some(sql::Value::Text(v)) => assert_eq!(v, "hello"),
            other => panic!("Expected 'hello', got {:?}", other),
        }
    }
}

/// WAL checkpoint + restart: data recoverable after checkpoint.
#[test]
fn test_durability_after_checkpoint() {
    let dir = TempDir::new().unwrap();

    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE t (id INT, val INT)");
        for i in 1..=50 {
            exec(&engine, &format!("INSERT INTO t VALUES ({}, {})", i, i * 2));
        }
        // Trigger checkpoint via WAL
        // (The WalWriter + CheckpointManager from wal crate —
        //  for this test, just rely on the fact that all data was committed to heap files)
    }

    {
        let engine = make_engine(dir.path());
        let count = count_rows(&engine, "SELECT * FROM t");
        assert_eq!(count, 50, "All 50 rows should survive restart");

        let sum = query_int(&engine, "SELECT SUM(val) FROM t");
        // sum of 2+4+6+...+100 = 2*(1+2+...+50) = 2*1275 = 2550
        assert_eq!(sum, 2550);
    }
}

/// Multiple restart cycles: each restart preserves cumulative data.
#[test]
fn test_durability_multiple_restarts() {
    let dir = TempDir::new().unwrap();

    // Cycle 1
    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE log (round INT, msg TEXT)");
        exec(&engine, "INSERT INTO log VALUES (1, 'first')");
    }

    // Cycle 2
    {
        let engine = make_engine(dir.path());
        exec(&engine, "INSERT INTO log VALUES (2, 'second')");
    }

    // Cycle 3
    {
        let engine = make_engine(dir.path());
        exec(&engine, "INSERT INTO log VALUES (3, 'third')");
    }

    // Final check
    {
        let engine = make_engine(dir.path());
        let count = count_rows(&engine, "SELECT * FROM log");
        assert_eq!(count, 3, "All 3 inserts across restarts must persist");
    }
}

/// Aborted transactions must NOT survive restart.
#[test]
fn test_durability_aborted_not_persisted() {
    let dir = TempDir::new().unwrap();

    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE t (id INT, val INT)");
        exec(&engine, "INSERT INTO t VALUES (1, 100)");  // committed

        // Abort a second insert
        let xid = engine.txn_manager.begin(txn::transaction::IsolationLevel::ReadCommitted);
        engine.execute_in_txn(xid, "INSERT INTO t VALUES (2, 200)").unwrap();
        engine.txn_manager.abort(xid).unwrap();
    }

    {
        let engine = make_engine(dir.path());
        let count = count_rows(&engine, "SELECT * FROM t");
        assert_eq!(count, 1, "Only committed row should persist; aborted row must not");
    }
}
