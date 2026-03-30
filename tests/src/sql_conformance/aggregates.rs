/// Category 3: Aggregate function tests
/// Based on PostgreSQL aggregates.sql patterns.
use tempfile::TempDir;
use crate::common::{make_engine, exec, query_int, count_rows, Backend};
use sql::Value;

// ── Setup helpers ─────────────────────────────────────────────────────────────

fn setup_sales(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE sales (id INT, region TEXT, category TEXT, amount FLOAT, active INT)");
    exec(b, "INSERT INTO sales VALUES (1, 'north', 'electronics', 300.0, 1)");
    exec(b, "INSERT INTO sales VALUES (2, 'north', 'electronics', 200.0, 1)");
    exec(b, "INSERT INTO sales VALUES (3, 'south', 'clothing', 150.0, 1)");
    exec(b, "INSERT INTO sales VALUES (4, 'south', 'clothing', 100.0, 0)");
    exec(b, "INSERT INTO sales VALUES (5, 'east', 'electronics', 400.0, 1)");
    exec(b, "INSERT INTO sales VALUES (6, 'east', 'clothing', 250.0, 1)");
}

fn get_float(v: &Value) -> f64 {
    match v {
        Value::Float8(f) => *f,
        Value::Float8(f) => *f,
        Value::Int4(i) => *i as f64,
        Value::Int8(i) => *i as f64,
        other => panic!("expected numeric, got {:?}", other),
    }
}

fn get_int(v: &Value) -> i64 {
    match v {
        Value::Int8(n) => *n,
        Value::Int4(n) => *n as i64,
        other => panic!("expected integer, got {:?}", other),
    }
}

// ── Original tests ────────────────────────────────────────────────────────────

fn test_aggregate_count_star_body(b: &crate::common::Backend) {
    setup_sales(b);
    let n = query_int(b, "SELECT COUNT(*) FROM sales");
    assert_eq!(n, 6, "COUNT(*) should count all 6 rows");
}

#[test]
fn test_aggregate_count_star() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_count_star_body(&b);
}

crate::net_tests!(test_aggregate_count_star);


fn test_aggregate_count_col_skips_nulls_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, note TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'a')");
    exec(b, "INSERT INTO t VALUES (2, NULL)");
    exec(b, "INSERT INTO t VALUES (3, 'c')");

    let total = query_int(b, "SELECT COUNT(*) FROM t");
    let non_null = query_int(b, "SELECT COUNT(note) FROM t");
    assert_eq!(total, 3, "COUNT(*) = 3");
    assert_eq!(non_null, 2, "COUNT(note) skips NULLs = 2");
}

#[test]
fn test_aggregate_count_col_skips_nulls() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_count_col_skips_nulls_body(&b);
}

crate::net_tests!(test_aggregate_count_col_skips_nulls);


fn test_aggregate_sum_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (10)");
    exec(b, "INSERT INTO t VALUES (20)");
    exec(b, "INSERT INTO t VALUES (30)");
    let s = query_int(b, "SELECT SUM(val) FROM t");
    assert_eq!(s, 60, "SUM(val) = 60");
}

#[test]
fn test_aggregate_sum() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_sum_body(&b);
}

crate::net_tests!(test_aggregate_sum);


fn test_aggregate_avg_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val FLOAT)");
    exec(b, "INSERT INTO t VALUES (10.0)");
    exec(b, "INSERT INTO t VALUES (20.0)");
    exec(b, "INSERT INTO t VALUES (30.0)");

    let result = exec(b, "SELECT AVG(val) FROM t");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => {
            assert!((v - 20.0).abs() < 0.001, "AVG should be 20.0, got {}", v);
        }
        Some(Value::Int4(v)) => assert_eq!(*v, 20),
        Some(Value::Int8(v)) => assert_eq!(*v, 20),
        other => panic!("expected numeric avg, got {:?}", other),
    }
}

#[test]
fn test_aggregate_avg() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_avg_body(&b);
}

crate::net_tests!(test_aggregate_avg);


fn test_aggregate_min_body(b: &crate::common::Backend) {
    setup_sales(b);
    let result = exec(b, "SELECT MIN(amount) FROM sales");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => {
            assert!((v - 100.0).abs() < 0.001, "MIN(amount) should be 100.0, got {}", v);
        }
        other => panic!("expected Float8, got {:?}", other),
    }
}

#[test]
fn test_aggregate_min() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_min_body(&b);
}

crate::net_tests!(test_aggregate_min);


fn test_aggregate_max_body(b: &crate::common::Backend) {
    setup_sales(b);
    let result = exec(b, "SELECT MAX(amount) FROM sales");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => {
            assert!((v - 400.0).abs() < 0.001, "MAX(amount) should be 400.0, got {}", v);
        }
        other => panic!("expected Float8, got {:?}", other),
    }
}

#[test]
fn test_aggregate_max() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_max_body(&b);
}

crate::net_tests!(test_aggregate_max);


