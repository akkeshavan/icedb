use catalog::manager::CatalogManager;
use std::sync::Arc;
use tempfile::tempdir;
use txn::manager::TransactionManager;
use wal::writer::WalWriter;

use crate::engine::QueryEngine;
use crate::error::SqlError;

fn make_engine(dir: &std::path::Path) -> QueryEngine {
    let wal = Arc::new(WalWriter::open(dir).unwrap());
    let txn_mgr = Arc::new(TransactionManager::new(Arc::clone(&wal)));
    let catalog =
        Arc::new(CatalogManager::open(dir, Arc::clone(&wal), Arc::clone(&txn_mgr)).unwrap());
    QueryEngine::new(txn_mgr, catalog, dir.to_path_buf())
}

fn reopen_engine(dir: &std::path::Path) -> (QueryEngine, Arc<CatalogManager>) {
    // Use WAL recovery so that committed XIDs from previous runs are visible.
    let wal = Arc::new(WalWriter::open(dir).unwrap());
    let txn_mgr = Arc::new(TransactionManager::new_with_wal_recovery(Arc::clone(&wal), dir));
    let catalog =
        Arc::new(CatalogManager::open(dir, Arc::clone(&wal), Arc::clone(&txn_mgr)).unwrap());
    let engine = QueryEngine::new(txn_mgr, Arc::clone(&catalog), dir.to_path_buf());
    (engine, catalog)
}

fn make_engine_with_catalog(dir: &std::path::Path) -> (QueryEngine, Arc<CatalogManager>) {
    let wal = Arc::new(WalWriter::open(dir).unwrap());
    let txn_mgr = Arc::new(TransactionManager::new(Arc::clone(&wal)));
    let catalog =
        Arc::new(CatalogManager::open(dir, Arc::clone(&wal), Arc::clone(&txn_mgr)).unwrap());
    let engine = QueryEngine::new(txn_mgr, Arc::clone(&catalog), dir.to_path_buf());
    (engine, catalog)
}

#[test]
fn test_parse_select() {
    use crate::parser::Parser;
    let result = Parser::parse("SELECT 1");
    assert!(result.is_ok(), "Expected parse to succeed: {:?}", result);
    let stmts = result.unwrap();
    assert_eq!(stmts.len(), 1);
}

#[test]
fn test_create_table_and_select_empty() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());

    engine
        .execute("CREATE TABLE t (id INT, name TEXT)")
        .unwrap();

    let result = engine.execute("SELECT * FROM t").unwrap();
    assert_eq!(result.rows.len(), 0, "Expected 0 rows from empty table");
}

#[test]
fn test_insert_and_select() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());

    engine
        .execute("CREATE TABLE t (id INT, name TEXT)")
        .unwrap();
    engine.execute("INSERT INTO t VALUES (1, 'Alice')").unwrap();
    engine.execute("INSERT INTO t VALUES (2, 'Bob')").unwrap();
    engine
        .execute("INSERT INTO t VALUES (3, 'Charlie')")
        .unwrap();

    let result = engine.execute("SELECT * FROM t").unwrap();
    assert_eq!(result.rows.len(), 3, "Expected 3 rows");

    // Verify values are correct
    let ids: Vec<&crate::Value> = result.rows.iter().map(|r| r.get("id").unwrap()).collect();
    // Check all 3 IDs exist
    assert!(ids.iter().any(|v| **v == crate::Value::Int4(1)));
    assert!(ids.iter().any(|v| **v == crate::Value::Int4(2)));
    assert!(ids.iter().any(|v| **v == crate::Value::Int4(3)));
}

#[test]
fn test_select_with_where() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());

    engine
        .execute("CREATE TABLE t (id INT, name TEXT)")
        .unwrap();
    for i in 1..=5 {
        engine
            .execute(&format!("INSERT INTO t VALUES ({i}, 'name{i}')"))
            .unwrap();
    }

    let result = engine.execute("SELECT * FROM t WHERE id = 3").unwrap();
    assert_eq!(result.rows.len(), 1, "Expected 1 row matching id=3");
    assert_eq!(result.rows[0].get("id"), Some(&crate::Value::Int4(3)));
}

#[test]
fn test_update() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());

    engine
        .execute("CREATE TABLE t (id INT, name TEXT)")
        .unwrap();
    engine.execute("INSERT INTO t VALUES (1, 'Alice')").unwrap();
    engine.execute("INSERT INTO t VALUES (2, 'Bob')").unwrap();

    engine
        .execute("UPDATE t SET name = 'foo' WHERE id = 1")
        .unwrap();

    let result = engine.execute("SELECT * FROM t WHERE id = 1").unwrap();
    assert_eq!(result.rows.len(), 1);
    assert_eq!(
        result.rows[0].get("name"),
        Some(&crate::Value::Text("foo".to_string()))
    );

    // Bob should be unchanged
    let result2 = engine.execute("SELECT * FROM t WHERE id = 2").unwrap();
    assert_eq!(result2.rows.len(), 1);
    assert_eq!(
        result2.rows[0].get("name"),
        Some(&crate::Value::Text("Bob".to_string()))
    );
}

#[test]
fn test_delete() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());

    engine
        .execute("CREATE TABLE t (id INT, name TEXT)")
        .unwrap();
    for i in 1..=5 {
        engine
            .execute(&format!("INSERT INTO t VALUES ({i}, 'name{i}')"))
            .unwrap();
    }

    engine.execute("DELETE FROM t WHERE id > 3").unwrap();

    let result = engine.execute("SELECT * FROM t").unwrap();
    assert_eq!(
        result.rows.len(),
        3,
        "Expected 3 rows after deleting id > 3"
    );

    // Ensure rows 1, 2, 3 remain
    for row in &result.rows {
        let id = row.get("id").unwrap();
        assert!(
            matches!(
                id,
                crate::Value::Int4(1) | crate::Value::Int4(2) | crate::Value::Int4(3)
            ),
            "Unexpected row with id={id:?}"
        );
    }
}

#[test]
fn test_order_by() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());

    engine
        .execute("CREATE TABLE t (id INT, name TEXT)")
        .unwrap();
    engine
        .execute("INSERT INTO t VALUES (1, 'banana')")
        .unwrap();
    engine.execute("INSERT INTO t VALUES (2, 'apple')").unwrap();
    engine
        .execute("INSERT INTO t VALUES (3, 'cherry')")
        .unwrap();

    let result = engine.execute("SELECT * FROM t ORDER BY name").unwrap();
    assert_eq!(result.rows.len(), 3);

    // Get names in order
    let names: Vec<&crate::Value> = result
        .rows
        .iter()
        .map(|r| r.get_by_idx(1).unwrap())
        .collect();
    assert_eq!(names[0], &crate::Value::Text("apple".to_string()));
    assert_eq!(names[1], &crate::Value::Text("banana".to_string()));
    assert_eq!(names[2], &crate::Value::Text("cherry".to_string()));
}

#[test]
fn test_limit_offset() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());

    engine
        .execute("CREATE TABLE t (id INT, name TEXT)")
        .unwrap();
    for i in 1..=10 {
        engine
            .execute(&format!("INSERT INTO t VALUES ({i}, 'name{i}')"))
            .unwrap();
    }

    let result = engine
        .execute("SELECT * FROM t ORDER BY id LIMIT 3 OFFSET 2")
        .unwrap();
    assert_eq!(result.rows.len(), 3, "Expected 3 rows");

    // Should be rows 3, 4, 5 (offset 2 from ordered list starting at 1)
    let ids: Vec<i32> = result
        .rows
        .iter()
        .map(|r| match r.get("id").unwrap() {
            crate::Value::Int4(i) => *i,
            _ => panic!("expected Int4"),
        })
        .collect();
    assert_eq!(ids, vec![3, 4, 5]);
}

#[test]
fn test_count_aggregate() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());

    engine
        .execute("CREATE TABLE t (id INT, name TEXT)")
        .unwrap();
    for i in 1..=5 {
        engine
            .execute(&format!("INSERT INTO t VALUES ({i}, 'name{i}')"))
            .unwrap();
    }

    let result = engine.execute("SELECT COUNT(*) FROM t").unwrap();
    assert_eq!(result.rows.len(), 1, "Expected 1 row from aggregate");

    let count_val = result.rows[0].get_by_idx(0).unwrap();
    assert_eq!(*count_val, crate::Value::Int8(5), "Expected count = 5");
}

#[test]
fn test_inner_join() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());

    engine
        .execute("CREATE TABLE users (id INT, name TEXT)")
        .unwrap();
    engine
        .execute("CREATE TABLE orders (user_id INT, product TEXT)")
        .unwrap();

    engine
        .execute("INSERT INTO users VALUES (1, 'Alice')")
        .unwrap();
    engine
        .execute("INSERT INTO users VALUES (2, 'Bob')")
        .unwrap();
    engine
        .execute("INSERT INTO users VALUES (3, 'Charlie')")
        .unwrap();

    engine
        .execute("INSERT INTO orders VALUES (1, 'Widget')")
        .unwrap();
    engine
        .execute("INSERT INTO orders VALUES (1, 'Gadget')")
        .unwrap();
    engine
        .execute("INSERT INTO orders VALUES (2, 'Thingy')")
        .unwrap();

    let result = engine
        .execute("SELECT u.name, o.product FROM users u JOIN orders o ON u.id = o.user_id")
        .unwrap();

    assert_eq!(result.rows.len(), 3, "Expected 3 joined rows");

    // Collect name/product pairs
    let mut pairs: Vec<(String, String)> = result
        .rows
        .iter()
        .map(|r| {
            let name = match r.get_by_idx(0).unwrap() {
                crate::Value::Text(s) => s.clone(),
                _ => panic!("expected text for name"),
            };
            let product = match r.get_by_idx(1).unwrap() {
                crate::Value::Text(s) => s.clone(),
                _ => panic!("expected text for product"),
            };
            (name, product)
        })
        .collect();
    pairs.sort();

    assert_eq!(
        pairs,
        vec![
            ("Alice".to_string(), "Gadget".to_string()),
            ("Alice".to_string(), "Widget".to_string()),
            ("Bob".to_string(), "Thingy".to_string()),
        ]
    );
}

