use icedb_driver::{open, open_pool, Connection, open_async, rows_to_record_batch};
use std::sync::Arc;

fn test_sync_crud() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open(dir.path()).unwrap();

    engine.execute("CREATE TABLE products (id INT, name TEXT, price FLOAT)").unwrap();
    engine.execute("INSERT INTO products VALUES (1, 'Widget', 9.99)").unwrap();
    engine.execute("INSERT INTO products VALUES (2, 'Gadget', 24.99)").unwrap();
    engine.execute("INSERT INTO products VALUES (3, 'Doohickey', 4.99)").unwrap();

    let result = engine.execute("SELECT id, name FROM products WHERE price < 15.0").unwrap();
    assert_eq!(result.rows.len(), 2, "expected 2 cheap products");
    println!("PASS sync_crud ({} rows)", result.rows.len());
}

fn test_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open(dir.path()).unwrap();
    engine.execute("CREATE TABLE accounts (id INT, balance FLOAT)").unwrap();
    engine.execute("INSERT INTO accounts VALUES (1, 1000.0)").unwrap();
    engine.execute("INSERT INTO accounts VALUES (2, 500.0)").unwrap();

    let mut conn = Connection::new(Arc::clone(&engine));
    let xid = conn.begin().unwrap();
    conn.execute_in_txn("UPDATE accounts SET balance = balance - 200.0 WHERE id = 1", xid).unwrap();
    conn.execute_in_txn("UPDATE accounts SET balance = balance + 200.0 WHERE id = 2", xid).unwrap();
    conn.commit().unwrap();

    let result = engine.execute("SELECT SUM(balance) FROM accounts").unwrap();
    println!("PASS transaction (total balance = {:?})", result.rows[0].get_by_idx(0));
}

fn test_pool() {
    let dir = tempfile::tempdir().unwrap();
    let pool = open_pool(dir.path(), 5).unwrap();

    pool.acquire().unwrap().execute("CREATE TABLE events (id INT, kind TEXT)").unwrap();
    for i in 0..10i32 {
        let conn = pool.acquire().unwrap();
        conn.execute(&format!("INSERT INTO events VALUES ({}, 'evt')", i)).unwrap();
    }
    let result = pool.acquire().unwrap().execute("SELECT COUNT(*) FROM events").unwrap();
    println!("PASS pool ({:?} events)", result.rows[0].get_by_idx(0));
}

fn test_arrow_output() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open(dir.path()).unwrap();
    engine.execute("CREATE TABLE nums (id INT, val FLOAT)").unwrap();
    for i in 0..5i32 {
        engine.execute(&format!("INSERT INTO nums VALUES ({}, {}.0)", i, i * 10)).unwrap();
    }
    let result = engine.execute("SELECT id, val FROM nums").unwrap();
    let batch = rows_to_record_batch(&result.rows).unwrap();
    assert_eq!(batch.num_rows(), 5);
    assert_eq!(batch.num_columns(), 2);
    println!("PASS arrow_output ({} rows, {} cols)", batch.num_rows(), batch.num_columns());
}

async fn test_async_inner() {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_async(dir.path().to_path_buf()).await.unwrap();
    conn.execute("CREATE TABLE async_test (id INT, msg TEXT)".to_string()).await.unwrap();
    conn.execute("INSERT INTO async_test VALUES (1, 'hello async')".to_string()).await.unwrap();
    let rows = conn.query("SELECT id, msg FROM async_test".to_string()).await.unwrap();
    assert_eq!(rows.len(), 1);
    println!("PASS async ({} row)", rows.len());
}

fn main() {
    println!("=== icedb Rust driver sandbox ===");
    test_sync_crud();
    test_transaction();
    test_pool();
    test_arrow_output();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(test_async_inner());

    println!("\nAll sandbox tests passed.");
}