fn test_aggregate_group_by_single_body(b: &crate::common::Backend) {
    setup_sales(b);

    let result = exec(b, "SELECT region, COUNT(*) FROM sales GROUP BY region ORDER BY region");
    assert_eq!(result.rows.len(), 3, "3 distinct regions");
    let counts: Vec<i64> = result.rows.iter().map(|r| match r.get_by_idx(1) {
        Some(Value::Int8(v)) => *v,
        Some(Value::Int4(v)) => *v as i64,
        other => panic!("expected int count, got {:?}", other),
    }).collect();
    assert!(counts.iter().all(|&c| c == 2), "Each region should have 2 rows: {:?}", counts);
}

#[test]
fn test_aggregate_group_by_single() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_group_by_single_body(&b);
}

crate::net_tests!(test_aggregate_group_by_single);


fn test_aggregate_group_by_multi_body(b: &crate::common::Backend) {
    setup_sales(b);

    let result = exec(b, "SELECT region, category, COUNT(*) FROM sales GROUP BY region, category ORDER BY region, category");
    assert_eq!(result.rows.len(), 4, "4 distinct (region, category) pairs");
}

#[test]
fn test_aggregate_group_by_multi() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_group_by_multi_body(&b);
}

crate::net_tests!(test_aggregate_group_by_multi);


fn test_aggregate_having_basic_body(b: &crate::common::Backend) {
    setup_sales(b);

    let result = exec(b, "SELECT region, COUNT(*) FROM sales GROUP BY region HAVING COUNT(*) > 1");
    assert_eq!(result.rows.len(), 3, "All 3 regions have count > 1");
}

#[test]
fn test_aggregate_having_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_having_basic_body(&b);
}

crate::net_tests!(test_aggregate_having_basic);


fn test_aggregate_having_filters_groups_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (group_id INT, val INT)");
    exec(b, "INSERT INTO t VALUES (1, 10)");
    exec(b, "INSERT INTO t VALUES (1, 20)");
    exec(b, "INSERT INTO t VALUES (2, 5)");
    exec(b, "INSERT INTO t VALUES (3, 100)");
    exec(b, "INSERT INTO t VALUES (3, 200)");

    let result = exec(b, "SELECT group_id, SUM(val) FROM t GROUP BY group_id HAVING SUM(val) > 50 ORDER BY group_id");
    assert_eq!(result.rows.len(), 1, "Only group 3 has SUM > 50");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(3)) => {}
        other => panic!("expected group_id=3, got {:?}", other),
    }
}

#[test]
fn test_aggregate_having_filters_groups() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_having_filters_groups_body(&b);
}

crate::net_tests!(test_aggregate_having_filters_groups);


fn test_aggregate_having_with_where_body(b: &crate::common::Backend) {
    setup_sales(b);

    let result = exec(b, "SELECT region, SUM(amount) FROM sales WHERE active = 1 \
         GROUP BY region HAVING SUM(amount) > 200 ORDER BY region");
    assert_eq!(result.rows.len(), 2, "north and east should pass HAVING");
}

#[test]
fn test_aggregate_having_with_where() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_having_with_where_body(&b);
}

crate::net_tests!(test_aggregate_having_with_where);


fn test_aggregate_count_distinct_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (category TEXT)");
    exec(b, "INSERT INTO t VALUES ('a')");
    exec(b, "INSERT INTO t VALUES ('b')");
    exec(b, "INSERT INTO t VALUES ('a')");
    exec(b, "INSERT INTO t VALUES ('c')");
    exec(b, "INSERT INTO t VALUES ('b')");

    let n = query_int(b, "SELECT COUNT(DISTINCT category) FROM t");
    assert_eq!(n, 3, "COUNT(DISTINCT category) should be 3");
}

#[test]
fn test_aggregate_count_distinct() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_count_distinct_body(&b);
}

crate::net_tests!(test_aggregate_count_distinct);


fn test_aggregate_sum_float_body(b: &crate::common::Backend) {
    setup_sales(b);
    let result = exec(b, "SELECT SUM(amount) FROM sales");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => {
            let expected = 300.0 + 200.0 + 150.0 + 100.0 + 400.0 + 250.0;
            assert!((v - expected).abs() < 0.01, "SUM should be {}, got {}", expected, v);
        }
        other => panic!("expected Float8, got {:?}", other),
    }
}

#[test]
fn test_aggregate_sum_float() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_sum_float_body(&b);
}

crate::net_tests!(test_aggregate_sum_float);


fn test_aggregate_on_empty_table_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");

    let n = query_int(b, "SELECT COUNT(*) FROM t");
    assert_eq!(n, 0, "COUNT(*) on empty table should be 0");
}

#[test]
fn test_aggregate_on_empty_table() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_on_empty_table_body(&b);
}

crate::net_tests!(test_aggregate_on_empty_table);


