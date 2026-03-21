/// Float type tests
/// Based on PostgreSQL float8.sql patterns with self-contained data.
use tempfile::TempDir;
use crate::common::{make_engine, exec, query_int};
use sql::Value;

fn setup_float_tbl(engine: &std::sync::Arc<sql::engine::QueryEngine>) {
    exec(engine, "CREATE TABLE float_tbl (f FLOAT)");
    exec(engine, "INSERT INTO float_tbl VALUES (0.0)");
    exec(engine, "INSERT INTO float_tbl VALUES (1.5)");
    exec(engine, "INSERT INTO float_tbl VALUES (-2.5)");
    exec(engine, "INSERT INTO float_tbl VALUES (1004.3)");
    exec(engine, "INSERT INTO float_tbl VALUES (-34.84)");
}

#[test]
fn test_float_basic_addition() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 1.5 + 2.5");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 4.0).abs() < 1e-9, "1.5 + 2.5 = 4.0"),
        Some(Value::Int4(4)) => {}
        other => panic!("Expected 4.0, got {:?}", other),
    }
}

#[test]
fn test_float_division() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 10.0 / 3.0");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 3.333333).abs() < 0.001, "10.0 / 3.0 ≈ 3.333"),
        other => panic!("Expected ~3.333, got {:?}", other),
    }
}

#[test]
fn test_float_subtraction() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 5.5 - 2.3");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 3.2).abs() < 1e-9, "5.5 - 2.3 = 3.2"),
        other => panic!("Expected 3.2, got {:?}", other),
    }
}

#[test]
fn test_float_multiplication() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 2.5 * 4.0");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 10.0).abs() < 1e-9, "2.5 * 4.0 = 10.0"),
        other => panic!("Expected 10.0, got {:?}", other),
    }
}

#[test]
fn test_float_comparison_less_than() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 1.5 < 2.5");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("1.5 < 2.5 should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_float_comparison_equal() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 3.14 = 3.14");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("3.14 = 3.14 should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_float_abs() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT ABS(-3.14)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 3.14).abs() < 1e-9),
        other => panic!("ABS(-3.14) = 3.14, got {:?}", other),
    }
}

#[test]
fn test_float_abs_positive() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT ABS(5.5)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 5.5).abs() < 1e-9),
        other => panic!("ABS(5.5) = 5.5, got {:?}", other),
    }
}

#[test]
fn test_float_special_infinity() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 'Infinity'::FLOAT8");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!(v.is_infinite() && *v > 0.0, "Should be +Infinity"),
        other => panic!("Expected Infinity, got {:?}", other),
    }
}

#[test]
fn test_float_special_neg_infinity() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT '-Infinity'::FLOAT8");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!(v.is_infinite() && *v < 0.0, "Should be -Infinity"),
        other => panic!("Expected -Infinity, got {:?}", other),
    }
}

#[test]
fn test_float_special_nan() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 'NaN'::FLOAT8");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!(v.is_nan(), "Should be NaN"),
        other => panic!("Expected NaN, got {:?}", other),
    }
}

#[test]
fn test_float_infinity_plus_number() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 'Infinity'::FLOAT8 + 100.0");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!(v.is_infinite() && *v > 0.0, "Infinity + 100 = Infinity"),
        other => panic!("Expected Infinity, got {:?}", other),
    }
}

#[test]
fn test_float_finite_divided_by_infinity() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 42.0 / 'Infinity'::FLOAT8");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v).abs() < 1e-9, "42 / Infinity = 0"),
        other => panic!("Expected 0.0, got {:?}", other),
    }
}

#[test]
fn test_float_from_table_where() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_float_tbl(&engine);
    let result = exec(&engine, "SELECT f FROM float_tbl WHERE f > 0.0 ORDER BY f");
    assert!(result.rows.len() > 0, "Some floats > 0");
    for row in &result.rows {
        match row.get_by_idx(0) {
            Some(Value::Float8(v)) => assert!(*v > 0.0),
            other => panic!("{:?}", other),
        }
    }
}

#[test]
fn test_float_sum_aggregate() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE measurements (val FLOAT)");
    exec(&engine, "INSERT INTO measurements VALUES (1.5), (2.5), (3.0), (4.0)");
    let result = exec(&engine, "SELECT SUM(val) FROM measurements");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 11.0).abs() < 1e-9, "SUM = 11.0"),
        other => panic!("Expected 11.0, got {:?}", other),
    }
}

#[test]
fn test_float_avg_aggregate() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE meas2 (val FLOAT)");
    exec(&engine, "INSERT INTO meas2 VALUES (2.0), (4.0), (6.0)");
    let result = exec(&engine, "SELECT AVG(val) FROM meas2");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 4.0).abs() < 1e-9, "AVG = 4.0"),
        other => panic!("Expected 4.0, got {:?}", other),
    }
}

#[test]
fn test_float_min_max() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_float_tbl(&engine);
    let result = exec(&engine, "SELECT MIN(f), MAX(f) FROM float_tbl");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!(*v < 0.0, "MIN should be negative"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_float_order_by_float_column() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE sorted_floats (f FLOAT)");
    exec(&engine, "INSERT INTO sorted_floats VALUES (3.3), (1.1), (2.2), (0.5)");
    let result = exec(&engine, "SELECT f FROM sorted_floats ORDER BY f ASC");
    assert_eq!(result.rows.len(), 4);
    let vals: Vec<f64> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Float8(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    for i in 0..vals.len()-1 {
        assert!(vals[i] <= vals[i+1], "Values should be sorted ascending");
    }
}

#[test]
fn test_float_to_int_cast() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 3.9::INT");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(v)) => assert!(*v == 3 || *v == 4, "3.9::INT should be 3 or 4 (truncate or round)"),
        other => panic!("Expected integer, got {:?}", other),
    }
}

#[test]
fn test_int_to_float_cast() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 5::FLOAT8");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 5.0).abs() < 1e-9),
        other => panic!("Expected 5.0, got {:?}", other),
    }
}

#[test]
fn test_float_negative_in_where() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_float_tbl(&engine);
    let result = exec(&engine, "SELECT f FROM float_tbl WHERE f < 0.0 ORDER BY f");
    assert!(result.rows.len() > 0, "Some floats are negative");
    for row in &result.rows {
        match row.get_by_idx(0) {
            Some(Value::Float8(v)) => assert!(*v < 0.0, "Should be negative"),
            other => panic!("{:?}", other),
        }
    }
}
