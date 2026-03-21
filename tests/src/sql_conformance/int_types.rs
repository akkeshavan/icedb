/// Integer type tests (INT4 and INT8)
/// Based on PostgreSQL int4.sql and int8.sql patterns with self-contained data.
use tempfile::TempDir;
use crate::common::{make_engine, exec, exec_err, query_int};
use sql::Value;

#[test]
fn test_int4_basic_arithmetic_add() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT -2 + 3");
    assert_eq!(n, 1);
}

#[test]
fn test_int4_subtract() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT 4 - 2");
    assert_eq!(n, 2);
}

#[test]
fn test_int4_subtract_negative() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT 2 - -1");
    assert_eq!(n, 3);
}

#[test]
fn test_int4_multiply() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT 3 * 7");
    assert_eq!(n, 21);
}

#[test]
fn test_int4_divide() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT 10 / 3");
    // Integer division truncates toward zero
    assert_eq!(n, 3, "10 / 3 = 3 (truncated)");
}

#[test]
fn test_int4_divide_negative_truncates_toward_zero() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT -7 / 2");
    assert_eq!(n, -3, "-7 / 2 = -3 (truncated toward zero)");
}

#[test]
fn test_int4_modulo_positive() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT 7 % 3");
    assert_eq!(n, 1);
}

#[test]
fn test_int4_modulo_negative() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT -7 % 3");
    assert_eq!(n, -1, "-7 % 3 = -1 in PostgreSQL");
}

#[test]
fn test_int4_modulo_zero_remainder() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT 8 % 4");
    assert_eq!(n, 0);
}

#[test]
fn test_int4_comparison_less_than() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 3 < 5");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("3 < 5 should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_int4_comparison_greater_than() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 5 > 3");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("5 > 3 should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_int4_comparison_not_equal() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 3 <> 5");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("3 <> 5 should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_int4_comparison_equal() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 42 = 42");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("42 = 42 should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_int4_abs() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT ABS(-42)");
    assert_eq!(n, 42);
}

#[test]
fn test_int4_negation() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT -(-5)");
    assert_eq!(n, 5);
}

#[test]
fn test_int4_order_of_operations() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT 2 + 2 / 2");
    assert_eq!(n, 3, "Division before addition: 2 + 1 = 3");
}



#[test]
fn test_int4_parentheses_override_precedence() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT (2 + 2) / 2");
    assert_eq!(n, 2, "(4) / 2 = 2");
}

#[test]
fn test_int4_max_value() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT 2147483647");
    assert_eq!(n, 2147483647, "MAX int4");
}

#[test]
fn test_int4_min_value() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT -2147483648");
    assert_eq!(n, -2147483648, "MIN int4");
}

#[test]
fn test_int4_div_by_zero_error() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let err = exec_err(&engine, "SELECT 1 / 0");
    let msg = err.to_string();
    assert!(
        msg.contains("22012") || msg.to_lowercase().contains("division by zero") || msg.to_lowercase().contains("zero"),
        "Division by zero should produce error code 22012 or descriptive message, got: {}", msg
    );
}

#[test]
fn test_int8_large_value() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 9223372036854775807::BIGINT");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int8(v)) => assert_eq!(*v, 9223372036854775807i64),
        other => panic!("Expected INT8 max, got {:?}", other),
    }
}

#[test]
fn test_int8_arithmetic() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT 1000000000::BIGINT * 2");
    assert_eq!(n, 2000000000);
}

#[test]
fn test_int4_to_int8_cast() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 42::BIGINT");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int8(v)) => assert_eq!(*v, 42),
        Some(Value::Int4(v)) => assert_eq!(*v, 42),
        other => panic!("Expected 42, got {:?}", other),
    }
}

#[test]
fn test_int_null_arithmetic() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT NULL + 1");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        other => panic!("NULL + 1 should be NULL, got {:?}", other),
    }
}

#[test]
fn test_int_null_multiply() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 5 * NULL");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        other => panic!("5 * NULL should be NULL, got {:?}", other),
    }
}

#[test]
fn test_int_in_where_clause() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE numbers (n INT)");
    exec(&engine, "INSERT INTO numbers VALUES (1), (2), (3), (4), (5), (6), (7), (8), (9), (10)");
    let result = exec(&engine, "SELECT n FROM numbers WHERE n > 7 ORDER BY n");
    assert_eq!(result.rows.len(), 3);
    let vals: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(vals, vec![8, 9, 10]);
}

#[test]
fn test_int_order_by() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE unsorted (n INT)");
    exec(&engine, "INSERT INTO unsorted VALUES (5), (3), (8), (1), (4)");
    let result = exec(&engine, "SELECT n FROM unsorted ORDER BY n ASC");
    let vals: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(vals, vec![1, 3, 4, 5, 8]);
}

#[test]
fn test_int_group_by() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE group_nums (n INT)");
    exec(&engine, "INSERT INTO group_nums VALUES (1), (2), (1), (3), (2), (1)");
    let result = exec(&engine, "SELECT n, COUNT(*) AS cnt FROM group_nums GROUP BY n ORDER BY n");
    assert_eq!(result.rows.len(), 3);
    match result.rows[0].get_by_idx(1) {
        Some(Value::Int4(3)) | Some(Value::Int8(3)) => {}
        other => panic!("n=1 appears 3 times, got {:?}", other),
    }
}

#[test]
fn test_int_ten_additions() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1");
    assert_eq!(n, 10);
}

#[test]
fn test_int4_false_comparison() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 1000 < 999");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(false)) => {}
        other => panic!("1000 < 999 should be FALSE, got {:?}", other),
    }
}

#[test]
fn test_int_between() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE range_test (n INT)");
    for i in 1..=10 { exec(&engine, &format!("INSERT INTO range_test VALUES ({})", i)); }
    let result = exec(&engine, "SELECT n FROM range_test WHERE n BETWEEN 4 AND 7 ORDER BY n");
    assert_eq!(result.rows.len(), 4);
    let vals: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(vals, vec![4, 5, 6, 7]);
}