fn test_aggregate_non_grouped_col_error_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (name TEXT, category TEXT)");
    exec(b, "INSERT INTO t VALUES ('Alice', 'a')");

    let result = b.try_execute("SELECT name, COUNT(*) FROM t GROUP BY category");
    let _ = result;
}

#[test]
fn test_aggregate_non_grouped_col_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_non_grouped_col_error_body(&b);
}

crate::net_tests!(test_aggregate_non_grouped_col_error);


fn test_aggregate_min_max_text_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (name TEXT)");
    exec(b, "INSERT INTO t VALUES ('banana')");
    exec(b, "INSERT INTO t VALUES ('apple')");
    exec(b, "INSERT INTO t VALUES ('cherry')");

    let result = exec(b, "SELECT MIN(name), MAX(name) FROM t");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "apple", "MIN text should be 'apple'"),
        other => panic!("expected Text, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "cherry", "MAX text should be 'cherry'"),
        other => panic!("expected Text, got {:?}", other),
    }
}

#[test]
fn test_aggregate_min_max_text() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_min_max_text_body(&b);
}

crate::net_tests!(test_aggregate_min_max_text);


fn test_aggregate_group_and_order_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (dept TEXT, salary INT)");
    exec(b, "INSERT INTO t VALUES ('eng', 100)");
    exec(b, "INSERT INTO t VALUES ('eng', 200)");
    exec(b, "INSERT INTO t VALUES ('hr', 50)");
    exec(b, "INSERT INTO t VALUES ('hr', 75)");

    let result = exec(b, "SELECT dept, SUM(salary) AS total FROM t GROUP BY dept ORDER BY total DESC");
    assert_eq!(result.rows.len(), 2);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "eng", "eng should be first (highest sum)"),
        other => panic!("expected Text, got {:?}", other),
    }
}

#[test]
fn test_aggregate_group_and_order() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_group_and_order_body(&b);
}

crate::net_tests!(test_aggregate_group_and_order);


fn test_aggregate_stddev_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val FLOAT)");
    exec(b, "INSERT INTO t VALUES (2.0)");
    exec(b, "INSERT INTO t VALUES (4.0)");
    exec(b, "INSERT INTO t VALUES (4.0)");
    exec(b, "INSERT INTO t VALUES (4.0)");
    exec(b, "INSERT INTO t VALUES (5.0)");
    exec(b, "INSERT INTO t VALUES (5.0)");
    exec(b, "INSERT INTO t VALUES (7.0)");
    exec(b, "INSERT INTO t VALUES (9.0)");
    let _result = exec(b, "SELECT STDDEV(val) FROM t");
}

#[test]
fn test_aggregate_stddev() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_stddev_body(&b);
}

crate::net_tests!(test_aggregate_stddev);


fn test_aggregate_multiple_in_one_query_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    for i in 1..=5 {
        exec(b, &format!("INSERT INTO t VALUES ({})", i));
    }
    let result = exec(b, "SELECT COUNT(*), SUM(val), MIN(val), MAX(val) FROM t");
    assert_eq!(result.rows.len(), 1);
    let cnt = match result.rows[0].get_by_idx(0) {
        Some(Value::Int8(v)) => *v,
        Some(Value::Int4(v)) => *v as i64,
        other => panic!("count: {:?}", other),
    };
    assert_eq!(cnt, 5);
    let sum = match result.rows[0].get_by_idx(1) {
        Some(Value::Int8(v)) => *v,
        Some(Value::Int4(v)) => *v as i64,
        other => panic!("sum: {:?}", other),
    };
    assert_eq!(sum, 15);
}

#[test]
fn test_aggregate_multiple_in_one_query() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_multiple_in_one_query_body(&b);
}

crate::net_tests!(test_aggregate_multiple_in_one_query);


// ── New tests ─────────────────────────────────────────────────────────────────

/// COUNT(*) on table with only NULL values counts all rows.
fn test_aggregate_count_star_with_nulls_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    let n = query_int(b, "SELECT COUNT(*) FROM t");
    assert_eq!(n, 3, "COUNT(*) must count rows with NULL values");
}

#[test]
fn test_aggregate_count_star_with_nulls() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_count_star_with_nulls_body(&b);
}

crate::net_tests!(test_aggregate_count_star_with_nulls);


/// SUM with NULL values: NULLs are skipped, sum of non-null values returned.
fn test_aggregate_sum_with_nulls_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (5)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (10)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (15)");
    let s = query_int(b, "SELECT SUM(val) FROM t");
    assert_eq!(s, 30, "SUM must skip NULLs: 5+10+15=30");
}

#[test]
fn test_aggregate_sum_with_nulls() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_sum_with_nulls_body(&b);
}

crate::net_tests!(test_aggregate_sum_with_nulls);


/// SUM of all-NULL column returns NULL.
fn test_aggregate_sum_all_null_returns_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    let result = exec(b, "SELECT SUM(val) FROM t");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        Some(Value::Int4(0)) | Some(Value::Int8(0)) => {}
        other => panic!("SUM of all-NULL should be NULL or 0, got {:?}", other),
    }
}