#[test]
fn test_transaction_isolation() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());

    engine
        .execute("CREATE TABLE t (id INT, name TEXT)")
        .unwrap();

    // Begin T1 and insert a row but don't commit
    let xid1 = engine
        .txn_manager
        .begin(txn::transaction::IsolationLevel::ReadCommitted);
    engine
        .execute_in_txn(xid1, "INSERT INTO t VALUES (1, 'Alice')")
        .unwrap();

    // T2 should NOT see T1's uncommitted insert
    let result = engine.execute("SELECT * FROM t").unwrap();
    assert_eq!(
        result.rows.len(),
        0,
        "T2 should not see T1's uncommitted data"
    );

    // Commit T1
    engine.txn_manager.commit(xid1).unwrap();

    // Now T3 should see the committed row
    let result2 = engine.execute("SELECT * FROM t").unwrap();
    assert_eq!(
        result2.rows.len(),
        1,
        "After T1 commits, T3 should see the row"
    );
}

#[test]
fn test_drop_table() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());

    engine
        .execute("CREATE TABLE t (id INT, name TEXT)")
        .unwrap();
    engine.execute("INSERT INTO t VALUES (1, 'Alice')").unwrap();

    // Verify it exists
    let result = engine.execute("SELECT * FROM t").unwrap();
    assert_eq!(result.rows.len(), 1);

    // Drop the table
    engine.execute("DROP TABLE t").unwrap();

    // Now SELECT should fail with TableNotFound
    let err = engine.execute("SELECT * FROM t").unwrap_err();
    assert!(
        matches!(err, SqlError::TableNotFound(_)),
        "Expected TableNotFound error, got: {:?}",
        err
    );
}

#[test]
fn test_index_scan() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());

    engine
        .execute("CREATE TABLE products (id INT, name TEXT, price INT)")
        .unwrap();

    for i in 1i32..=50 {
        engine
            .execute(&format!(
                "INSERT INTO products VALUES ({}, 'item{}', {})",
                i,
                i,
                i * 10
            ))
            .unwrap();
    }

    // Create index on id column
    engine.execute("CREATE INDEX ON products (id)").unwrap();

    // Index scan should return the correct row
    let result = engine
        .execute("SELECT * FROM products WHERE id = 25")
        .unwrap();
    assert_eq!(result.rows.len(), 1, "Expected exactly 1 row for id=25");
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Int4(v)) => assert_eq!(*v, 25),
        other => panic!("Expected Int4(25), got {:?}", other),
    }
}

// ── Value encoding/decoding unit tests ─────────────────────────────────────────

#[test]
fn test_value_date_roundtrip() {
    use crate::value::{parse_date_str, format_date};
    let d = parse_date_str("2024-06-15").expect("parse failed");
    assert_eq!(format_date(d), "2024-06-15");
}

#[test]
fn test_value_date_epoch() {
    use crate::value::parse_date_str;
    // 1970-01-01 should be day 0
    let d = parse_date_str("1970-01-01").expect("parse 1970-01-01");
    assert_eq!(d, 0);
}

#[test]
fn test_value_date_before_epoch() {
    use crate::value::{parse_date_str, format_date};
    let d = parse_date_str("1969-12-31").expect("parse 1969-12-31");
    assert_eq!(d, -1);
    assert_eq!(format_date(d), "1969-12-31");
}

#[test]
fn test_value_timestamp_roundtrip() {
    use crate::value::{parse_timestamp_str, format_timestamp};
    let ts = parse_timestamp_str("2024-01-15 12:30:00").expect("parse ts");
    let formatted = format_timestamp(ts);
    assert!(formatted.starts_with("2024-01-15"), "got: {}", formatted);
    assert!(formatted.contains("12:30:00"), "got: {}", formatted);
}

#[test]
fn test_value_to_bytes_from_bytes_date() {
    use crate::value::Value;
    let v = Value::Date(19523); // 2023-06-17 ish
    let bytes = v.to_bytes();
    assert_eq!(bytes.len(), 4);
    let v2 = Value::from_bytes(&bytes, &catalog::DataType::Date).unwrap();
    assert_eq!(v, v2);
}

#[test]
fn test_value_to_bytes_from_bytes_timestamp() {
    use crate::value::Value;
    let ts = 1_705_315_800_000_000i64; // some timestamp in microseconds
    let v = Value::Timestamp(ts);
    let bytes = v.to_bytes();
    assert_eq!(bytes.len(), 8);
    let v2 = Value::from_bytes(&bytes, &catalog::DataType::Timestamp).unwrap();
    assert_eq!(v, v2);
}

#[test]
fn test_value_to_bytes_from_bytes_numeric() {
    use crate::value::Value;
    let v = Value::Numeric("123.456".to_string());
    let bytes = v.to_bytes();
    assert!(bytes.len() >= 4, "Numeric bytes should have length prefix");
    let v2 = Value::from_bytes(&bytes, &catalog::DataType::Numeric).unwrap();
    assert_eq!(v, v2);
}

#[test]
fn test_value_to_bytes_from_bytes_uuid() {
    use crate::value::Value;
    let v = Value::Uuid("550e8400-e29b-41d4-a716-446655440000".to_string());
    let bytes = v.to_bytes();
    let v2 = Value::from_bytes(&bytes, &catalog::DataType::Uuid).unwrap();
    assert_eq!(v, v2);
}

#[test]
fn test_value_cast_text_to_date() {
    use crate::value::Value;
    let v = Value::Text("2024-03-15".to_string());
    let date = v.cast_to(&catalog::DataType::Date).unwrap();
    assert!(matches!(date, Value::Date(_)));
}

#[test]
fn test_value_cast_date_to_text() {
    use crate::value::Value;
    let v = Value::Date(0); // 1970-01-01
    let text = v.cast_to(&catalog::DataType::Text).unwrap();
    match text {
        Value::Text(s) => assert_eq!(s, "1970-01-01"),
        other => panic!("expected text, got {:?}", other),
    }
}

#[test]
fn test_value_cast_int_to_numeric() {
    use crate::value::Value;
    let v = Value::Int4(42);
    let n = v.cast_to(&catalog::DataType::Numeric).unwrap();
    match n {
        Value::Numeric(s) => assert_eq!(s, "42"),
        other => panic!("expected Numeric, got {:?}", other),
    }
}

#[test]
#[allow(clippy::approx_constant)]
fn test_value_cast_numeric_to_float() {
    use crate::value::Value;
    let v = Value::Numeric("3.14".to_string());
    let f = v.cast_to(&catalog::DataType::Float8).unwrap();
    match f {
        Value::Float8(x) => assert!((x - 3.14_f64).abs() < 1e-10),
        other => panic!("expected Float8, got {:?}", other),
    }
}

#[test]
fn test_value_display_date() {
    use crate::value::Value;
    let v = Value::Date(0);
    assert_eq!(v.to_string(), "1970-01-01");
}

#[test]
fn test_value_display_numeric() {
    use crate::value::Value;
    let v = Value::Numeric("99.99".to_string());
    assert_eq!(v.to_string(), "99.99");
}

#[test]
fn test_value_display_uuid() {
    use crate::value::Value;
    let v = Value::Uuid("550e8400-e29b-41d4-a716-446655440000".to_string());
    assert_eq!(v.to_string(), "550e8400-e29b-41d4-a716-446655440000");
}

#[test]
fn test_value_partial_ord_date() {
    use crate::value::Value;
    let d1 = Value::Date(100);
    let d2 = Value::Date(200);
    assert!(d1 < d2);
    assert!(d2 > d1);
    assert_eq!(d1, Value::Date(100));
}

#[test]
fn test_value_partial_ord_timestamp() {
    use crate::value::Value;
    let t1 = Value::Timestamp(1000000);
    let t2 = Value::Timestamp(2000000);
    assert!(t1 < t2);
    assert!(t2 > t1);
}

// ── Parse-level unit tests ──────────────────────────────────────────────────────

#[test]
fn test_parse_create_table_with_serial() {
    use crate::parser::Parser;
    let result = Parser::parse("CREATE TABLE t (id SERIAL, name TEXT)");
    assert!(result.is_ok(), "SERIAL should parse: {:?}", result);
}

#[test]
fn test_parse_create_table_with_bigserial() {
    use crate::parser::Parser;
    let result = Parser::parse("CREATE TABLE t (id BIGSERIAL, name TEXT)");
    assert!(result.is_ok(), "BIGSERIAL should parse: {:?}", result);
}

#[test]
fn test_parse_create_table_with_date() {
    use crate::parser::Parser;
    let result = Parser::parse("CREATE TABLE t (id INT, d DATE)");
    assert!(result.is_ok(), "DATE type should parse: {:?}", result);
}

