/// Float type tests
/// Based on PostgreSQL float8.sql patterns with self-contained data.
use tempfile::TempDir;
use crate::common::{make_engine, exec, query_int, Backend};
use sql::Value;

fn setup_float_tbl(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE float_tbl (f FLOAT)");
    exec(b, "INSERT INTO float_tbl VALUES (0.0)");
    exec(b, "INSERT INTO float_tbl VALUES (1.5)");
    exec(b, "INSERT INTO float_tbl VALUES (-2.5)");
    exec(b, "INSERT INTO float_tbl VALUES (1004.3)");
    exec(b, "INSERT INTO float_tbl VALUES (-34.84)");
}

fn test_float_basic_addition_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 1.5 + 2.5");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 4.0).abs() < 1e-9, "1.5 + 2.5 = 4.0"),
        Some(Value::Int4(4)) => {}
        other => panic!("Expected 4.0, got {:?}", other),
    }
}

#[test]
fn test_float_basic_addition() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_basic_addition_body(&b);
}

crate::net_tests!(test_float_basic_addition);


fn test_float_division_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 10.0 / 3.0");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 3.333333).abs() < 0.001, "10.0 / 3.0 ≈ 3.333"),
        other => panic!("Expected ~3.333, got {:?}", other),
    }
}

#[test]
fn test_float_division() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_division_body(&b);
}

crate::net_tests!(test_float_division);


fn test_float_subtraction_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 5.5 - 2.3");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 3.2).abs() < 1e-9, "5.5 - 2.3 = 3.2"),
        other => panic!("Expected 3.2, got {:?}", other),
    }
}

#[test]
fn test_float_subtraction() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_subtraction_body(&b);
}

crate::net_tests!(test_float_subtraction);


fn test_float_multiplication_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 2.5 * 4.0");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 10.0).abs() < 1e-9, "2.5 * 4.0 = 10.0"),
        other => panic!("Expected 10.0, got {:?}", other),
    }
}

#[test]
fn test_float_multiplication() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_multiplication_body(&b);
}

crate::net_tests!(test_float_multiplication);


fn test_float_comparison_less_than_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 1.5 < 2.5");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("1.5 < 2.5 should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_float_comparison_less_than() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_comparison_less_than_body(&b);
}

crate::net_tests!(test_float_comparison_less_than);


fn test_float_comparison_equal_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 3.14 = 3.14");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("3.14 = 3.14 should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_float_comparison_equal() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_comparison_equal_body(&b);
}

crate::net_tests!(test_float_comparison_equal);


fn test_float_abs_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT ABS(-3.14)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 3.14).abs() < 1e-9),
        other => panic!("ABS(-3.14) = 3.14, got {:?}", other),
    }
}

#[test]
fn test_float_abs() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_abs_body(&b);
}

crate::net_tests!(test_float_abs);


fn test_float_abs_positive_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT ABS(5.5)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 5.5).abs() < 1e-9),
        other => panic!("ABS(5.5) = 5.5, got {:?}", other),
    }
}

#[test]
fn test_float_abs_positive() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_abs_positive_body(&b);
}

crate::net_tests!(test_float_abs_positive);


fn test_float_special_infinity_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 'Infinity'::FLOAT8");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!(v.is_infinite() && *v > 0.0, "Should be +Infinity"),
        other => panic!("Expected Infinity, got {:?}", other),
    }
}

#[test]
fn test_float_special_infinity() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_special_infinity_body(&b);
}

crate::net_tests!(test_float_special_infinity);


fn test_float_special_neg_infinity_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT '-Infinity'::FLOAT8");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!(v.is_infinite() && *v < 0.0, "Should be -Infinity"),
        other => panic!("Expected -Infinity, got {:?}", other),
    }
}

#[test]
fn test_float_special_neg_infinity() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_special_neg_infinity_body(&b);
}

crate::net_tests!(test_float_special_neg_infinity);


fn test_float_special_nan_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 'NaN'::FLOAT8");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!(v.is_nan(), "Should be NaN"),
        other => panic!("Expected NaN, got {:?}", other),
    }
}

#[test]
fn test_float_special_nan() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_special_nan_body(&b);
}

crate::net_tests!(test_float_special_nan);


fn test_float_infinity_plus_number_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 'Infinity'::FLOAT8 + 100.0");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!(v.is_infinite() && *v > 0.0, "Infinity + 100 = Infinity"),
        other => panic!("Expected Infinity, got {:?}", other),
    }
}

