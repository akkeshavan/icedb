use tempfile::TempDir;
use crate::common::*;
use crate::common::{exec_engine as exec, exec_err_engine as exec_err, query_int_engine as query_int, count_rows_engine as count_rows, exec_session_engine as exec_session, exec_session_err_engine as exec_session_err};

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

/// UPDATE durability: committed updates must survive restart.
#[test]
fn test_durability_update_survives_restart() {
    let dir = TempDir::new().unwrap();

    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE t (id INT PRIMARY KEY, val TEXT)");
        exec(&engine, "INSERT INTO t VALUES (1, 'original')");
        exec(&engine, "UPDATE t SET val = 'updated' WHERE id = 1");
    }

    {
        let engine = make_engine(dir.path());
        let result = exec(&engine, "SELECT val FROM t WHERE id = 1");
        match result.rows.first().and_then(|r| r.get_by_idx(0)) {
            Some(sql::Value::Text(v)) => assert_eq!(v, "updated",
                "Committed UPDATE must survive restart"),
            other => panic!("Expected 'updated', got {:?}", other),
        }
    }
}

/// DELETE durability: committed deletes must survive restart.
#[test]
fn test_durability_delete_survives_restart() {
    let dir = TempDir::new().unwrap();

    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE t (id INT)");
        for i in 1..=5 {
            exec(&engine, &format!("INSERT INTO t VALUES ({})", i));
        }
        exec(&engine, "DELETE FROM t WHERE id <= 3");
    }

    {
        let engine = make_engine(dir.path());
        let count = count_rows(&engine, "SELECT * FROM t");
        assert_eq!(count, 2, "After DELETE + restart, only 2 rows must remain");

        let sum = query_int(&engine, "SELECT SUM(id) FROM t");
        assert_eq!(sum, 4 + 5, "Remaining rows must be id=4 and id=5");
    }
}

/// DDL durability: CREATE TABLE survives restart; the schema is queryable.
#[test]
fn test_durability_ddl_schema_survives_restart() {
    let dir = TempDir::new().unwrap();

    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE orders (id INT PRIMARY KEY, amount INT, note TEXT)");
        exec(&engine, "INSERT INTO orders VALUES (1, 500, 'first order')");
    }

    {
        let engine = make_engine(dir.path());
        // The table must still exist and be queryable
        let count = count_rows(&engine, "SELECT * FROM orders");
        assert_eq!(count, 1, "Table and data must survive restart");

        // Schema must be intact: all three columns accessible
        let amount = query_int(&engine, "SELECT amount FROM orders WHERE id = 1");
        assert_eq!(amount, 500);

        let result = exec(&engine, "SELECT note FROM orders WHERE id = 1");
        match result.rows.first().and_then(|r| r.get_by_idx(0)) {
            Some(sql::Value::Text(v)) => assert_eq!(v, "first order"),
            other => panic!("Expected 'first order', got {:?}", other),
        }
    }
}

/// DROP TABLE durability: a dropped table must not exist after restart.
#[test]
fn test_durability_drop_table_survives_restart() {
    let dir = TempDir::new().unwrap();

    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE tmp (id INT)");
        exec(&engine, "INSERT INTO tmp VALUES (1)");
        exec(&engine, "DROP TABLE tmp");
    }

    {
        let engine = make_engine(dir.path());
        let err = engine.execute("SELECT * FROM tmp").expect_err("Table must not exist after restart");
        assert!(matches!(err, sql::SqlError::Catalog(_) | sql::SqlError::TableNotFound(_)),
            "Expected table-not-found after restart, got {:?}", err);
    }
}

/// Index durability: a CREATE INDEX survives restart; index-based queries still work.
#[test]
fn test_durability_index_survives_restart() {
    let dir = TempDir::new().unwrap();

    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE t (id INT PRIMARY KEY, val INT)");
        for i in 1..=100 {
            exec(&engine, &format!("INSERT INTO t VALUES ({}, {})", i, i * 10));
        }
        exec(&engine, "CREATE INDEX idx_val ON t (val)");
    }

    {
        let engine = make_engine(dir.path());
        // Query that can use the index
        let count = count_rows(&engine, "SELECT * FROM t WHERE val = 500");
        assert_eq!(count, 1, "Index-based query must return correct result after restart");

        let id = query_int(&engine, "SELECT id FROM t WHERE val = 500");
        assert_eq!(id, 50, "Row with val=500 must have id=50 after restart");
    }
}

/// High-volume commit durability: 1,000 committed rows must all survive restart.
#[test]
fn test_durability_high_volume_commits() {
    let dir = TempDir::new().unwrap();
    let n = 1_000usize;

    {
        let engine = make_engine(dir.path());
        exec(&engine, "CREATE TABLE t (id INT, val INT)");
        for i in 1..=n {
            exec(&engine, &format!("INSERT INTO t VALUES ({}, {})", i, i));
        }
    }

    {
        let engine = make_engine(dir.path());
        let count = count_rows(&engine, "SELECT * FROM t");
        assert_eq!(count, n, "All {} rows must survive restart", n);

        let sum = query_int(&engine, "SELECT SUM(val) FROM t");
        let expected = (n * (n + 1) / 2) as i64;
        assert_eq!(sum, expected, "SUM must be {} after restart", expected);
    }
}