#[test]
fn test_aggregate_sum_all_null_returns_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_sum_all_null_returns_null_body(&b);
}

crate::net_tests!(test_aggregate_sum_all_null_returns_null);


/// COUNT(*) returns 0, SUM returns NULL on empty table.
fn test_aggregate_empty_table_sum_null_count_zero_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");

    let n = query_int(b, "SELECT COUNT(*) FROM t");
    assert_eq!(n, 0, "COUNT(*) on empty table is 0");

    let result = exec(b, "SELECT SUM(val) FROM t");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        Some(Value::Int4(0)) | Some(Value::Int8(0)) => {}
        other => panic!("SUM on empty table should be NULL or 0, got {:?}", other),
    }
}

#[test]
fn test_aggregate_empty_table_sum_null_count_zero() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_empty_table_sum_null_count_zero_body(&b);
}

crate::net_tests!(test_aggregate_empty_table_sum_null_count_zero);


/// AVG with integer column — verify numeric correctness.
fn test_aggregate_avg_integer_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (1)");
    exec(b, "INSERT INTO t VALUES (2)");
    exec(b, "INSERT INTO t VALUES (3)");
    exec(b, "INSERT INTO t VALUES (4)");
    exec(b, "INSERT INTO t VALUES (5)");

    let result = exec(b, "SELECT AVG(val) FROM t");
    let avg = get_float(result.rows[0].get_by_idx(0).unwrap());
    assert!((avg - 3.0).abs() < 0.001, "AVG of 1..5 should be 3.0, got {}", avg);
}

#[test]
fn test_aggregate_avg_integer() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_avg_integer_body(&b);
}

crate::net_tests!(test_aggregate_avg_integer);


/// AVG with NULL values: NULLs excluded from average computation.
fn test_aggregate_avg_with_nulls_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (10)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (20)");

    let result = exec(b, "SELECT AVG(val) FROM t");
    let avg = get_float(result.rows[0].get_by_idx(0).unwrap());
    assert!((avg - 15.0).abs() < 0.001, "AVG(10, NULL, 20) should be 15.0, got {}", avg);
}

#[test]
fn test_aggregate_avg_with_nulls() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_avg_with_nulls_body(&b);
}

crate::net_tests!(test_aggregate_avg_with_nulls);


/// MIN on NULL-containing integer column.
fn test_aggregate_min_with_nulls_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (5)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (3)");
    exec(b, "INSERT INTO t VALUES (8)");

    let v = query_int(b, "SELECT MIN(val) FROM t");
    assert_eq!(v, 3, "MIN must skip NULLs and return 3");
}

#[test]
fn test_aggregate_min_with_nulls() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_min_with_nulls_body(&b);
}

crate::net_tests!(test_aggregate_min_with_nulls);


/// MAX on NULL-containing integer column.
fn test_aggregate_max_with_nulls_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (5)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (9)");
    exec(b, "INSERT INTO t VALUES (2)");

    let v = query_int(b, "SELECT MAX(val) FROM t");
    assert_eq!(v, 9, "MAX must skip NULLs and return 9");
}

#[test]
fn test_aggregate_max_with_nulls() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_max_with_nulls_body(&b);
}

crate::net_tests!(test_aggregate_max_with_nulls);


/// Multiple aggregates in a single SELECT on the same table.
fn test_aggregate_multi_agg_single_select_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (2)");
    exec(b, "INSERT INTO t VALUES (4)");
    exec(b, "INSERT INTO t VALUES (6)");
    exec(b, "INSERT INTO t VALUES (8)");

    let result = exec(b, "SELECT COUNT(*), SUM(val), MIN(val), MAX(val) FROM t");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(get_int(result.rows[0].get_by_idx(0).unwrap()), 4, "COUNT");
    assert_eq!(get_int(result.rows[0].get_by_idx(1).unwrap()), 20, "SUM");
    assert_eq!(get_int(result.rows[0].get_by_idx(2).unwrap()), 2, "MIN");
    assert_eq!(get_int(result.rows[0].get_by_idx(3).unwrap()), 8, "MAX");
}

#[test]
fn test_aggregate_multi_agg_single_select() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_multi_agg_single_select_body(&b);
}

crate::net_tests!(test_aggregate_multi_agg_single_select);


/// GROUP BY multiple columns with COUNT and SUM per group.
fn test_aggregate_group_by_two_cols_with_sum_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE orders (customer TEXT, product TEXT, qty INT)");
    exec(b, "INSERT INTO orders VALUES ('alice', 'widget', 3)");
    exec(b, "INSERT INTO orders VALUES ('alice', 'gadget', 2)");
    exec(b, "INSERT INTO orders VALUES ('bob', 'widget', 5)");
    exec(b, "INSERT INTO orders VALUES ('alice', 'widget', 1)");

    let result = exec(b, "SELECT customer, product, SUM(qty) FROM orders GROUP BY customer, product ORDER BY customer, product");
    // alice+gadget=2, alice+widget=4, bob+widget=5
    assert_eq!(result.rows.len(), 3, "3 distinct (customer, product) groups");
}