#[test]
fn test_parse_create_table_with_timestamp() {
    use crate::parser::Parser;
    let result = Parser::parse("CREATE TABLE t (id INT, ts TIMESTAMP)");
    assert!(result.is_ok(), "TIMESTAMP type should parse: {:?}", result);
}

#[test]
fn test_parse_create_table_with_uuid() {
    use crate::parser::Parser;
    let result = Parser::parse("CREATE TABLE t (id INT, uid UUID)");
    assert!(result.is_ok(), "UUID type should parse: {:?}", result);
}

#[test]
fn test_parse_create_table_with_numeric() {
    use crate::parser::Parser;
    let result = Parser::parse("CREATE TABLE t (id INT, price NUMERIC)");
    assert!(result.is_ok(), "NUMERIC type should parse: {:?}", result);
}

#[test]
fn test_parse_string_agg() {
    use crate::parser::Parser;
    let result = Parser::parse("SELECT string_agg(name, ', ') FROM t");
    assert!(result.is_ok(), "string_agg should parse: {:?}", result);
}

#[test]
fn test_parse_stddev() {
    use crate::parser::Parser;
    let result = Parser::parse("SELECT stddev(val) FROM t");
    assert!(result.is_ok(), "stddev should parse: {:?}", result);
}

#[test]
fn test_parse_now_function() {
    use crate::parser::Parser;
    let result = Parser::parse("SELECT NOW()");
    assert!(result.is_ok(), "NOW() should parse: {:?}", result);
}

#[test]
fn test_parse_gen_random_uuid() {
    use crate::parser::Parser;
    let result = Parser::parse("SELECT gen_random_uuid()");
    assert!(result.is_ok(), "gen_random_uuid() should parse: {:?}", result);
}

#[test]
fn test_parse_upper_lower() {
    use crate::parser::Parser;
    let result = Parser::parse("SELECT upper(name), lower(name) FROM t");
    assert!(result.is_ok(), "upper/lower should parse: {:?}", result);
}

#[test]
fn test_parse_default_clause() {
    use crate::parser::Parser;
    let result = Parser::parse("CREATE TABLE t (id INT, status TEXT DEFAULT 'active')");
    assert!(result.is_ok(), "DEFAULT clause should parse: {:?}", result);
}

// ── Engine unit tests: new features ────────────────────────────────────────────

#[test]
fn test_engine_select_current_date() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT CURRENT_DATE").unwrap();
    assert_eq!(result.rows.len(), 1);
    assert!(matches!(result.rows[0].get_by_idx(0), Some(crate::Value::Date(_))));
}

#[test]
fn test_engine_select_now() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT NOW()").unwrap();
    assert_eq!(result.rows.len(), 1);
    assert!(matches!(result.rows[0].get_by_idx(0), Some(crate::Value::Timestamp(_))));
}

#[test]
fn test_engine_upper_function() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT upper('hello')").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Text(s)) => assert_eq!(s, "HELLO"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_engine_lower_function() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT lower('WORLD')").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Text(s)) => assert_eq!(s, "world"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_engine_abs_function() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT abs(-10)").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Int4(10)) => {}
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_engine_sqrt_function() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT sqrt(25.0)").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Float8(v)) => assert!((v - 5.0).abs() < 1e-9),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_engine_count_aggregate() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (x INT)").unwrap();
    for i in 1..=5 {
        engine.execute(&format!("INSERT INTO t VALUES ({})", i)).unwrap();
    }
    let result = engine.execute("SELECT count(*) FROM t").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Int8(5)) => {}
        other => panic!("expected 5, got {:?}", other),
    }
}

#[test]
fn test_engine_sum_aggregate() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (x INT)").unwrap();
    for i in 1..=4 {
        engine.execute(&format!("INSERT INTO t VALUES ({})", i)).unwrap();
    }
    let result = engine.execute("SELECT sum(x) FROM t").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Int8(10)) | Some(crate::Value::Int4(10)) => {}
        other => panic!("expected 10, got {:?}", other),
    }
}

#[test]
fn test_engine_concat_string_function() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT concat('foo', 'bar')").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Text(s)) => assert_eq!(s, "foobar"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_engine_replace_function() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT replace('aababab', 'ab', 'X')").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Text(s)) => assert_eq!(s, "aXXX"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_engine_serial_basic() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id SERIAL, val TEXT)").unwrap();
    engine.execute("INSERT INTO t (val) VALUES ('a')").unwrap();
    engine.execute("INSERT INTO t (val) VALUES ('b')").unwrap();
    let result = engine.execute("SELECT id FROM t ORDER BY id").unwrap();
    assert_eq!(result.rows.len(), 2);
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Int4(1)) | Some(crate::Value::Int8(1)) => {}
        other => panic!("expected id=1, got {:?}", other),
    }
    match result.rows[1].get_by_idx(0) {
        Some(crate::Value::Int4(2)) | Some(crate::Value::Int8(2)) => {}
        other => panic!("expected id=2, got {:?}", other),
    }
}

#[test]
fn test_engine_default_value() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, status TEXT DEFAULT 'pending')").unwrap();
    engine.execute("INSERT INTO t (id) VALUES (1)").unwrap();
    let result = engine.execute("SELECT status FROM t WHERE id = 1").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Text(s)) => assert_eq!(s, "pending"),
        other => panic!("expected 'pending', got {:?}", other),
    }
}

#[test]
fn test_engine_date_filter() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, d DATE)").unwrap();
    engine.execute("INSERT INTO t VALUES (1, '2024-01-01')").unwrap();
    engine.execute("INSERT INTO t VALUES (2, '2024-06-15')").unwrap();
    let result = engine.execute("SELECT id FROM t WHERE d > '2024-03-01'").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Int4(2)) => {}
        other => panic!("expected id=2, got {:?}", other),
    }
}

#[test]
fn test_engine_select_all_columns() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, name TEXT, score FLOAT)").unwrap();
    engine.execute("INSERT INTO t VALUES (1, 'Alice', 95.5)").unwrap();
    let result = engine.execute("SELECT * FROM t").unwrap();
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].values.len(), 3);
}

#[test]
fn test_engine_drop_table() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT)").unwrap();
    engine.execute("DROP TABLE t").unwrap();
    let result = engine.execute("SELECT * FROM t");
    assert!(result.is_err(), "Table should not exist after DROP");
}

#[test]
fn test_engine_alter_table_add_column() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT)").unwrap();
    engine.execute("INSERT INTO t VALUES (1)").unwrap();
    engine.execute("ALTER TABLE t ADD COLUMN name TEXT").unwrap();
    let result = engine.execute("SELECT id, name FROM t").unwrap();
    assert_eq!(result.rows.len(), 1);
    // Old row should have NULL for new column
    match result.rows[0].get_by_idx(1) {
        Some(crate::Value::Null) | None => {}
        Some(crate::Value::Text(_)) => {} // NULL may be displayed as empty text
        other => panic!("expected Null for new column, got {:?}", other),
    }
}

#[test]
fn test_engine_update_multiple_rows() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (dept TEXT, salary INT)").unwrap();
    engine.execute("INSERT INTO t VALUES ('eng', 100)").unwrap();
    engine.execute("INSERT INTO t VALUES ('eng', 200)").unwrap();
    engine.execute("INSERT INTO t VALUES ('hr', 150)").unwrap();
    engine.execute("UPDATE t SET salary = salary * 2 WHERE dept = 'eng'").unwrap();
    let result = engine.execute("SELECT salary FROM t WHERE dept = 'eng' ORDER BY salary").unwrap();
    assert_eq!(result.rows.len(), 2);
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Int4(200)) => {}
        other => panic!("expected 200, got {:?}", other),
    }
    match result.rows[1].get_by_idx(0) {
        Some(crate::Value::Int4(400)) => {}
        other => panic!("expected 400, got {:?}", other),
    }
}

#[test]
fn test_engine_order_by_text() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (name TEXT)").unwrap();
    engine.execute("INSERT INTO t VALUES ('Charlie')").unwrap();
    engine.execute("INSERT INTO t VALUES ('Alice')").unwrap();
    engine.execute("INSERT INTO t VALUES ('Bob')").unwrap();
    let result = engine.execute("SELECT name FROM t ORDER BY name ASC").unwrap();
    let names: Vec<String> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(crate::Value::Text(s)) => s.clone(),
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(names, vec!["Alice", "Bob", "Charlie"]);
}

#[test]
fn test_engine_group_by_count() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (category TEXT, val INT)").unwrap();
    engine.execute("INSERT INTO t VALUES ('a', 1)").unwrap();
    engine.execute("INSERT INTO t VALUES ('a', 2)").unwrap();
    engine.execute("INSERT INTO t VALUES ('b', 3)").unwrap();
    let result = engine.execute("SELECT category, count(*) FROM t GROUP BY category ORDER BY category").unwrap();
    assert_eq!(result.rows.len(), 2);
    match result.rows[0].get_by_idx(1) {
        Some(crate::Value::Int8(2)) => {}
        other => panic!("expected count=2, got {:?}", other),
    }
}

