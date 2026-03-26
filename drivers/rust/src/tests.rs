#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use tempfile::TempDir;
    use crate::{open, open_pool, Connection, DriverError};
    use crate::arrow_output::rows_to_record_batch;
    use crate::async_connection::open_async;

    fn make_tmpdir() -> TempDir {
        tempfile::tempdir().expect("failed to create temp dir")
    }

    /// 1. Open an embedded db, create table, insert, query.
    #[test]
    fn test_open_embedded() {
        let dir = make_tmpdir();
        let engine = open(dir.path()).expect("open failed");
        engine.execute("CREATE TABLE t1 (id INTEGER, name TEXT)").expect("create table");
        engine.execute("INSERT INTO t1 VALUES (1, 'hello')").expect("insert");
        let result = engine.execute("SELECT id, name FROM t1").expect("select");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].get("id"), Some(&sql::Value::Int4(1)));
        assert_eq!(result.rows[0].get("name"), Some(&sql::Value::Text("hello".to_string())));
    }

    /// 2. Connection::execute with SELECT.
    #[test]
    fn test_connection_execute() {
        let dir = make_tmpdir();
        let engine = open(dir.path()).expect("open failed");
        engine.execute("CREATE TABLE t2 (val INTEGER)").expect("create table");
        engine.execute("INSERT INTO t2 VALUES (42)").expect("insert");
        let conn = Connection::new(engine);
        let result = conn.execute("SELECT val FROM t2").expect("select");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].get("val"), Some(&sql::Value::Int4(42)));
    }

    /// 3. begin, insert, commit, verify visible.
    #[test]
    fn test_connection_transaction() {
        let dir = make_tmpdir();
        let engine = open(dir.path()).expect("open failed");
        engine.execute("CREATE TABLE t3 (id INTEGER)").expect("create table");

        let mut conn = Connection::new(std::sync::Arc::clone(&engine));
        let xid = conn.begin().expect("begin");
        conn.execute_in_txn("INSERT INTO t3 VALUES (99)", xid).expect("insert");
        conn.commit().expect("commit");

        // Verify visible via a separate auto-commit query
        let result = engine.execute("SELECT id FROM t3").expect("select");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].get("id"), Some(&sql::Value::Int4(99)));
    }

    /// 4. begin, insert, rollback, verify NOT visible.
    #[test]
    fn test_connection_rollback() {
        let dir = make_tmpdir();
        let engine = open(dir.path()).expect("open failed");
        engine.execute("CREATE TABLE t4 (id INTEGER)").expect("create table");

        let mut conn = Connection::new(std::sync::Arc::clone(&engine));
        let xid = conn.begin().expect("begin");
        conn.execute_in_txn("INSERT INTO t4 VALUES (77)", xid).expect("insert");
        conn.rollback().expect("rollback");

        let result = engine.execute("SELECT id FROM t4").expect("select");
        assert_eq!(result.rows.len(), 0, "row should not be visible after rollback");
    }

    /// 5. begin a txn, drop Connection without commit → verify row not committed.
    #[test]
    fn test_connection_auto_rollback_on_drop() {
        let dir = make_tmpdir();
        let engine = open(dir.path()).expect("open failed");
        engine.execute("CREATE TABLE t5 (id INTEGER)").expect("create table");

        {
            let mut conn = Connection::new(std::sync::Arc::clone(&engine));
            let xid = conn.begin().expect("begin");
            conn.execute_in_txn("INSERT INTO t5 VALUES (55)", xid).expect("insert");
            // conn dropped here without commit → auto-rollback
        }

        let result = engine.execute("SELECT id FROM t5").expect("select");
        assert_eq!(result.rows.len(), 0, "row should be invisible after auto-rollback on drop");
    }

    /// 6. pool with max=2, acquire 2 connections, release, acquire again.
    #[test]
    fn test_pool_acquire_release() {
        let dir = make_tmpdir();
        let pool = open_pool(dir.path(), 2).expect("open_pool failed");
        pool.acquire().expect("acquire for create").execute("CREATE TABLE IF NOT EXISTS t6 (id INTEGER)").expect("create table via pool");

        let c1 = pool.acquire().expect("acquire 1");
        let c2 = pool.acquire().expect("acquire 2");

        // Use both
        c1.execute("SELECT 1").ok();
        c2.execute("SELECT 1").ok();

        // Return both by dropping
        drop(c1);
        drop(c2);

        // Should be able to acquire again
        let c3 = pool.acquire().expect("acquire after release");
        drop(c3);
    }

    /// 7. pool with max=1, acquire 2 → PoolExhausted on second.
    #[test]
    fn test_pool_exhausted() {
        let dir = make_tmpdir();
        let pool = open_pool(dir.path(), 1).expect("open_pool failed");

        let _c1 = pool.acquire().expect("first acquire should succeed");
        let result = pool.acquire();
        assert!(
            matches!(result, Err(DriverError::PoolExhausted)),
            "expected PoolExhausted"
        );
    }

    /// 8. open_pool, get connection, create table, insert 10 rows, query with WHERE, verify results.
    #[test]
    fn test_full_workflow() {
        let dir = make_tmpdir();
        let pool = open_pool(dir.path(), 5).expect("open_pool failed");

        // Create table
        {
            let conn = pool.acquire().expect("acquire");
            conn.execute("CREATE TABLE items (id INTEGER, score INTEGER)")
                .expect("create table");
        }

        // Insert 10 rows
        for i in 0..10i32 {
            let conn = pool.acquire().expect("acquire");
            let sql = format!("INSERT INTO items VALUES ({}, {})", i, i * 10);
            conn.execute(&sql).expect("insert");
        }

        // Query with WHERE
        {
            let conn = pool.acquire().expect("acquire");
            let result = conn.execute("SELECT id, score FROM items WHERE score > 50")
                .expect("select");
            // Rows where score > 50: i=6,7,8,9 → 4 rows
            assert_eq!(result.rows.len(), 4, "expected 4 rows with score > 50");
            for row in &result.rows {
                if let Some(sql::Value::Int4(score)) = row.get("score") {
                    assert!(*score > 50, "expected score > 50, got {score}");
                } else {
                    panic!("expected Int4 score value");
                }
            }
        }
    }

    // ── Arrow output tests ────────────────────────────────────────────────────

    /// 9. rows_to_record_batch on an empty slice returns an empty batch or a clear error —
    /// it must not panic.
    #[test]
    fn test_arrow_empty_input() {
        // Arrow 53 may reject empty-schema RecordBatch via try_new; either an empty
        // batch or a TypeConversion error is acceptable — the important invariant is
        // that the function does not panic.
        match rows_to_record_batch(&[]) {
            Ok(batch) => {
                assert_eq!(batch.num_rows(), 0);
            }
            Err(DriverError::TypeConversion(_)) => {
                // Arrow 53 returns an error for empty schema — acceptable
            }
            Err(e) => panic!("unexpected error from rows_to_record_batch(&[]): {:?}", e),
        }
    }

    /// 10. rows_to_record_batch preserves column count, row count, and types.
    #[test]
    fn test_arrow_basic_types() {
        let dir = make_tmpdir();
        let engine = open(dir.path()).expect("open failed");
        engine.execute("CREATE TABLE types_test (i INT, f FLOAT8, b BOOL, t TEXT)").unwrap();
        engine.execute("INSERT INTO types_test VALUES (1, 1.5, TRUE, 'hello')").unwrap();
        engine.execute("INSERT INTO types_test VALUES (2, 2.5, FALSE, 'world')").unwrap();

        let result = engine.execute("SELECT i, f, b, t FROM types_test ORDER BY i").unwrap();
        let batch = rows_to_record_batch(&result.rows).expect("rows_to_record_batch failed");

        assert_eq!(batch.num_rows(), 2);
        assert_eq!(batch.num_columns(), 4);

        // Column 0 is Int32
        assert_eq!(batch.schema().field(0).data_type(), &arrow::datatypes::DataType::Int32);
        // Column 1 is Float64
        assert_eq!(batch.schema().field(1).data_type(), &arrow::datatypes::DataType::Float64);
        // Column 2 is Boolean
        assert_eq!(batch.schema().field(2).data_type(), &arrow::datatypes::DataType::Boolean);
        // Column 3 is Utf8
        assert_eq!(batch.schema().field(3).data_type(), &arrow::datatypes::DataType::Utf8);
    }

    /// 11. rows_to_record_batch handles NULL values as array nulls (not panics).
    #[test]
    fn test_arrow_null_values() {
        let dir = make_tmpdir();
        let engine = open(dir.path()).expect("open failed");
        engine.execute("CREATE TABLE nullable (id INT, note TEXT)").unwrap();
        engine.execute("INSERT INTO nullable VALUES (1, NULL)").unwrap();
        engine.execute("INSERT INTO nullable VALUES (2, 'present')").unwrap();

        let result = engine.execute("SELECT id, note FROM nullable ORDER BY id").unwrap();
        let batch = rows_to_record_batch(&result.rows).expect("rows_to_record_batch with NULLs failed");

        assert_eq!(batch.num_rows(), 2);

        use arrow::array::Array;
        let note_col = batch.column(1)
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .expect("expected StringArray");
        assert!(note_col.is_null(0), "First row note must be null");
        assert!(!note_col.is_null(1), "Second row note must not be null");
        assert_eq!(note_col.value(1), "present");
    }

    /// 12. rows_to_record_batch on a large result set completes without error.
    #[test]
    fn test_arrow_large_result_set() {
        let dir = make_tmpdir();
        let engine = open(dir.path()).expect("open failed");
        engine.execute("CREATE TABLE big (id INT, val INT)").unwrap();
        for i in 0..1000i32 {
            engine.execute(&format!("INSERT INTO big VALUES ({}, {})", i, i * 2)).unwrap();
        }
        let result = engine.execute("SELECT id, val FROM big").unwrap();
        let batch = rows_to_record_batch(&result.rows).expect("large rows_to_record_batch failed");
        assert_eq!(batch.num_rows(), 1000);

        let id_col = batch.column(0)
            .as_any()
            .downcast_ref::<arrow::array::Int32Array>()
            .expect("expected Int32Array");
        // Spot-check a value
        let sum: i64 = (0..1000i32).map(|i| id_col.value(i as usize) as i64).sum();
        assert_eq!(sum, (0..1000i64).sum::<i64>());
    }

    // ── Async connection tests ────────────────────────────────────────────────

    /// 13. open_async + query returns the correct rows.
    ///
    /// Note: the driver's `open()` uses TransactionManager::new (no WAL recovery),
    /// so we create schema + data through the *same* engine instance inside the
    /// async block rather than relying on cross-engine durability.
    #[test]
    fn test_async_query() {
        let dir = make_tmpdir();
        let dir_path = dir.path().to_path_buf();

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let conn = open_async(dir_path).await.expect("open_async failed");
            conn.execute("CREATE TABLE async_t (id INT, val TEXT)").await.unwrap();
            conn.execute("INSERT INTO async_t VALUES (1, 'a')").await.unwrap();
            conn.execute("INSERT INTO async_t VALUES (2, 'b')").await.unwrap();

            let rows = conn.query("SELECT id, val FROM async_t ORDER BY id")
                .await
                .expect("async query failed");
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].get("id"), Some(&sql::Value::Int4(1)));
            assert_eq!(rows[1].get("id"), Some(&sql::Value::Int4(2)));
        });
    }

    /// 14. open_async execute returns the correct rows_affected count.
    #[test]
    fn test_async_execute_rows_affected() {
        let dir = make_tmpdir();
        let dir_path = dir.path().to_path_buf();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let conn = open_async(dir_path).await.expect("open_async failed");
            conn.execute("CREATE TABLE async_dml (id INT, val INT)").await.unwrap();
            for i in 0..5i32 {
                let sql = format!("INSERT INTO async_dml VALUES ({}, {})", i, i * 10);
                conn.execute(sql).await.unwrap();
            }
            let affected = conn.execute("UPDATE async_dml SET val = 0 WHERE val > 20")
                .await
                .expect("async execute failed");
            // val > 20 means i=3 (30) and i=4 (40) → 2 rows
            assert_eq!(affected, 2, "expected 2 rows affected by UPDATE");
        });
    }

    // ── Multi-session isolation tests ─────────────────────────────────────────

    /// 15. Two sessions sharing the same engine must not see each other's uncommitted writes.
    #[test]
    fn test_session_uncommitted_isolation() {
        let dir = make_tmpdir();
        let engine = open(dir.path()).expect("open failed");
        engine.execute("CREATE TABLE t (id INT, val INT)").unwrap();
        engine.execute("INSERT INTO t VALUES (1, 100)").unwrap();

        // Session A starts a transaction and updates
        engine.execute_session("A", "BEGIN").unwrap();
        engine.execute_session("A", "UPDATE t SET val = 999 WHERE id = 1").unwrap();

        // Session B (auto-commit) must still see 100
        let result = engine.execute_session("B", "SELECT val FROM t WHERE id = 1").unwrap();
        match result.rows.first().and_then(|r| r.get_by_idx(0)) {
            Some(sql::Value::Int4(v)) => assert_eq!(*v, 100,
                "Session B must not see session A's uncommitted write"),
            other => panic!("Expected Int4(100), got {:?}", other),
        }

        engine.execute_session("A", "ROLLBACK").unwrap();
    }

    /// 16. Pool connections perform sequential CRUD via acquire/release cycling.
    ///
    /// Verifies that the pool correctly round-trips connections (acquire → use →
    /// drop → re-acquire) and that all inserts are visible through a later
    /// connection from the same pool.
    #[test]
    fn test_pool_sequential_crud() {
        let dir = make_tmpdir();
        let pool = Arc::new(open_pool(dir.path(), 4).expect("open_pool failed"));
        pool.acquire().unwrap().execute("CREATE TABLE seq_crud (id INT PRIMARY KEY, val INT)").unwrap();

        // Insert 8 rows, acquiring and releasing a new pool connection each time
        for i in 0..8i32 {
            let conn = pool.acquire().expect("acquire");
            conn.execute(&format!("INSERT INTO seq_crud VALUES ({}, {})", i, i * 10))
                .expect("insert");
        }

        let conn = pool.acquire().unwrap();
        let result = conn.execute("SELECT COUNT(*) FROM seq_crud").unwrap();
        match result.rows.first().and_then(|r| r.get_by_idx(0)) {
            Some(sql::Value::Int8(v)) => assert_eq!(*v, 8, "expected 8 rows"),
            Some(sql::Value::Int4(v)) => assert_eq!(*v, 8, "expected 8 rows"),
            other => panic!("Expected count=8, got {:?}", other),
        }
    }

    /// 17. Pool with size=0 must return an error (not panic).
    #[test]
    fn test_pool_zero_size() {
        let dir = make_tmpdir();
        // A pool of size 0 means no connections can ever be acquired; must not panic
        let pool = open_pool(dir.path(), 0);
        match pool {
            Err(_) => { /* expected: invalid pool size */ }
            Ok(p) => {
                let result = p.acquire();
                assert!(matches!(result, Err(DriverError::PoolExhausted)),
                    "Zero-size pool must immediately return PoolExhausted");
            }
        }
    }
}
