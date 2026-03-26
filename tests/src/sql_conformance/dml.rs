/// Category 8: DML (Data Manipulation Language) tests
/// INSERT, UPDATE, DELETE with RETURNING clause.
/// Expanded with PostgreSQL insert.sql, update.sql, delete.sql patterns.
use tempfile::TempDir;
use crate::common::{make_engine, exec, count_rows, query_int};
use sql::Value;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn get_int(v: &Value) -> i64 {
    match v {
        Value::Int4(n) => *n as i64,
        Value::Int8(n) => *n,
        other => panic!("expected integer value, got {:?}", other),
    }
}

fn get_float(v: &Value) -> f64 {
    match v {
        Value::Float8(f) => *f,
        Value::Float8(f) => *f,
        Value::Int4(i) => *i as f64,
        Value::Int8(i) => *i as f64,
        other => panic!("expected float value, got {:?}", other),
    }
}

// ── Original tests ────────────────────────────────────────────────────────────

#[test]
fn test_insert_single_row() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, name TEXT)");
    engine.execute("INSERT INTO t VALUES (1, 'Alice')").unwrap();
    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 1, "single INSERT should produce 1 row");
}

#[test]
fn test_insert_multi_row() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, name TEXT)");
    engine.execute("INSERT INTO t VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Carol')").unwrap();
    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 3, "multi-row INSERT should produce 3 rows");
}

#[test]
fn test_insert_column_list() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, name TEXT, active INT)");
    engine.execute("INSERT INTO t (id, name) VALUES (1, 'Alice')").unwrap();
    let result = engine.execute("SELECT id, name FROM t WHERE id = 1").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(1)) => {}
        other => panic!("expected id=1, got {:?}", other),
    }
}

#[test]
fn test_insert_returning_id_name() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, name TEXT)");

    let result = engine.execute("INSERT INTO t VALUES (42, 'foo') RETURNING id, name").unwrap();
    assert_eq!(result.rows.len(), 1, "RETURNING should return 1 row");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(42)) => {}
        other => panic!("expected id=42, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "foo"),
        other => panic!("expected 'foo', got {:?}", other),
    }
}

#[test]
fn test_insert_returning_star() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val FLOAT)");

    let result = engine.execute("INSERT INTO t VALUES (7, 3.14) RETURNING *").unwrap();
    assert_eq!(result.rows.len(), 1, "RETURNING * should return 1 row");
    assert_eq!(result.rows[0].values.len(), 2, "RETURNING * should have 2 columns");
}

#[test]
fn test_insert_multi_row_returning() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, name TEXT)");

    let result = engine.execute(
        "INSERT INTO t VALUES (1, 'Alpha'), (2, 'Beta'), (3, 'Gamma') RETURNING id"
    ).unwrap();
    assert_eq!(result.rows.len(), 3, "Multi-row INSERT RETURNING should return 3 rows");
}

#[test]
fn test_update_basic() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100)");
    exec(&engine, "INSERT INTO t VALUES (2, 200)");

    engine.execute("UPDATE t SET val = 999 WHERE id = 1").unwrap();

    let result = engine.execute("SELECT val FROM t WHERE id = 1").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(999)) => {}
        other => panic!("expected val=999 after UPDATE, got {:?}", other),
    }

    let result2 = engine.execute("SELECT val FROM t WHERE id = 2").unwrap();
    match result2.rows[0].get_by_idx(0) {
        Some(Value::Int4(200)) => {}
        other => panic!("expected val=200 (unchanged), got {:?}", other),
    }
}

#[test]
fn test_update_multiple_columns() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, a INT, b TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 10, 'old')");

    engine.execute("UPDATE t SET a = 99, b = 'new' WHERE id = 1").unwrap();

    let result = engine.execute("SELECT a, b FROM t WHERE id = 1").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(99)) => {}
        other => panic!("expected a=99, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "new"),
        other => panic!("expected b='new', got {:?}", other),
    }
}

#[test]
fn test_update_expression() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, price FLOAT)");
    exec(&engine, "INSERT INTO t VALUES (1, 10.0)");
    exec(&engine, "INSERT INTO t VALUES (2, 20.0)");

    engine.execute("UPDATE t SET price = price * 1.10").unwrap();

    let result = engine.execute("SELECT price FROM t WHERE id = 1").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => {
            assert!((v - 11.0).abs() < 0.001, "price should be 11.0, got {}", v);
        }
        other => panic!("expected Float8, got {:?}", other),
    }
}