#[test]
fn test_float_infinity_plus_number() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_infinity_plus_number_body(&b);
}

crate::net_tests!(test_float_infinity_plus_number);


fn test_float_finite_divided_by_infinity_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 42.0 / 'Infinity'::FLOAT8");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v).abs() < 1e-9, "42 / Infinity = 0"),
        other => panic!("Expected 0.0, got {:?}", other),
    }
}

#[test]
fn test_float_finite_divided_by_infinity() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_finite_divided_by_infinity_body(&b);
}

crate::net_tests!(test_float_finite_divided_by_infinity);


fn test_float_from_table_where_body(b: &crate::common::Backend) {
    setup_float_tbl(b);
    let result = exec(b, "SELECT f FROM float_tbl WHERE f > 0.0 ORDER BY f");
    assert!(result.rows.len() > 0, "Some floats > 0");
    for row in &result.rows {
        match row.get_by_idx(0) {
            Some(Value::Float8(v)) => assert!(*v > 0.0),
            other => panic!("{:?}", other),
        }
    }
}

#[test]
fn test_float_from_table_where() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_from_table_where_body(&b);
}

crate::net_tests!(test_float_from_table_where);


fn test_float_sum_aggregate_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE measurements (val FLOAT)");
    exec(b, "INSERT INTO measurements VALUES (1.5), (2.5), (3.0), (4.0)");
    let result = exec(b, "SELECT SUM(val) FROM measurements");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 11.0).abs() < 1e-9, "SUM = 11.0"),
        other => panic!("Expected 11.0, got {:?}", other),
    }
}

#[test]
fn test_float_sum_aggregate() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_sum_aggregate_body(&b);
}

crate::net_tests!(test_float_sum_aggregate);


fn test_float_avg_aggregate_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE meas2 (val FLOAT)");
    exec(b, "INSERT INTO meas2 VALUES (2.0), (4.0), (6.0)");
    let result = exec(b, "SELECT AVG(val) FROM meas2");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 4.0).abs() < 1e-9, "AVG = 4.0"),
        other => panic!("Expected 4.0, got {:?}", other),
    }
}

#[test]
fn test_float_avg_aggregate() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_avg_aggregate_body(&b);
}

crate::net_tests!(test_float_avg_aggregate);


fn test_float_min_max_body(b: &crate::common::Backend) {
    setup_float_tbl(b);
    let result = exec(b, "SELECT MIN(f), MAX(f) FROM float_tbl");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!(*v < 0.0, "MIN should be negative"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_float_min_max() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_min_max_body(&b);
}

crate::net_tests!(test_float_min_max);


fn test_float_order_by_float_column_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE sorted_floats (f FLOAT)");
    exec(b, "INSERT INTO sorted_floats VALUES (3.3), (1.1), (2.2), (0.5)");
    let result = exec(b, "SELECT f FROM sorted_floats ORDER BY f ASC");
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
fn test_float_order_by_float_column() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_order_by_float_column_body(&b);
}

crate::net_tests!(test_float_order_by_float_column);


fn test_float_to_int_cast_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 3.9::INT");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(v)) => assert!(*v == 3 || *v == 4, "3.9::INT should be 3 or 4 (truncate or round)"),
        other => panic!("Expected integer, got {:?}", other),
    }
}

#[test]
fn test_float_to_int_cast() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_to_int_cast_body(&b);
}

crate::net_tests!(test_float_to_int_cast);


fn test_int_to_float_cast_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 5::FLOAT8");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 5.0).abs() < 1e-9),
        other => panic!("Expected 5.0, got {:?}", other),
    }
}

#[test]
fn test_int_to_float_cast() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_int_to_float_cast_body(&b);
}

crate::net_tests!(test_int_to_float_cast);


fn test_float_negative_in_where_body(b: &crate::common::Backend) {
    setup_float_tbl(b);
    let result = exec(b, "SELECT f FROM float_tbl WHERE f < 0.0 ORDER BY f");
    assert!(result.rows.len() > 0, "Some floats are negative");
    for row in &result.rows {
        match row.get_by_idx(0) {
            Some(Value::Float8(v)) => assert!(*v < 0.0, "Should be negative"),
            other => panic!("{:?}", other),
        }
    }
}

#[test]
fn test_float_negative_in_where() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_float_negative_in_where_body(&b);
}

crate::net_tests!(test_float_negative_in_where);