#[test]
fn test_engine_having_with_sum() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (g TEXT, v INT)").unwrap();
    engine.execute("INSERT INTO t VALUES ('x', 10)").unwrap();
    engine.execute("INSERT INTO t VALUES ('x', 20)").unwrap();
    engine.execute("INSERT INTO t VALUES ('y', 5)").unwrap();
    let result = engine.execute("SELECT g, sum(v) FROM t GROUP BY g HAVING sum(v) > 15").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Text(s)) => assert_eq!(s, "x"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_engine_limit_offset() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT)").unwrap();
    for i in 1..=10 {
        engine.execute(&format!("INSERT INTO t VALUES ({})", i)).unwrap();
    }
    let result = engine.execute("SELECT id FROM t ORDER BY id LIMIT 3 OFFSET 5").unwrap();
    assert_eq!(result.rows.len(), 3);
    let ids: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(crate::Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(ids, vec![6, 7, 8]);
}

#[test]
fn test_engine_coalesce() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, val TEXT)").unwrap();
    engine.execute("INSERT INTO t VALUES (1, NULL)").unwrap();
    engine.execute("INSERT INTO t VALUES (2, 'hello')").unwrap();
    let result = engine.execute("SELECT id, coalesce(val, 'default') FROM t ORDER BY id").unwrap();
    assert_eq!(result.rows.len(), 2);
    match result.rows[0].get_by_idx(1) {
        Some(crate::Value::Text(s)) => assert_eq!(s, "default"),
        other => panic!("expected 'default' for NULL, got {:?}", other),
    }
    match result.rows[1].get_by_idx(1) {
        Some(crate::Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_engine_bool_aggregate() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (active BOOLEAN)").unwrap();
    engine.execute("INSERT INTO t VALUES (true)").unwrap();
    engine.execute("INSERT INTO t VALUES (false)").unwrap();
    let result = engine.execute("SELECT bool_or(active), bool_and(active) FROM t").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Bool(true)) => {}
        other => panic!("bool_or should be true, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(crate::Value::Bool(false)) => {}
        other => panic!("bool_and should be false, got {:?}", other),
    }
}

#[test]
fn test_engine_like_pattern() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (name TEXT)").unwrap();
    engine.execute("INSERT INTO t VALUES ('postgresql')").unwrap();
    engine.execute("INSERT INTO t VALUES ('mysql')").unwrap();
    engine.execute("INSERT INTO t VALUES ('postgres')").unwrap();
    let result = engine.execute("SELECT name FROM t WHERE name LIKE 'post%'").unwrap();
    assert_eq!(result.rows.len(), 2, "LIKE 'post%' should match 2 rows");
}

#[test]
fn test_engine_case_expression() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (score INT)").unwrap();
    engine.execute("INSERT INTO t VALUES (90)").unwrap();
    engine.execute("INSERT INTO t VALUES (70)").unwrap();
    engine.execute("INSERT INTO t VALUES (50)").unwrap();
    let result = engine.execute(
        "SELECT score, CASE WHEN score >= 80 THEN 'A' WHEN score >= 60 THEN 'B' ELSE 'C' END AS grade \
         FROM t ORDER BY score DESC"
    ).unwrap();
    assert_eq!(result.rows.len(), 3);
    match result.rows[0].get_by_idx(1) {
        Some(crate::Value::Text(s)) => assert_eq!(s, "A"),
        other => panic!("expected 'A', got {:?}", other),
    }
}

#[test]
fn test_engine_subquery_in_select() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, val INT)").unwrap();
    for i in 1..=5 {
        engine.execute(&format!("INSERT INTO t VALUES ({}, {})", i, i * 10)).unwrap();
    }
    let result = engine.execute("SELECT id FROM t WHERE val = (SELECT max(val) FROM t)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Int4(5)) => {}
        other => panic!("expected id=5, got {:?}", other),
    }
}

#[test]
fn test_engine_between_operator() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT)").unwrap();
    for i in 1..=10 {
        engine.execute(&format!("INSERT INTO t VALUES ({})", i)).unwrap();
    }
    let result = engine.execute("SELECT id FROM t WHERE id BETWEEN 3 AND 7 ORDER BY id").unwrap();
    assert_eq!(result.rows.len(), 5);
    let ids: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(crate::Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(ids, vec![3, 4, 5, 6, 7]);
}

#[test]
fn test_engine_not_in() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT)").unwrap();
    for i in 1..=5 {
        engine.execute(&format!("INSERT INTO t VALUES ({})", i)).unwrap();
    }
    let result = engine.execute("SELECT id FROM t WHERE id NOT IN (2, 4) ORDER BY id").unwrap();
    assert_eq!(result.rows.len(), 3);
    let ids: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(crate::Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(ids, vec![1, 3, 5]);
}

#[test]
fn test_engine_string_concat_operator() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (first TEXT, last TEXT)").unwrap();
    engine.execute("INSERT INTO t VALUES ('John', 'Doe')").unwrap();
    let result = engine.execute("SELECT first || ' ' || last AS full_name FROM t").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Text(s)) => assert_eq!(s, "John Doe"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_engine_nullif() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT nullif(5, 5), nullif(5, 6)").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(crate::Value::Null) => {}
        other => panic!("nullif(5,5) should be NULL, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(crate::Value::Int4(5)) => {}
        other => panic!("nullif(5,6) should be 5, got {:?}", other),
    }
}

#[test]
fn test_engine_left_join_returns_null() {
    let dir = tempdir().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE a (id INT, name TEXT)").unwrap();
    engine.execute("CREATE TABLE b (a_id INT, info TEXT)").unwrap();
    engine.execute("INSERT INTO a VALUES (1, 'Alice')").unwrap();
    engine.execute("INSERT INTO a VALUES (2, 'Bob')").unwrap();
    engine.execute("INSERT INTO b VALUES (1, 'info for alice')").unwrap();
    let result = engine.execute(
        "SELECT a.name, b.info FROM a LEFT JOIN b ON a.id = b.a_id ORDER BY a.id"
    ).unwrap();
    assert_eq!(result.rows.len(), 2);
    // Bob has no matching b row — info should be NULL
    match result.rows[1].get_by_idx(1) {
        Some(crate::Value::Null) | None => {}
        other => panic!("expected NULL for Bob's info, got {:?}", other),
    }
}

#[test]
fn test_drop_table_removed_from_list_tables() {
    let dir = tempdir().unwrap();
    let (engine, catalog) = make_engine_with_catalog(dir.path());

    engine.execute("CREATE TABLE items (id INT, name TEXT)").unwrap();

    // Verify the table appears in list_tables
    let tables_before = catalog.list_tables("public").unwrap();
    assert!(tables_before.contains(&"items".to_string()), "items should be in list before drop");

    // Drop the table
    engine.execute("DROP TABLE items").unwrap();

    // Verify the table no longer appears in list_tables
    let tables_after = catalog.list_tables("public").unwrap();
    assert!(
        !tables_after.contains(&"items".to_string()),
        "items should NOT be in list after drop, but got: {:?}",
        tables_after
    );
}

#[test]
fn test_drop_table_after_restart_removed_from_list_tables() {
    // Create table in session 1, drop it in session 2 (simulating engine restart)
    let dir = tempdir().unwrap();

    // Session 1: create table
    {
        let engine = make_engine(dir.path());
        engine.execute("CREATE TABLE items (id INT, name TEXT)").unwrap();
        engine.execute("INSERT INTO items VALUES (1, 'Laptop')").unwrap();
    }
    // Engine drops here, simulating a restart

    // Session 2: drop table and verify it's gone from list_tables
    let (engine2, catalog2) = reopen_engine(dir.path());

    let tables_before = catalog2.list_tables("public").unwrap();
    assert!(tables_before.contains(&"items".to_string()), "items should exist after restart");

    engine2.execute("DROP TABLE items").unwrap();

    let tables_after = catalog2.list_tables("public").unwrap();
    assert!(
        !tables_after.contains(&"items".to_string()),
        "items should NOT be in list after drop post-restart, but got: {:?}",
        tables_after
    );
}

#[test]
fn test_drop_table_via_session_removed_from_list_tables() {
    // Uses execute_session (same path as CLI REPL)
    let dir = tempdir().unwrap();
    let (engine, catalog) = make_engine_with_catalog(dir.path());

    engine.execute_session("repl", "CREATE TABLE items (id INT, name TEXT)").unwrap();

    let tables_before = catalog.list_tables("public").unwrap();
    assert!(tables_before.contains(&"items".to_string()), "items should be in list before drop");

    engine.execute_session("repl", "DROP TABLE items").unwrap();

    let tables_after = catalog.list_tables("public").unwrap();
    assert!(
        !tables_after.contains(&"items".to_string()),
        "items should NOT be in list after drop via session, but got: {:?}",
        tables_after
    );
}

#[test]
fn test_drop_table_with_fk_removed_from_list_tables() {
    // Simulates the bookstore chapter FK scenario
    let dir = tempdir().unwrap();
    let (engine, catalog) = make_engine_with_catalog(dir.path());

    engine.execute("CREATE TABLE categories (id SERIAL PRIMARY KEY, name TEXT NOT NULL)").unwrap();
    engine.execute("CREATE TABLE items (id SERIAL PRIMARY KEY, category_id INT NOT NULL REFERENCES categories(id), name TEXT NOT NULL)").unwrap();
    engine.execute("INSERT INTO categories (name) VALUES ('Electronics')").unwrap();
    engine.execute("INSERT INTO items (category_id, name) VALUES (1, 'Laptop')").unwrap();

    let tables_before = catalog.list_tables("public").unwrap();
    assert!(tables_before.contains(&"items".to_string()), "items should exist before drop");
    assert!(tables_before.contains(&"categories".to_string()), "categories should exist before drop");

    // Drop items table
    engine.execute("DROP TABLE items").unwrap();

    let tables_after = catalog.list_tables("public").unwrap();
    assert!(
        !tables_after.contains(&"items".to_string()),
        "items should NOT be in list after drop, but got: {:?}",
        tables_after
    );
    assert!(tables_after.contains(&"categories".to_string()), "categories should still exist");
}