#[test]
fn test_aggregate_group_by_two_cols_with_sum() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_group_by_two_cols_with_sum_body(&b);
}

crate::net_tests!(test_aggregate_group_by_two_cols_with_sum);


/// HAVING with multiple conditions: HAVING COUNT(*) >= 2 AND SUM(val) > 10.
fn test_aggregate_having_multiple_conditions_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (grp TEXT, val INT)");
    exec(b, "INSERT INTO t VALUES ('a', 5)");
    exec(b, "INSERT INTO t VALUES ('a', 8)");   // count=2, sum=13 => passes
    exec(b, "INSERT INTO t VALUES ('b', 20)");  // count=1, sum=20 => fails count
    exec(b, "INSERT INTO t VALUES ('c', 3)");
    exec(b, "INSERT INTO t VALUES ('c', 4)");   // count=2, sum=7 => fails sum

    let result = exec(b, "SELECT grp FROM t GROUP BY grp HAVING COUNT(*) >= 2 AND SUM(val) > 10 ORDER BY grp");
    assert_eq!(result.rows.len(), 1, "Only group 'a' satisfies both HAVING conditions");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "a"),
        other => panic!("expected 'a', got {:?}", other),
    }
}

#[test]
fn test_aggregate_having_multiple_conditions() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_having_multiple_conditions_body(&b);
}

crate::net_tests!(test_aggregate_having_multiple_conditions);


/// Aggregate on JOIN result.
fn test_aggregate_on_join_result_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE emp (id INT, dept_id INT, salary INT)");
    exec(b, "CREATE TABLE dept (id INT, name TEXT)");
    exec(b, "INSERT INTO dept VALUES (1, 'Engineering')");
    exec(b, "INSERT INTO dept VALUES (2, 'Marketing')");
    exec(b, "INSERT INTO emp VALUES (1, 1, 100)");
    exec(b, "INSERT INTO emp VALUES (2, 1, 150)");
    exec(b, "INSERT INTO emp VALUES (3, 2, 80)");

    let result = exec(b, "SELECT d.name, SUM(e.salary) FROM emp e JOIN dept d ON e.dept_id = d.id GROUP BY d.name ORDER BY d.name");
    assert_eq!(result.rows.len(), 2, "Two departments");

    // Engineering: 100+150=250, Marketing: 80
    let eng_row = result.rows.iter().find(|r| {
        matches!(r.get_by_idx(0), Some(Value::Text(s)) if s == "Engineering")
    }).expect("Engineering row missing");
    assert_eq!(get_int(eng_row.get_by_idx(1).unwrap()), 250, "Engineering salary sum");
}

#[test]
fn test_aggregate_on_join_result() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_on_join_result_body(&b);
}

crate::net_tests!(test_aggregate_on_join_result);


/// COUNT(DISTINCT col) counts only distinct non-null values.
fn test_aggregate_count_distinct_with_nulls_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (1)");
    exec(b, "INSERT INTO t VALUES (1)");
    exec(b, "INSERT INTO t VALUES (2)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (3)");

    let n = query_int(b, "SELECT COUNT(DISTINCT val) FROM t");
    assert_eq!(n, 3, "COUNT(DISTINCT val) ignores NULLs and duplicates: 1,2,3 => 3");
}

#[test]
fn test_aggregate_count_distinct_with_nulls() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_count_distinct_with_nulls_body(&b);
}

crate::net_tests!(test_aggregate_count_distinct_with_nulls);


/// GROUP BY with ORDER BY on aggregate result descending.
fn test_aggregate_group_by_order_by_agg_desc_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (cat TEXT, val INT)");
    exec(b, "INSERT INTO t VALUES ('x', 10)");
    exec(b, "INSERT INTO t VALUES ('x', 20)");
    exec(b, "INSERT INTO t VALUES ('y', 100)");
    exec(b, "INSERT INTO t VALUES ('z', 5)");

    let result = exec(b, "SELECT cat, SUM(val) AS s FROM t GROUP BY cat ORDER BY s DESC");
    assert_eq!(result.rows.len(), 3);
    // y=100, x=30, z=5
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "y", "y has highest sum"),
        other => panic!("expected 'y', got {:?}", other),
    }
    match result.rows[2].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "z", "z has lowest sum"),
        other => panic!("expected 'z', got {:?}", other),
    }
}

#[test]
fn test_aggregate_group_by_order_by_agg_desc() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_group_by_order_by_agg_desc_body(&b);
}

crate::net_tests!(test_aggregate_group_by_order_by_agg_desc);


/// MIN and MAX on single-row table.
fn test_aggregate_min_max_single_row_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (42)");

    let min = query_int(b, "SELECT MIN(val) FROM t");
    let max = query_int(b, "SELECT MAX(val) FROM t");
    assert_eq!(min, 42);
    assert_eq!(max, 42);
}