#[test]
fn test_update_returning() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, price FLOAT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100.0)");
    exec(&engine, "INSERT INTO t VALUES (2, 200.0)");

    let result = engine.execute(
        "UPDATE t SET price = price * 1.10 WHERE id = 1 RETURNING id, price"
    ).unwrap();
    assert_eq!(result.rows.len(), 1, "UPDATE RETURNING should return 1 row");
    match result.rows[0].get_by_idx(1) {
        Some(Value::Float8(v)) => {
            assert!((v - 110.0).abs() < 0.001, "price should be 110.0, got {}", v);
        }
        other => panic!("expected Float8, got {:?}", other),
    }
}

#[test]
fn test_update_no_rows_matched() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 10)");

    engine.execute("UPDATE t SET val = 999 WHERE id = 999").unwrap();

    let v = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(v, 10, "No-op UPDATE should not change any rows");
}

#[test]
fn test_delete_with_where() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, name TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Carol')");

    engine.execute("DELETE FROM t WHERE id = 2").unwrap();

    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 2, "DELETE should leave 2 rows");

    let n2 = count_rows(&engine, "SELECT * FROM t WHERE name = 'Bob'");
    assert_eq!(n2, 0, "Bob should be deleted");
}

#[test]
fn test_delete_returning() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, name TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Carol')");

    let result = engine.execute("DELETE FROM t WHERE id > 2 RETURNING id, name").unwrap();
    assert_eq!(result.rows.len(), 1, "DELETE RETURNING should return 1 deleted row");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(3)) => {}
        other => panic!("expected id=3, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "Carol"),
        other => panic!("expected Carol, got {:?}", other),
    }
}

#[test]
fn test_delete_returning_star() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, name TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'x'), (2, 'y')");

    let result = engine.execute("DELETE FROM t WHERE id = 1 RETURNING *").unwrap();
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].values.len(), 2, "RETURNING * should have 2 columns");
}

#[test]
fn test_delete_all_rows() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");
    for i in 1..=5 {
        exec(&engine, &format!("INSERT INTO t VALUES ({})", i));
    }

    engine.execute("DELETE FROM t").unwrap();

    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 0, "DELETE without WHERE should remove all rows");
}

#[test]
fn test_delete_multiple_rows() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, category TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'a'), (2, 'b'), (3, 'a'), (4, 'c'), (5, 'a')");

    engine.execute("DELETE FROM t WHERE category = 'a'").unwrap();

    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 2, "Deleting 3 rows with category='a' should leave 2");
}

#[test]
fn test_update_all_rows() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, flag INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 0), (2, 0), (3, 0)");

    engine.execute("UPDATE t SET flag = 1").unwrap();

    let result = engine.execute("SELECT flag FROM t WHERE flag = 0").unwrap();
    assert_eq!(result.rows.len(), 0, "All rows should have flag=1 after UPDATE");
}

#[test]
fn test_insert_then_update_then_select() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'initial')");

    engine.execute("UPDATE t SET val = 'updated' WHERE id = 1").unwrap();

    let result = engine.execute("SELECT val FROM t WHERE id = 1").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "updated"),
        other => panic!("expected 'updated', got {:?}", other),
    }
}

#[test]
fn test_insert_then_delete_then_reinsert() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100)");
    exec(&engine, "DELETE FROM t WHERE id = 1");
    exec(&engine, "INSERT INTO t VALUES (1, 200)");

    let v = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(v, 200, "After delete+reinsert, val should be 200");
}

#[test]
fn test_update_set_null() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, note TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'some note')");

    engine.execute("UPDATE t SET note = NULL WHERE id = 1").unwrap();

    let result = engine.execute("SELECT note FROM t WHERE id = 1").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) => {}
        other => panic!("expected NULL after setting note=NULL, got {:?}", other),
    }
}

// ── New tests ─────────────────────────────────────────────────────────────────