#[test]
fn test_check_constraint_numeric_vs_int_literal() {
    // Regression: CHECK (amount >= 0) was falsely failing when amount was NUMERIC
    // because partial_cmp(Numeric, Int4) returned None.
    let dir = tempdir().unwrap();
    let (engine, _catalog) = make_engine_with_catalog(dir.path());

    engine
        .execute("CREATE TABLE prices (id SERIAL PRIMARY KEY, amount NUMERIC(10,2) CHECK (amount >= 0), label TEXT)")
        .unwrap();

    // Should succeed: 19.99 >= 0
    let r = engine.execute("INSERT INTO prices (amount, label) VALUES (19.99, 'Standard')");
    assert!(r.is_ok(), "positive amount should satisfy CHECK >= 0, got: {:?}", r);

    // Should succeed: 0 >= 0
    let r = engine.execute("INSERT INTO prices (amount, label) VALUES (0, 'Free')");
    assert!(r.is_ok(), "zero amount should satisfy CHECK >= 0, got: {:?}", r);

    // Should fail: -1 < 0
    let r = engine.execute("INSERT INTO prices (amount, label) VALUES (-1, 'Invalid')");
    assert!(r.is_err(), "negative amount should violate CHECK >= 0");
}

#[test]
fn test_drop_table_clears_check_constraints() {
    // Regression: after DROP TABLE, recreating the same table should not see stale
    // check constraints from the previous incarnation.
    let dir = tempdir().unwrap();
    let (engine, catalog) = make_engine_with_catalog(dir.path());

    engine
        .execute("CREATE TABLE prices (id SERIAL, amount NUMERIC(10,2) CHECK (amount >= 0))")
        .unwrap();
    engine.execute("DROP TABLE prices").unwrap();

    // check_registry should be cleared after drop
    let tables = catalog.list_tables("public").unwrap();
    assert!(!tables.contains(&"prices".to_string()), "prices should not be listed after drop");

    // Recreate and insert — should work fine without stale constraint interference
    engine
        .execute("CREATE TABLE prices (id SERIAL, amount NUMERIC(10,2) CHECK (amount >= 0))")
        .unwrap();
    let r = engine.execute("INSERT INTO prices (amount) VALUES (5.00)");
    assert!(r.is_ok(), "insert after recreate should succeed, got: {:?}", r);
}

// ── Numeric arithmetic coercion ──────────────────────────────────────────────

#[test]
fn test_numeric_arithmetic_with_int_literal() {
    // Regression: `balance - 100` where balance is NUMERIC(12,2) and 100 is Int4
    // used to fail with "arithmetic type mismatch: Numeric vs Int4".
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());

    engine.execute("CREATE TABLE acct (name TEXT, balance NUMERIC(12,2))").unwrap();
    engine.execute("INSERT INTO acct VALUES ('Alice', 1000.00)").unwrap();

    let r = engine.execute("UPDATE acct SET balance = balance - 100 WHERE name = 'Alice'");
    assert!(r.is_ok(), "Numeric - Int should work: {:?}", r);

    let result = engine.execute("SELECT balance FROM acct WHERE name = 'Alice'").unwrap();
    let bal: f64 = match &result.rows[0].values[0] {
        crate::value::Value::Numeric(s) => s.parse().unwrap(),
        v => panic!("unexpected type {:?}", v),
    };
    assert!((bal - 900.0).abs() < 0.001, "expected 900, got {}", bal);

    // Also test Int + Numeric (reversed operand order)
    let r = engine.execute("UPDATE acct SET balance = 100 + balance WHERE name = 'Alice'");
    assert!(r.is_ok(), "Int + Numeric should work: {:?}", r);
}

// ── Savepoint / subtransaction undo ─────────────────────────────────────────

#[test]
fn test_savepoint_partial_rollback_preserves_prior_inserts() {
    // Core savepoint contract: ROLLBACK TO SAVEPOINT undoes only the work
    // done *after* the savepoint, leaving earlier work intact.
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());

    engine.execute("CREATE TABLE acct (name TEXT, balance INT)").unwrap();
    engine.execute_session("t", "BEGIN").unwrap();
    engine.execute_session("t", "INSERT INTO acct VALUES ('Alice', 1000)").unwrap();
    engine.execute_session("t", "SAVEPOINT sp1").unwrap();
    engine.execute_session("t", "INSERT INTO acct VALUES ('Bob', 500)").unwrap();

    // Bob was inserted after the savepoint — roll it back.
    engine.execute_session("t", "ROLLBACK TO SAVEPOINT sp1").unwrap();

    // Alice must still be there (inserted before the savepoint).
    let result = engine.execute_session("t", "SELECT name FROM acct").unwrap();
    let names: Vec<&str> = result.rows.iter()
        .map(|r| if let crate::value::Value::Text(s) = &r.values[0] { s.as_str() } else { "" })
        .collect();
    assert!(names.contains(&"Alice"), "Alice should survive ROLLBACK TO SAVEPOINT; got {:?}", names);
    assert!(!names.contains(&"Bob"),  "Bob was inserted after savepoint and should be gone; got {:?}", names);

    engine.execute_session("t", "COMMIT").unwrap();

    // After commit, Alice is persisted and Bob is gone.
    let result = engine.execute("SELECT name FROM acct").unwrap();
    let names: Vec<&str> = result.rows.iter()
        .map(|r| if let crate::value::Value::Text(s) = &r.values[0] { s.as_str() } else { "" })
        .collect();
    assert_eq!(names, vec!["Alice"]);
}

#[test]
fn test_savepoint_rollback_then_continue() {
    // After ROLLBACK TO SAVEPOINT the transaction stays open and new work
    // can be committed normally.
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());

    engine.execute("CREATE TABLE log (msg TEXT)").unwrap();
    engine.execute_session("t", "BEGIN").unwrap();
    engine.execute_session("t", "INSERT INTO log VALUES ('step1')").unwrap();
    engine.execute_session("t", "SAVEPOINT after_step1").unwrap();
    engine.execute_session("t", "INSERT INTO log VALUES ('bad_step')").unwrap();
    engine.execute_session("t", "ROLLBACK TO SAVEPOINT after_step1").unwrap();
    // Re-do with corrected value
    engine.execute_session("t", "INSERT INTO log VALUES ('step2')").unwrap();
    engine.execute_session("t", "COMMIT").unwrap();

    let result = engine.execute("SELECT msg FROM log ORDER BY msg").unwrap();
    let msgs: Vec<&str> = result.rows.iter()
        .map(|r| if let crate::value::Value::Text(s) = &r.values[0] { s.as_str() } else { "" })
        .collect();
    assert_eq!(msgs, vec!["step1", "step2"], "expected step1 and step2; got {:?}", msgs);
}

#[test]
fn test_savepoint_update_partial_rollback() {
    // UPDATE after a savepoint is rolled back, restoring original values.
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());

    engine.execute("CREATE TABLE acct (name TEXT, balance INT)").unwrap();
    engine.execute("INSERT INTO acct VALUES ('Alice', 1000)").unwrap();
    engine.execute("INSERT INTO acct VALUES ('Bob', 500)").unwrap();


    engine.execute_session("t", "BEGIN").unwrap();
    // Debit Alice
    engine.execute_session("t", "UPDATE acct SET balance = balance - 100 WHERE name = 'Alice'").unwrap();
    engine.execute_session("t", "SAVEPOINT after_debit").unwrap();
    // Credit Bob — then change mind
    engine.execute_session("t", "UPDATE acct SET balance = balance + 100 WHERE name = 'Bob'").unwrap();
    engine.execute_session("t", "ROLLBACK TO SAVEPOINT after_debit").unwrap();
    // Credit Bob with correct amount
    engine.execute_session("t", "UPDATE acct SET balance = balance + 100 WHERE name = 'Bob'").unwrap();
    engine.execute_session("t", "COMMIT").unwrap();

    let result = engine.execute("SELECT name, balance FROM acct ORDER BY name").unwrap();
    let rows: Vec<(&str, i64)> = result.rows.iter().map(|r| {
        let name = if let crate::value::Value::Text(s) = &r.values[0] { s.as_str() } else { "" };
        let bal = match &r.values[1] {
            crate::value::Value::Int4(i) => *i as i64,
            crate::value::Value::Int8(i) => *i,
            _ => -1,
        };
        (name, bal)
    }).collect();
    assert_eq!(rows, vec![("Alice", 900), ("Bob", 600)],
        "Alice -100, Bob +100; got {:?}", rows);
}