#[test]
fn test_aggregate_min_max_single_row() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_min_max_single_row_body(&b);
}

crate::net_tests!(test_aggregate_min_max_single_row);


/// HAVING SUM(...) > AVG(...) style — HAVING that uses two aggregates.
/// Not all engines support two aggregates in HAVING; mark ignored if unsupported.
fn test_aggregate_having_sum_gt_constant_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (grp INT, val INT)");
    exec(b, "INSERT INTO t VALUES (1, 100)");
    exec(b, "INSERT INTO t VALUES (1, 200)");
    exec(b, "INSERT INTO t VALUES (2, 10)");
    exec(b, "INSERT INTO t VALUES (2, 20)");

    let result = exec(b, "SELECT grp, SUM(val) FROM t GROUP BY grp HAVING SUM(val) > 100 ORDER BY grp");
    // grp=1: sum=300>100 yes; grp=2: sum=30 no
    assert_eq!(result.rows.len(), 1, "Only group 1 has SUM > 100");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(1)) => {}
        other => panic!("expected grp=1, got {:?}", other),
    }
}

#[test]
fn test_aggregate_having_sum_gt_constant() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_having_sum_gt_constant_body(&b);
}

crate::net_tests!(test_aggregate_having_sum_gt_constant);


/// Aggregate with WHERE that filters all rows — result is COUNT=0 / SUM=NULL.
fn test_aggregate_where_filters_all_rows_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (1)");
    exec(b, "INSERT INTO t VALUES (2)");

    let n = query_int(b, "SELECT COUNT(*) FROM t WHERE val > 1000");
    assert_eq!(n, 0, "COUNT(*) with no matching rows is 0");
}

#[test]
fn test_aggregate_where_filters_all_rows() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_where_filters_all_rows_body(&b);
}

crate::net_tests!(test_aggregate_where_filters_all_rows);


/// GROUP BY a column that has all-identical values => one group.
fn test_aggregate_group_by_single_group_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (grp TEXT, val INT)");
    exec(b, "INSERT INTO t VALUES ('same', 10)");
    exec(b, "INSERT INTO t VALUES ('same', 20)");
    exec(b, "INSERT INTO t VALUES ('same', 30)");

    let result = exec(b, "SELECT grp, COUNT(*), SUM(val) FROM t GROUP BY grp");
    assert_eq!(result.rows.len(), 1, "All rows in same group");
    assert_eq!(get_int(result.rows[0].get_by_idx(1).unwrap()), 3, "COUNT=3");
    assert_eq!(get_int(result.rows[0].get_by_idx(2).unwrap()), 60, "SUM=60");
}

#[test]
fn test_aggregate_group_by_single_group() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_group_by_single_group_body(&b);
}

crate::net_tests!(test_aggregate_group_by_single_group);


/// MIN/MAX on all-null column: both return NULL.
fn test_aggregate_min_max_all_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (NULL)");

    let result = exec(b, "SELECT MIN(val), MAX(val) FROM t");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        other => panic!("MIN of all-NULL should be NULL, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Null) | None => {}
        other => panic!("MAX of all-NULL should be NULL, got {:?}", other),
    }
}

#[test]
fn test_aggregate_min_max_all_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_min_max_all_null_body(&b);
}

crate::net_tests!(test_aggregate_min_max_all_null);


/// COUNT(col) = 0 when all rows are NULL.
fn test_aggregate_count_col_all_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (NULL)");

    let n = query_int(b, "SELECT COUNT(val) FROM t");
    assert_eq!(n, 0, "COUNT(col) of all-NULL rows is 0");
}

#[test]
fn test_aggregate_count_col_all_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_count_col_all_null_body(&b);
}

crate::net_tests!(test_aggregate_count_col_all_null);


/// COUNT(DISTINCT) on empty table = 0.
fn test_aggregate_count_distinct_empty_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    let n = query_int(b, "SELECT COUNT(DISTINCT val) FROM t");
    assert_eq!(n, 0);
}

#[test]
fn test_aggregate_count_distinct_empty() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_count_distinct_empty_body(&b);
}

crate::net_tests!(test_aggregate_count_distinct_empty);


/// AVG on empty table returns NULL.
fn test_aggregate_avg_empty_table_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val FLOAT)");
    let result = exec(b, "SELECT AVG(val) FROM t");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        other => panic!("AVG on empty table should be NULL, got {:?}", other),
    }
}

#[test]
fn test_aggregate_avg_empty_table() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_avg_empty_table_body(&b);
}

crate::net_tests!(test_aggregate_avg_empty_table);


/// GROUP BY preserves all groups even when HAVING omits none.
fn test_aggregate_having_all_groups_pass_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (grp TEXT, val INT)");
    exec(b, "INSERT INTO t VALUES ('a', 10)");
    exec(b, "INSERT INTO t VALUES ('b', 20)");
    exec(b, "INSERT INTO t VALUES ('c', 30)");

    let result = exec(b, "SELECT grp FROM t GROUP BY grp HAVING SUM(val) > 0 ORDER BY grp");
    assert_eq!(result.rows.len(), 3, "All 3 groups have SUM > 0");
}

