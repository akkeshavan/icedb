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