#[test]
fn test_savepoint_tutorial_numeric_balances() {
    // Exact tutorial scenario: NUMERIC(12,2) balances with integer literals in arithmetic.
    // Reproduces the "arithmetic type mismatch: Numeric vs Int4" regression.
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());

    engine.execute(
        "CREATE TABLE accounts (name TEXT NOT NULL, balance NUMERIC(12,2) NOT NULL)"
    ).unwrap();
    engine.execute("INSERT INTO accounts VALUES ('Alice', 1000.00)").unwrap();
    engine.execute("INSERT INTO accounts VALUES ('Bob', 500.00)").unwrap();

    // Each statement is sent as its own execute_session call, mirroring CLI behaviour.
    engine.execute_session("tut", "BEGIN").unwrap();
    engine.execute_session("tut", "UPDATE accounts SET balance = balance - 100 WHERE name = 'Alice'").unwrap();
    engine.execute_session("tut", "SAVEPOINT after_debit").unwrap();
    engine.execute_session("tut", "UPDATE accounts SET balance = balance + 50 WHERE name = 'Bob'").unwrap();
    engine.execute_session("tut", "ROLLBACK TO SAVEPOINT after_debit").unwrap();
    engine.execute_session("tut", "UPDATE accounts SET balance = balance + 100 WHERE name = 'Bob'").unwrap();
    engine.execute_session("tut", "COMMIT").unwrap();

    let result = engine.execute("SELECT name, balance FROM accounts ORDER BY name").unwrap();
    assert_eq!(result.rows.len(), 2);
    // Alice: 1000 - 100 = 900, Bob: 500 + 100 = 600 (the 50 credit was rolled back)
    let alice_bal = result.rows[0].values[1].to_string();
    let bob_bal   = result.rows[1].values[1].to_string();
    assert!(alice_bal.starts_with("900"), "Alice balance: {alice_bal}");
    assert!(bob_bal.starts_with("600"), "Bob balance: {bob_bal}");
}

/// Regression: `--` comments containing apostrophes (e.g. "Alice's") used to corrupt
/// the single-quote tracking in the SQL splitter, causing multiple statements to be
/// merged into one big string.  The merged string was then parsed as a batch by
/// sqlparser-rs, which produced a SAVEPOINT AST node that reached the planner and
/// triggered "Not implemented: statement type: SAVEPOINT".
///
/// This test sends the *exact* comment-annotated block the user pastes in the CLI —
/// as a single string to `execute_session_multi` (which exercises the same
/// `split_sql_statements` path that the CLI exercises).
#[test]
fn test_savepoint_with_comment_apostrophes() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());

    engine.execute(
        "CREATE TABLE accounts (name TEXT NOT NULL, balance NUMERIC(12,2) NOT NULL)"
    ).unwrap();
    engine.execute("INSERT INTO accounts VALUES ('Alice', 1000.00)").unwrap();
    engine.execute("INSERT INTO accounts VALUES ('Bob', 500.00)").unwrap();

    // Simulate pasting the whole tutorial block — comments with apostrophes included.
    let block = "
BEGIN;

-- Step 1: deduct from Alice's account
UPDATE accounts SET balance = balance - 100 WHERE name = 'Alice';
SAVEPOINT after_debit;

-- Step 2: credit Bob (imagine a logic error was detected here)
UPDATE accounts SET balance = balance + 100 WHERE name = 'Bob';

-- Roll back only step 2; Alice's debit is preserved:
ROLLBACK TO SAVEPOINT after_debit;

-- Retry step 2 with the corrected amount, then commit:
UPDATE accounts SET balance = balance + 100 WHERE name = 'Bob';
COMMIT;
";

    let results = engine.execute_session_multi("sess", block).unwrap();
    // Every statement must succeed; none may return an error.
    let commands: Vec<&str> = results.iter().map(|r| r.command.as_str()).collect();
    assert!(
        commands.contains(&"BEGIN"),
        "expected BEGIN in results, got {:?}", commands
    );
    assert!(
        commands.contains(&"SAVEPOINT"),
        "expected SAVEPOINT in results, got {:?}", commands
    );
    assert!(
        commands.contains(&"COMMIT"),
        "expected COMMIT in results, got {:?}", commands
    );

    // Final balances: Alice 900 (debit kept), Bob 600 (only the retry credit).
    let result = engine.execute("SELECT name, balance FROM accounts ORDER BY name").unwrap();
    let alice_bal = result.rows[0].values[1].to_string();
    let bob_bal   = result.rows[1].values[1].to_string();
    assert!(alice_bal.starts_with("900"), "Alice balance should be 900, got {alice_bal}");
    assert!(bob_bal.starts_with("600"), "Bob balance should be 600, got {bob_bal}");
}

/// The `split_sql_statements` splitter must not let apostrophes inside `--` comments
/// corrupt quote tracking — even when semicolons also appear inside the comment.
#[test]
fn test_split_not_confused_by_comment_apostrophe_and_semicolon() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());

    engine.execute("CREATE TABLE t (v INT)").unwrap();

    // The comment contains both an apostrophe AND a semicolon — the classic splitter trap.
    let sql = "INSERT INTO t VALUES (1); -- don't split here; really\nINSERT INTO t VALUES (2);";
    engine.execute_session_multi("s", sql).unwrap();

    let r = engine.execute("SELECT COUNT(*) FROM t").unwrap();
    let cnt = match &r.rows[0].values[0] {
        crate::value::Value::Int8(n) => *n,
        crate::value::Value::Int4(n) => *n as i64,
        _ => -1,
    };
    assert_eq!(cnt, 2, "both inserts should succeed, got count {cnt}");
}

// ══════════════════════════════════════════════════════════════════════════════
// PRODUCTION QUALITY EDGE-CASE TESTS
// ══════════════════════════════════════════════════════════════════════════════

// ─── MVCC / Transaction Isolation ────────────────────────────────────────────

/// Dirty-read prevention: uncommitted inserts must NOT be visible to other sessions.
#[test]
fn test_dirty_read_prevention() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (v INT)").unwrap();

    // Session A begins but does NOT commit.
    engine.execute_session("a", "BEGIN").unwrap();
    engine.execute_session("a", "INSERT INTO t VALUES (42)").unwrap();

    // Session B (auto-commit) must see zero rows — the insert is not committed.
    let r = engine.execute("SELECT COUNT(*) FROM t").unwrap();
    let cnt = match &r.rows[0].values[0] {
        crate::value::Value::Int8(n) => *n,
        crate::value::Value::Int4(n) => *n as i64,
        _ => -1,
    };
    assert_eq!(cnt, 0, "dirty read: uncommitted row visible, expected 0 got {cnt}");

    engine.execute_session("a", "ROLLBACK").unwrap();
}

/// Rollback must leave zero visible changes.
#[test]
fn test_rollback_leaves_no_changes() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (v INT)").unwrap();

    engine.execute_session("r", "BEGIN").unwrap();
    engine.execute_session("r", "INSERT INTO t VALUES (1)").unwrap();
    engine.execute_session("r", "INSERT INTO t VALUES (2)").unwrap();
    engine.execute_session("r", "ROLLBACK").unwrap();

    let r = engine.execute("SELECT COUNT(*) FROM t").unwrap();
    let cnt = match &r.rows[0].values[0] {
        crate::value::Value::Int8(n) => *n,
        crate::value::Value::Int4(n) => *n as i64,
        _ => -1,
    };
    assert_eq!(cnt, 0, "rollback failed: {cnt} rows visible");
}

/// Read-own-writes: within a transaction, a session must see its own inserts.
#[test]
fn test_read_own_writes_within_transaction() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (v INT)").unwrap();

    engine.execute_session("s", "BEGIN").unwrap();
    engine.execute_session("s", "INSERT INTO t VALUES (99)").unwrap();

    // Within the same transaction, must see the inserted row.
    let r = engine.execute_session("s", "SELECT v FROM t WHERE v = 99").unwrap();
    assert_eq!(r.rows.len(), 1, "read-own-writes failed: inserted row not visible within txn");

    engine.execute_session("s", "ROLLBACK").unwrap();
}

/// Committed writes become visible to later transactions.
#[test]
fn test_committed_write_visible_after_commit() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (v INT)").unwrap();

    engine.execute_session("w", "BEGIN").unwrap();
    engine.execute_session("w", "INSERT INTO t VALUES (7)").unwrap();
    engine.execute_session("w", "COMMIT").unwrap();

    let r = engine.execute("SELECT v FROM t WHERE v = 7").unwrap();
    assert_eq!(r.rows.len(), 1, "committed write not visible after commit");
}

/// UPDATE inside a transaction, rolled back — original value must be unchanged.
#[test]
fn test_update_rollback_restores_original() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (id INT, v INT)").unwrap();
    engine.execute("INSERT INTO t VALUES (1, 100)").unwrap();

    engine.execute_session("u", "BEGIN").unwrap();
    engine.execute_session("u", "UPDATE t SET v = 999 WHERE id = 1").unwrap();

    // Verify the session sees its own change.
    let mid = engine.execute_session("u", "SELECT v FROM t WHERE id = 1").unwrap();
    assert_eq!(mid.rows[0].values[0], crate::value::Value::Int4(999));

    engine.execute_session("u", "ROLLBACK").unwrap();

    // After rollback, original value must be back.
    let r = engine.execute("SELECT v FROM t WHERE id = 1").unwrap();
    assert_eq!(r.rows[0].values[0], crate::value::Value::Int4(100),
        "update rollback did not restore original value");
}

/// DELETE inside a transaction, rolled back — row must reappear.
#[test]
fn test_delete_rollback_restores_row() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (id INT)").unwrap();
    engine.execute("INSERT INTO t VALUES (5)").unwrap();

    engine.execute_session("d", "BEGIN").unwrap();
    engine.execute_session("d", "DELETE FROM t WHERE id = 5").unwrap();
    engine.execute_session("d", "ROLLBACK").unwrap();

    let r = engine.execute("SELECT COUNT(*) FROM t").unwrap();
    let cnt = match &r.rows[0].values[0] {
        crate::value::Value::Int8(n) => *n,
        crate::value::Value::Int4(n) => *n as i64,
        _ => -1,
    };
    assert_eq!(cnt, 1, "deleted row not restored after rollback");
}