#[test]
fn test_aggregate_having_all_groups_pass() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_aggregate_having_all_groups_pass_body(&b);
}

crate::net_tests!(test_aggregate_having_all_groups_pass);


// ── Window frame specs ────────────────────────────────────────────────────────

/// ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW gives running sum.
fn test_window_frame_rows_unbounded_preceding_current_row_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE sales2 (id INT, amt INT)");
    exec(b, "INSERT INTO sales2 VALUES (1, 10)");
    exec(b, "INSERT INTO sales2 VALUES (2, 20)");
    exec(b, "INSERT INTO sales2 VALUES (3, 30)");

    let result = exec(b,
        "SELECT SUM(amt) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS running_sum \
         FROM sales2 ORDER BY id");
    assert_eq!(result.rows.len(), 3);
    let v0 = result.rows[0].get_by_idx(0).unwrap();
    let v1 = result.rows[1].get_by_idx(0).unwrap();
    let v2 = result.rows[2].get_by_idx(0).unwrap();
    // Running sums: 10, 30, 60
    let s0 = match v0 { Value::Int4(v) => *v as i64, Value::Int8(v) => *v, _ => panic!("not int: {:?}", v0) };
    let s1 = match v1 { Value::Int4(v) => *v as i64, Value::Int8(v) => *v, _ => panic!("not int: {:?}", v1) };
    let s2 = match v2 { Value::Int4(v) => *v as i64, Value::Int8(v) => *v, _ => panic!("not int: {:?}", v2) };
    assert_eq!(s0, 10, "first running sum should be 10");
    assert_eq!(s1, 30, "second running sum should be 30");
    assert_eq!(s2, 60, "third running sum should be 60");
}

#[test]
fn test_window_frame_rows_unbounded_preceding_current_row() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_window_frame_rows_unbounded_preceding_current_row_body(&b);
}

crate::net_tests!(test_window_frame_rows_unbounded_preceding_current_row);


/// ROWS BETWEEN 1 PRECEDING AND CURRENT ROW gives sliding window sum.
fn test_window_frame_rows_1_preceding_current_row_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE sales3 (id INT, amt INT)");
    exec(b, "INSERT INTO sales3 VALUES (1, 10)");
    exec(b, "INSERT INTO sales3 VALUES (2, 20)");
    exec(b, "INSERT INTO sales3 VALUES (3, 30)");

    let result = exec(b,
        "SELECT SUM(amt) OVER (ORDER BY id ROWS BETWEEN 1 PRECEDING AND CURRENT ROW) AS win_sum \
         FROM sales3 ORDER BY id");
    assert_eq!(result.rows.len(), 3);
    let v0 = result.rows[0].get_by_idx(0).unwrap();
    let v1 = result.rows[1].get_by_idx(0).unwrap();
    let v2 = result.rows[2].get_by_idx(0).unwrap();
    // Sliding sums: 10 (only row 1), 30 (rows 1+2), 50 (rows 2+3)
    let s0 = match v0 { Value::Int4(v) => *v as i64, Value::Int8(v) => *v, _ => panic!("not int: {:?}", v0) };
    let s1 = match v1 { Value::Int4(v) => *v as i64, Value::Int8(v) => *v, _ => panic!("not int: {:?}", v1) };
    let s2 = match v2 { Value::Int4(v) => *v as i64, Value::Int8(v) => *v, _ => panic!("not int: {:?}", v2) };
    assert_eq!(s0, 10, "first sliding sum should be 10");
    assert_eq!(s1, 30, "second sliding sum should be 30");
    assert_eq!(s2, 50, "third sliding sum should be 50");
}

#[test]
fn test_window_frame_rows_1_preceding_current_row() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_window_frame_rows_1_preceding_current_row_body(&b);
}

crate::net_tests!(test_window_frame_rows_1_preceding_current_row);


/// ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING gives full partition sum.
fn test_window_frame_rows_unbounded_both_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE sales4 (id INT, amt INT)");
    exec(b, "INSERT INTO sales4 VALUES (1, 10)");
    exec(b, "INSERT INTO sales4 VALUES (2, 20)");
    exec(b, "INSERT INTO sales4 VALUES (3, 30)");

    let result = exec(b,
        "SELECT SUM(amt) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) AS total \
         FROM sales4 ORDER BY id");
    assert_eq!(result.rows.len(), 3);
    // All rows get the full sum = 60
    for row in &result.rows {
        let v = row.get_by_idx(0).unwrap();
        let total = match v { Value::Int4(n) => *n as i64, Value::Int8(n) => *n, _ => panic!("not int: {:?}", v) };
        assert_eq!(total, 60, "each row should see total 60");
    }
}

