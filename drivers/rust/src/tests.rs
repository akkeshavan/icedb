#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use crate::{open, open_pool, Connection, DriverError};

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
}