// ─── NULL semantics ───────────────────────────────────────────────────────────

#[test]
fn test_null_insert_into_nullable_column() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (a INT, b TEXT)").unwrap();
    engine.execute("INSERT INTO t (a) VALUES (1)").unwrap();

    let r = engine.execute("SELECT b FROM t WHERE a = 1").unwrap();
    assert_eq!(r.rows[0].values[0], crate::value::Value::Null,
        "omitted column should be NULL");
}

#[test]
fn test_not_null_constraint_rejected() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (a INT NOT NULL, b TEXT)").unwrap();

    let err = engine.execute("INSERT INTO t (b) VALUES ('hello')");
    assert!(err.is_err(), "INSERT violating NOT NULL must fail");
}

#[test]
fn test_null_comparison_is_null() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (v INT)").unwrap();
    engine.execute("INSERT INTO t VALUES (NULL)").unwrap();
    engine.execute("INSERT INTO t VALUES (1)").unwrap();

    let r = engine.execute("SELECT COUNT(*) FROM t WHERE v IS NULL").unwrap();
    let cnt = match &r.rows[0].values[0] {
        crate::value::Value::Int8(n) => *n,
        crate::value::Value::Int4(n) => *n as i64,
        _ => -1,
    };
    assert_eq!(cnt, 1);
}

#[test]
fn test_null_not_equal_to_value() {
    // NULL = 1 is not TRUE — NULL comparisons are always unknown/false.
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (v INT)").unwrap();
    engine.execute("INSERT INTO t VALUES (NULL)").unwrap();

    let r = engine.execute("SELECT COUNT(*) FROM t WHERE v = 1").unwrap();
    let cnt = match &r.rows[0].values[0] {
        crate::value::Value::Int8(n) => *n,
        crate::value::Value::Int4(n) => *n as i64,
        _ => -1,
    };
    assert_eq!(cnt, 0, "NULL = 1 should be false, not match");
}

// ─── Constraint violations ────────────────────────────────────────────────────

#[test]
fn test_unique_constraint_violation() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (id INT UNIQUE, v TEXT)").unwrap();
    engine.execute("INSERT INTO t VALUES (1, 'a')").unwrap();

    let err = engine.execute("INSERT INTO t VALUES (1, 'b')");
    assert!(err.is_err(), "duplicate UNIQUE key must be rejected");
}

#[test]
fn test_primary_key_uniqueness() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (id INT PRIMARY KEY, v TEXT)").unwrap();
    engine.execute("INSERT INTO t VALUES (1, 'first')").unwrap();

    let err = engine.execute("INSERT INTO t VALUES (1, 'duplicate')");
    assert!(err.is_err(), "duplicate PRIMARY KEY must be rejected");
}

#[test]
fn test_check_constraint_enforced_on_update() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (v INT CHECK (v > 0))").unwrap();
    engine.execute("INSERT INTO t VALUES (5)").unwrap();

    let err = engine.execute("UPDATE t SET v = -1 WHERE v = 5");
    assert!(err.is_err(), "UPDATE violating CHECK must be rejected");
}

#[test]
fn test_fk_violation_on_insert() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE parent (id INT PRIMARY KEY)").unwrap();
    engine.execute("CREATE TABLE child (id INT, parent_id INT REFERENCES parent(id))").unwrap();

    let err = engine.execute("INSERT INTO child VALUES (1, 999)");
    assert!(err.is_err(), "INSERT with non-existent FK target must fail");
}

#[test]
fn test_fk_cascade_delete() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE parent (id INT PRIMARY KEY)").unwrap();
    engine.execute(
        "CREATE TABLE child (id INT, parent_id INT REFERENCES parent(id) ON DELETE CASCADE)"
    ).unwrap();
    engine.execute("INSERT INTO parent VALUES (1)").unwrap();
    engine.execute("INSERT INTO child VALUES (10, 1)").unwrap();
    engine.execute("INSERT INTO child VALUES (11, 1)").unwrap();

    engine.execute("DELETE FROM parent WHERE id = 1").unwrap();

    let r = engine.execute("SELECT COUNT(*) FROM child").unwrap();
    let cnt = match &r.rows[0].values[0] {
        crate::value::Value::Int8(n) => *n,
        crate::value::Value::Int4(n) => *n as i64,
        _ => -1,
    };
    assert_eq!(cnt, 0, "cascade delete should remove child rows, got {cnt}");
}

#[test]
fn test_fk_restrict_delete_blocked() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE parent (id INT PRIMARY KEY)").unwrap();
    engine.execute(
        "CREATE TABLE child (id INT, parent_id INT REFERENCES parent(id))"
    ).unwrap();
    engine.execute("INSERT INTO parent VALUES (1)").unwrap();
    engine.execute("INSERT INTO child VALUES (10, 1)").unwrap();

    let err = engine.execute("DELETE FROM parent WHERE id = 1");
    assert!(err.is_err(), "DELETE restricted by FK must fail");
}

// ─── Aggregate edge cases ─────────────────────────────────────────────────────

#[test]
fn test_count_star_on_empty_table() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (v INT)").unwrap();
    let r = engine.execute("SELECT COUNT(*) FROM t").unwrap();
    let cnt = match &r.rows[0].values[0] {
        crate::value::Value::Int8(n) => *n,
        crate::value::Value::Int4(n) => *n as i64,
        _ => -1,
    };
    assert_eq!(cnt, 0);
}

#[test]
fn test_sum_on_empty_table_returns_null() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (v INT)").unwrap();
    let r = engine.execute("SELECT SUM(v) FROM t").unwrap();
    assert_eq!(r.rows[0].values[0], crate::value::Value::Null,
        "SUM of empty table must be NULL");
}

#[test]
fn test_avg_on_empty_table_returns_null() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (v INT)").unwrap();
    let r = engine.execute("SELECT AVG(v) FROM t").unwrap();
    assert_eq!(r.rows[0].values[0], crate::value::Value::Null,
        "AVG of empty table must be NULL");
}

#[test]
fn test_min_max_on_single_row() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (v INT)").unwrap();
    engine.execute("INSERT INTO t VALUES (42)").unwrap();
    let r = engine.execute("SELECT MIN(v), MAX(v) FROM t").unwrap();
    assert_eq!(r.rows[0].values[0], crate::value::Value::Int4(42));
    assert_eq!(r.rows[0].values[1], crate::value::Value::Int4(42));
}

// ─── Data persistence / WAL recovery ──────────────────────────────────────────

/// Data written and committed must survive an engine restart (WAL + heap recovery).
#[test]
fn test_data_survives_restart() {
    let dir = tempdir().unwrap();
    {
        let (engine, _) = make_engine_with_catalog(dir.path());
        engine.execute("CREATE TABLE t (id INT, name TEXT)").unwrap();
        engine.execute("INSERT INTO t VALUES (1, 'Alice')").unwrap();
        engine.execute("INSERT INTO t VALUES (2, 'Bob')").unwrap();
    } // Engine dropped here — simulates server restart.

    let (engine2, _) = reopen_engine(dir.path());
    let r = engine2.execute("SELECT COUNT(*) FROM t").unwrap();
    let cnt = match &r.rows[0].values[0] {
        crate::value::Value::Int8(n) => *n,
        crate::value::Value::Int4(n) => *n as i64,
        _ => -1,
    };
    assert_eq!(cnt, 2, "data not persisted across restart");
}

/// Schema (table definitions) must persist across restarts.
#[test]
fn test_schema_survives_restart() {
    let dir = tempdir().unwrap();
    {
        let (engine, _) = make_engine_with_catalog(dir.path());
        engine.execute(
            "CREATE TABLE persist_me (id SERIAL PRIMARY KEY, val TEXT NOT NULL)"
        ).unwrap();
    }

    let (engine2, _) = reopen_engine(dir.path());
    // Should be able to insert into the table that was created before restart.
    engine2.execute("INSERT INTO persist_me (val) VALUES ('hello')").unwrap();
    let r = engine2.execute("SELECT val FROM persist_me").unwrap();
    assert_eq!(r.rows.len(), 1);
}

/// Uncommitted transaction at shutdown must not appear after restart.
#[test]
fn test_uncommitted_txn_not_visible_after_restart() {
    let dir = tempdir().unwrap();
    {
        let (engine, _) = make_engine_with_catalog(dir.path());
        engine.execute("CREATE TABLE t (v INT)").unwrap();

        // Start a transaction but don't commit — engine will drop mid-txn.
        engine.execute_session("orphan", "BEGIN").unwrap();
        engine.execute_session("orphan", "INSERT INTO t VALUES (999)").unwrap();
        // No COMMIT — session is abandoned on engine drop.
    }

    let (engine2, _) = reopen_engine(dir.path());
    let r = engine2.execute("SELECT COUNT(*) FROM t").unwrap();
    let cnt = match &r.rows[0].values[0] {
        crate::value::Value::Int8(n) => *n,
        crate::value::Value::Int4(n) => *n as i64,
        _ => -1,
    };
    assert_eq!(cnt, 0, "uncommitted insert visible after restart — durability violated");
}