/// INSERT with DEFAULT for a nullable column: omitted column receives NULL.
#[test]
fn test_insert_default_nullable_column() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, name TEXT, score INT)");
    // Omit score — it should default to NULL
    engine.execute("INSERT INTO t (id, name) VALUES (1, 'Alice')").unwrap();
    let result = engine.execute("SELECT score FROM t WHERE id = 1").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        other => panic!("Omitted nullable column should be NULL, got {:?}", other),
    }
}

/// INSERT with explicit DEFAULT keyword for a column with a default value.
#[test]
fn test_insert_explicit_default_keyword() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT DEFAULT 42)");
    engine.execute("INSERT INTO t (id) VALUES (1)").unwrap();
    let v = query_int(&engine, "SELECT val FROM t WHERE id = 1");
    assert_eq!(v, 42, "DEFAULT value should be 42");
}

/// INSERT ... SELECT: populate a table from a query result.
#[test]
fn test_insert_select() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE src (id INT, val INT)");
    exec(&engine, "INSERT INTO src VALUES (1, 10), (2, 20), (3, 30)");
    exec(&engine, "CREATE TABLE dst (id INT, val INT)");

    engine.execute("INSERT INTO dst SELECT id, val FROM src WHERE val > 10").unwrap();

    let n = count_rows(&engine, "SELECT * FROM dst");
    assert_eq!(n, 2, "INSERT ... SELECT should insert 2 rows (val=20 and val=30)");
}

/// INSERT multiple rows with single VALUES list, verify row count.
#[test]
fn test_insert_values_multi_tuple() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");
    engine.execute("INSERT INTO t VALUES (1), (2), (3), (4), (5)").unwrap();
    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 5, "Five-tuple VALUES insert should produce 5 rows");
}

/// INSERT with column list omitting some columns — omitted columns get NULL.
#[test]
fn test_insert_column_list_omits_columns() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (a INT, b TEXT, c FLOAT)");
    engine.execute("INSERT INTO t (a, c) VALUES (7, 3.14)").unwrap();
    let result_b = engine.execute("SELECT b FROM t WHERE a = 7").unwrap();
    match result_b.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        other => panic!("Omitted column b should be NULL, got {:?}", other),
    }
    let result_c = engine.execute("SELECT c FROM t WHERE a = 7").unwrap();
    let c_val = get_float(result_c.rows[0].get_by_idx(0).unwrap_or(&Value::Null));
    let _ = c_val; // just verify no panic
}

/// INSERT ... SELECT from same table (self-insert / copy within table).
#[test]
fn test_insert_select_from_same_table() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100)");
    exec(&engine, "INSERT INTO t VALUES (2, 200)");

    // Insert new rows with id+10 from existing rows
    engine.execute("INSERT INTO t (id, val) SELECT id + 10, val FROM t").unwrap();

    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 4, "After self-insert, table should have 4 rows");
    assert_eq!(count_rows(&engine, "SELECT * FROM t WHERE id = 11"), 1, "Row id=11 exists");
    assert_eq!(count_rows(&engine, "SELECT * FROM t WHERE id = 12"), 1, "Row id=12 exists");
}

/// UPDATE with expression: SET price = price * 1.1 (10% increase).
#[test]
fn test_update_expression_multiply() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, price FLOAT)");
    exec(&engine, "INSERT INTO t VALUES (1, 100.0)");
    exec(&engine, "INSERT INTO t VALUES (2, 50.0)");

    engine.execute("UPDATE t SET price = price * 1.1").unwrap();

    let result = engine.execute("SELECT price FROM t WHERE id = 1").unwrap();
    let p1 = get_float(result.rows[0].get_by_idx(0).unwrap());
    assert!((p1 - 110.0).abs() < 0.01, "price of row 1 should be 110.0, got {}", p1);

    let result2 = engine.execute("SELECT price FROM t WHERE id = 2").unwrap();
    let p2 = get_float(result2.rows[0].get_by_idx(0).unwrap());
    assert!((p2 - 55.0).abs() < 0.01, "price of row 2 should be 55.0, got {}", p2);
}