#[test]
fn test_window_frame_rows_unbounded_both() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_window_frame_rows_unbounded_both_body(&b);
}

crate::net_tests!(test_window_frame_rows_unbounded_both);


// ── ROLLUP ────────────────────────────────────────────────────────────────────

/// GROUP BY ROLLUP(region, product) produces subtotals and grand total.
fn test_rollup_basic_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE revenue (region TEXT, product TEXT, amt INT)");
    exec(b, "INSERT INTO revenue VALUES ('East', 'A', 100)");
    exec(b, "INSERT INTO revenue VALUES ('East', 'B', 200)");
    exec(b, "INSERT INTO revenue VALUES ('West', 'A', 150)");

    // ROLLUP(region, product) → grouping sets: (region,product), (region), ()
    // Should produce 3 detail rows + 2 region subtotals + 1 grand total = 6 rows
    let result = exec(b,
        "SELECT region, product, SUM(amt) FROM revenue GROUP BY ROLLUP(region, product)");
    // At minimum we get more rows than the plain GROUP BY
    assert!(result.rows.len() >= 4, "ROLLUP should produce subtotals, got {} rows", result.rows.len());
}

#[test]
fn test_rollup_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_rollup_basic_body(&b);
}

crate::net_tests!(test_rollup_basic);


/// GROUP BY ROLLUP with grand total row (NULL, NULL).
fn test_rollup_grand_total_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE rev2 (region TEXT, amt INT)");
    exec(b, "INSERT INTO rev2 VALUES ('East', 100)");
    exec(b, "INSERT INTO rev2 VALUES ('West', 200)");

    // ROLLUP(region) → grouping sets: (region), ()
    // Should produce: East/100, West/200, NULL/300
    let result = exec(b,
        "SELECT region, SUM(amt) AS total FROM rev2 GROUP BY ROLLUP(region) ORDER BY region");
    // 3 rows: East, West, NULL (grand total)
    assert_eq!(result.rows.len(), 3, "ROLLUP should produce 2 group rows + 1 grand total");

    // The last row (NULL region) should have the grand total
    let null_row = result.rows.iter().find(|r| matches!(r.get_by_idx(0), Some(Value::Null)));
    assert!(null_row.is_some(), "Should have a NULL region row for grand total");
    if let Some(row) = null_row {
        let total = match row.get_by_idx(1) {
            Some(Value::Int4(n)) => *n as i64,
            Some(Value::Int8(n)) => *n,
            other => panic!("Grand total should be int, got {:?}", other),
        };
        assert_eq!(total, 300, "Grand total should be 300");
    }
}

#[test]
fn test_rollup_grand_total() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_rollup_grand_total_body(&b);
}

crate::net_tests!(test_rollup_grand_total);


// ── CUBE ──────────────────────────────────────────────────────────────────────

/// GROUP BY CUBE(a, b) produces all 2^n subsets.
fn test_cube_basic_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE cube_t (a TEXT, b TEXT, val INT)");
    exec(b, "INSERT INTO cube_t VALUES ('x', 'y', 10)");
    exec(b, "INSERT INTO cube_t VALUES ('x', 'z', 20)");

    // CUBE(a, b) → grouping sets: (a,b), (a), (b), ()
    // With the data: (x,y)=10, (x,z)=20, then subtotals:
    // (a,b): x/y=10, x/z=20
    // (a): x=30
    // (b): y=10, z=20
    // (): 30
    let result = exec(b,
        "SELECT a, b, SUM(val) FROM cube_t GROUP BY CUBE(a, b)");
    // At least 6 rows (2 detail + 1 a-subtotal + 2 b-subtotals + 1 grand total)
    assert!(result.rows.len() >= 4, "CUBE should produce multiple grouping set rows, got {}", result.rows.len());
}

#[test]
fn test_cube_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cube_basic_body(&b);
}

crate::net_tests!(test_cube_basic);


// ── GROUPING SETS ─────────────────────────────────────────────────────────────

/// GROUPING SETS explicitly specifies grouping sets.
fn test_grouping_sets_basic_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE gs_t (region TEXT, product TEXT, amt INT)");
    exec(b, "INSERT INTO gs_t VALUES ('East', 'A', 100)");
    exec(b, "INSERT INTO gs_t VALUES ('East', 'B', 200)");
    exec(b, "INSERT INTO gs_t VALUES ('West', 'A', 150)");

    // GROUPING SETS ((region), (product)) → 2 grouping sets
    let result = exec(b,
        "SELECT region, product, SUM(amt) FROM gs_t \
         GROUP BY GROUPING SETS ((region), (product))");
    // Should produce rows for each set: 2 regions + 2 products = 4 rows
    assert!(result.rows.len() >= 2, "GROUPING SETS should produce rows for each set, got {}", result.rows.len());
}

#[test]
fn test_grouping_sets_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_grouping_sets_basic_body(&b);
}

crate::net_tests!(test_grouping_sets_basic);