/// Index must survive restart and still support queries.
#[test]
fn test_index_survives_restart() {
    let dir = tempdir().unwrap();
    {
        let (engine, _) = make_engine_with_catalog(dir.path());
        engine.execute("CREATE TABLE t (id INT)").unwrap();
        engine.execute("CREATE INDEX idx_t_id ON t (id)").unwrap();
        for i in 0..50i32 {
            engine.execute(&format!("INSERT INTO t VALUES ({i})")).unwrap();
        }
    }

    let (engine2, _) = reopen_engine(dir.path());
    let r = engine2.execute("SELECT id FROM t WHERE id = 25").unwrap();
    assert_eq!(r.rows.len(), 1, "index lookup failed after restart");
}

// ─── Nested SAVEPOINTs ─────────────────────────────────────────────────────────

#[test]
fn test_nested_savepoints() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (v INT)").unwrap();

    engine.execute_session("n", "BEGIN").unwrap();
    engine.execute_session("n", "INSERT INTO t VALUES (1)").unwrap();
    engine.execute_session("n", "SAVEPOINT sp1").unwrap();
    engine.execute_session("n", "INSERT INTO t VALUES (2)").unwrap();
    engine.execute_session("n", "SAVEPOINT sp2").unwrap();
    engine.execute_session("n", "INSERT INTO t VALUES (3)").unwrap();
    // Roll back to sp2 — removes 3, keeps 1 and 2.
    engine.execute_session("n", "ROLLBACK TO SAVEPOINT sp2").unwrap();
    // Roll back to sp1 — removes 2 as well, keeps only 1.
    engine.execute_session("n", "ROLLBACK TO SAVEPOINT sp1").unwrap();
    engine.execute_session("n", "COMMIT").unwrap();

    let r = engine.execute("SELECT v FROM t ORDER BY v").unwrap();
    assert_eq!(r.rows.len(), 1, "nested savepoints: expected 1 row");
    assert_eq!(r.rows[0].values[0], crate::value::Value::Int4(1));
}

#[test]
fn test_release_savepoint_merges_into_parent() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (v INT)").unwrap();

    engine.execute_session("r", "BEGIN").unwrap();
    engine.execute_session("r", "INSERT INTO t VALUES (10)").unwrap();
    engine.execute_session("r", "SAVEPOINT sp").unwrap();
    engine.execute_session("r", "INSERT INTO t VALUES (20)").unwrap();
    engine.execute_session("r", "RELEASE SAVEPOINT sp").unwrap();
    // After RELEASE, the insert of 20 is merged into the outer transaction.
    engine.execute_session("r", "COMMIT").unwrap();

    let r = engine.execute("SELECT COUNT(*) FROM t").unwrap();
    let cnt = match &r.rows[0].values[0] {
        crate::value::Value::Int8(n) => *n,
        crate::value::Value::Int4(n) => *n as i64,
        _ => -1,
    };
    assert_eq!(cnt, 2, "released savepoint should commit both rows");
}

// ─── SQL edge cases ───────────────────────────────────────────────────────────

#[test]
fn test_update_where_no_match_returns_zero() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (id INT, v INT)").unwrap();
    engine.execute("INSERT INTO t VALUES (1, 10)").unwrap();

    let r = engine.execute("UPDATE t SET v = 99 WHERE id = 999").unwrap();
    assert_eq!(r.rows_affected, 0, "UPDATE with no match should affect 0 rows");
}

#[test]
fn test_delete_where_no_match_returns_zero() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (id INT)").unwrap();
    engine.execute("INSERT INTO t VALUES (1)").unwrap();

    let r = engine.execute("DELETE FROM t WHERE id = 999").unwrap();
    assert_eq!(r.rows_affected, 0, "DELETE with no match should affect 0 rows");
}

#[test]
fn test_select_from_nonexistent_table_errors() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    let err = engine.execute("SELECT * FROM nonexistent_table");
    assert!(err.is_err(), "SELECT from non-existent table must error");
}

#[test]
fn test_insert_wrong_column_count_errors() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (a INT, b INT, c INT)").unwrap();

    let err = engine.execute("INSERT INTO t VALUES (1, 2)");
    assert!(err.is_err(), "INSERT with wrong column count must error");
}

#[test]
fn test_division_by_zero_errors() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    let err = engine.execute("SELECT 10 / 0");
    assert!(err.is_err(), "division by zero must error");
}

#[test]
fn test_large_table_insert_and_count() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (id INT, val TEXT)").unwrap();

    for i in 0..500i32 {
        engine.execute(&format!("INSERT INTO t VALUES ({i}, 'row{i}')")).unwrap();
    }

    let r = engine.execute("SELECT COUNT(*) FROM t").unwrap();
    let cnt = match &r.rows[0].values[0] {
        crate::value::Value::Int8(n) => *n,
        crate::value::Value::Int4(n) => *n as i64,
        _ => -1,
    };
    assert_eq!(cnt, 500);
}

#[test]
fn test_truncate_via_delete_all() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (v INT)").unwrap();
    for i in 0..20i32 {
        engine.execute(&format!("INSERT INTO t VALUES ({i})")).unwrap();
    }
    engine.execute("DELETE FROM t").unwrap();

    let r = engine.execute("SELECT COUNT(*) FROM t").unwrap();
    let cnt = match &r.rows[0].values[0] {
        crate::value::Value::Int8(n) => *n,
        crate::value::Value::Int4(n) => *n as i64,
        _ => -1,
    };
    assert_eq!(cnt, 0, "DELETE without WHERE must remove all rows");
}

#[test]
fn test_insert_on_conflict_do_nothing() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (id INT PRIMARY KEY, v TEXT)").unwrap();
    engine.execute("INSERT INTO t VALUES (1, 'original')").unwrap();

    // ON CONFLICT DO NOTHING must not error and must not change the row.
    engine.execute("INSERT INTO t VALUES (1, 'conflict') ON CONFLICT DO NOTHING").unwrap();

    let r = engine.execute("SELECT v FROM t WHERE id = 1").unwrap();
    assert_eq!(r.rows[0].values[0], crate::value::Value::Text("original".into()),
        "ON CONFLICT DO NOTHING must not overwrite");
}

#[test]
fn test_insert_on_conflict_do_update() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (id INT PRIMARY KEY, v TEXT)").unwrap();
    engine.execute("INSERT INTO t VALUES (1, 'original')").unwrap();

    engine.execute(
        "INSERT INTO t VALUES (1, 'updated') ON CONFLICT (id) DO UPDATE SET v = 'updated'"
    ).unwrap();

    let r = engine.execute("SELECT v FROM t WHERE id = 1").unwrap();
    assert_eq!(r.rows[0].values[0], crate::value::Value::Text("updated".into()),
        "ON CONFLICT DO UPDATE must overwrite");
}

#[test]
fn test_alter_table_add_column_existing_rows_get_null() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (id INT)").unwrap();
    engine.execute("INSERT INTO t VALUES (1)").unwrap();
    engine.execute("ALTER TABLE t ADD COLUMN extra TEXT").unwrap();

    let r = engine.execute("SELECT extra FROM t WHERE id = 1").unwrap();
    assert_eq!(r.rows[0].values[0], crate::value::Value::Null,
        "new column in existing row should be NULL");
}

#[test]
fn test_cte_basic() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (v INT)").unwrap();
    for i in 1..=5i32 {
        engine.execute(&format!("INSERT INTO t VALUES ({i})")).unwrap();
    }

    let r = engine.execute(
        "WITH big AS (SELECT v FROM t WHERE v > 3) SELECT COUNT(*) FROM big"
    ).unwrap();
    let cnt = match &r.rows[0].values[0] {
        crate::value::Value::Int8(n) => *n,
        crate::value::Value::Int4(n) => *n as i64,
        _ => -1,
    };
    assert_eq!(cnt, 2, "CTE should filter to v=4,5");
}

#[test]
fn test_subquery_in_where() {
    let dir = tempdir().unwrap();
    let (engine, _) = make_engine_with_catalog(dir.path());
    engine.execute("CREATE TABLE t (id INT, v INT)").unwrap();
    for i in 1..=5i32 {
        engine.execute(&format!("INSERT INTO t VALUES ({i}, {})", i * 10)).unwrap();
    }

    let r = engine.execute(
        "SELECT id FROM t WHERE v > (SELECT AVG(v) FROM t)"
    ).unwrap();
    // AVG = 30; rows with v > 30 are id=4 (v=40) and id=5 (v=50).
    assert_eq!(r.rows.len(), 2);
}

// ─── WAL record robustness ─────────────────────────────────────────────────────

#[test]
fn test_wal_decode_rejects_truncated_record() {
    use wal::record::WalRecord;
    // A record that's only 3 bytes — far too short.
    let result = WalRecord::decode(&[0x01, 0x00, 0x00]);
    assert!(result.is_err(), "truncated WAL record must be rejected");
}

#[test]
fn test_wal_decode_rejects_crc_mismatch() {
    use wal::record::{WalRecord, WalRecordType};
    // Build a valid Commit record, then corrupt the trailing CRC bytes.
    let rec = WalRecord::new(1, WalRecordType::Commit, 0, vec![]);
    let mut encoded = rec.encode();
    let last = encoded.len() - 1;
    encoded[last] ^= 0xFF;
    let result = WalRecord::decode(&encoded);
    assert!(result.is_err(), "CRC-corrupted WAL record must be rejected");
}