/// UPDATE with subquery in WHERE: WHERE id IN (SELECT ...).
#[test]
fn test_update_with_subquery_where() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE products (id INT, name TEXT, active INT)");
    exec(&engine, "CREATE TABLE to_deactivate (id INT)");
    exec(&engine, "INSERT INTO products VALUES (1, 'Apple', 1)");
    exec(&engine, "INSERT INTO products VALUES (2, 'Banana', 1)");
    exec(&engine, "INSERT INTO products VALUES (3, 'Cherry', 1)");
    exec(&engine, "INSERT INTO to_deactivate VALUES (2)");
    exec(&engine, "INSERT INTO to_deactivate VALUES (3)");

    engine.execute(
        "UPDATE products SET active = 0 WHERE id IN (SELECT id FROM to_deactivate)"
    ).unwrap();

    let active_count = count_rows(&engine, "SELECT * FROM products WHERE active = 1");
    assert_eq!(active_count, 1, "Only product 1 should remain active");
}

/// UPDATE multiple rows, verify affected rows via RETURNING.
#[test]
fn test_update_multiple_rows_returning() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, status TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'pending')");
    exec(&engine, "INSERT INTO t VALUES (2, 'pending')");
    exec(&engine, "INSERT INTO t VALUES (3, 'done')");

    let result = engine.execute(
        "UPDATE t SET status = 'active' WHERE status = 'pending' RETURNING id"
    ).unwrap();
    assert_eq!(result.rows.len(), 2, "UPDATE should return 2 rows (the updated ones)");
}

/// DELETE with subquery: WHERE id IN (SELECT ...).
#[test]
fn test_delete_with_subquery() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE items (id INT, name TEXT)");
    exec(&engine, "CREATE TABLE expired (id INT)");
    exec(&engine, "INSERT INTO items VALUES (1, 'Alpha')");
    exec(&engine, "INSERT INTO items VALUES (2, 'Beta')");
    exec(&engine, "INSERT INTO items VALUES (3, 'Gamma')");
    exec(&engine, "INSERT INTO expired VALUES (1)");
    exec(&engine, "INSERT INTO expired VALUES (3)");

    engine.execute("DELETE FROM items WHERE id IN (SELECT id FROM expired)").unwrap();

    let n = count_rows(&engine, "SELECT * FROM items");
    assert_eq!(n, 1, "Only item 2 (Beta) should remain after deleting expired items");
    assert_eq!(count_rows(&engine, "SELECT * FROM items WHERE name = 'Beta'"), 1);
}

/// DELETE + ROLLBACK verifies rows are not gone.
#[test]
fn test_delete_then_rollback() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, val TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'keep')");
    exec(&engine, "INSERT INTO t VALUES (2, 'keep')");

    let xid = engine.txn_manager.begin(txn::transaction::IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "DELETE FROM t").unwrap();
    engine.txn_manager.abort(xid).unwrap();

    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 2, "After DELETE + ROLLBACK, both rows must survive");
}

/// RETURNING with computed expression: RETURNING id, price * 2 AS doubled.
#[test]
fn test_insert_returning_computed_expression() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, price FLOAT)");

    let result = engine.execute(
        "INSERT INTO t VALUES (1, 50.0) RETURNING id, price * 2"
    ).unwrap();
    assert_eq!(result.rows.len(), 1);
    let doubled = get_float(result.rows[0].get_by_idx(1).unwrap());
    assert!((doubled - 100.0).abs() < 0.01, "RETURNING price * 2 should be 100.0, got {}", doubled);
}

/// UPDATE RETURNING with computed expression.
#[test]
fn test_update_returning_computed_expression() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, price FLOAT)");
    exec(&engine, "INSERT INTO t VALUES (1, 10.0)");

    let result = engine.execute(
        "UPDATE t SET price = price + 5.0 WHERE id = 1 RETURNING id, price * 2"
    ).unwrap();
    assert_eq!(result.rows.len(), 1);
    let doubled = get_float(result.rows[0].get_by_idx(1).unwrap());
    // price after update = 15.0; doubled = 30.0
    assert!((doubled - 30.0).abs() < 0.01, "RETURNING price*2 after update should be 30.0, got {}", doubled);
}

/// DELETE RETURNING with computed expression.
#[test]
fn test_delete_returning_computed_expression() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, price FLOAT)");
    exec(&engine, "INSERT INTO t VALUES (1, 25.0)");

    let result = engine.execute(
        "DELETE FROM t WHERE id = 1 RETURNING id, price * 4"
    ).unwrap();
    assert_eq!(result.rows.len(), 1);
    let quadrupled = get_float(result.rows[0].get_by_idx(1).unwrap());
    assert!((quadrupled - 100.0).abs() < 0.01, "RETURNING price*4 should be 100.0, got {}", quadrupled);
}

/// INSERT ... ON CONFLICT DO NOTHING (if supported; ignored if not).
#[test]
fn test_insert_on_conflict_do_nothing() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT PRIMARY KEY, val TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'original')");
    engine.execute("INSERT INTO t VALUES (1, 'duplicate') ON CONFLICT DO NOTHING").unwrap();
    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 1, "ON CONFLICT DO NOTHING should not insert duplicate");
}

/// INSERT ... ON CONFLICT DO UPDATE (upsert): ignored if not supported.
#[test]
fn test_insert_on_conflict_do_update() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT PRIMARY KEY, val TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'old')");
    engine.execute("INSERT INTO t VALUES (1, 'new') ON CONFLICT (id) DO UPDATE SET val = EXCLUDED.val").unwrap();
    let result = engine.execute("SELECT val FROM t WHERE id = 1").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "new"),
        other => panic!("expected 'new', got {:?}", other),
    }
}

/// UPDATE SET arithmetic expression combining two columns.
#[test]
fn test_update_expression_two_columns() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, a INT, b INT, total INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 10, 20, 0)");
    exec(&engine, "INSERT INTO t VALUES (2, 5, 15, 0)");

    engine.execute("UPDATE t SET total = a + b").unwrap();

    let result = engine.execute("SELECT total FROM t WHERE id = 1").unwrap();
    assert_eq!(get_int(result.rows[0].get_by_idx(0).unwrap()), 30, "total = a+b = 10+20");
    let result2 = engine.execute("SELECT total FROM t WHERE id = 2").unwrap();
    assert_eq!(get_int(result2.rows[0].get_by_idx(0).unwrap()), 20, "total = a+b = 5+15");
}

/// DELETE by text pattern.
#[test]
fn test_delete_by_text_condition() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, status TEXT)");
    exec(&engine, "INSERT INTO t VALUES (1, 'active')");
    exec(&engine, "INSERT INTO t VALUES (2, 'archived')");
    exec(&engine, "INSERT INTO t VALUES (3, 'archived')");
    exec(&engine, "INSERT INTO t VALUES (4, 'active')");

    engine.execute("DELETE FROM t WHERE status = 'archived'").unwrap();

    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 2, "2 active rows remain after deleting archived");
    let archived = count_rows(&engine, "SELECT * FROM t WHERE status = 'archived'");
    assert_eq!(archived, 0, "No archived rows should remain");
}

/// UPDATE then verify other rows are not affected.
#[test]
fn test_update_targeted_row_only() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT, score INT)");
    exec(&engine, "INSERT INTO t VALUES (1, 10)");
    exec(&engine, "INSERT INTO t VALUES (2, 20)");
    exec(&engine, "INSERT INTO t VALUES (3, 30)");

    engine.execute("UPDATE t SET score = 99 WHERE id = 2").unwrap();

    assert_eq!(query_int(&engine, "SELECT score FROM t WHERE id = 1"), 10, "Row 1 unchanged");
    assert_eq!(query_int(&engine, "SELECT score FROM t WHERE id = 2"), 99, "Row 2 updated");
    assert_eq!(query_int(&engine, "SELECT score FROM t WHERE id = 3"), 30, "Row 3 unchanged");
}

/// INSERT ... SELECT with WHERE that matches nothing: 0 rows inserted.
#[test]
fn test_insert_select_no_matching_rows() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE src (val INT)");
    exec(&engine, "INSERT INTO src VALUES (1), (2)");
    exec(&engine, "CREATE TABLE dst (val INT)");

    engine.execute("INSERT INTO dst SELECT val FROM src WHERE val > 1000").unwrap();

    let n = count_rows(&engine, "SELECT * FROM dst");
    assert_eq!(n, 0, "INSERT ... SELECT with no matching rows inserts 0 rows");
}

/// Multi-row insert with large VALUES list.
#[test]
fn test_insert_large_multi_row() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t (id INT)");
    engine.execute("INSERT INTO t VALUES (1),(2),(3),(4),(5),(6),(7),(8),(9),(10)").unwrap();
    let n = count_rows(&engine, "SELECT * FROM t");
    assert_eq!(n, 10, "10-row VALUES insert should produce 10 rows");
}
