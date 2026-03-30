/// Timestamp type tests
/// Based on PostgreSQL timestamp.sql patterns.
use tempfile::TempDir;
use crate::common::{make_engine, exec, count_rows, query_int, Backend};
use sql::Value;

fn setup_ts_tbl(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE ts_tbl (id INT, ts TIMESTAMP)");
    exec(b, "INSERT INTO ts_tbl VALUES (1, '2024-01-15 10:30:00')");
    exec(b, "INSERT INTO ts_tbl VALUES (2, '2023-12-31 23:59:59')");
    exec(b, "INSERT INTO ts_tbl VALUES (3, '2000-01-01 00:00:00')");
    exec(b, "INSERT INTO ts_tbl VALUES (4, '1990-06-15 12:00:00')");
    exec(b, "INSERT INTO ts_tbl VALUES (5, '2024-06-30 08:45:30')");
}

fn test_timestamp_insert_retrieve_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE tstbl (ts TIMESTAMP)");
    exec(b, "INSERT INTO tstbl VALUES ('2024-01-15 10:30:00')");
    let result = exec(b, "SELECT ts FROM tstbl");
    assert_eq!(result.rows.len(), 1);
    assert!(result.rows[0].get_by_idx(0).is_some(), "Should retrieve timestamp");
}

#[test]
fn test_timestamp_insert_retrieve() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_timestamp_insert_retrieve_body(&b);
}

crate::net_tests!(test_timestamp_insert_retrieve);


fn test_timestamp_multiple_rows_body(b: &crate::common::Backend) {
    setup_ts_tbl(b);
    let n = count_rows(b, "SELECT * FROM ts_tbl");
    assert_eq!(n, 5);
}

#[test]
fn test_timestamp_multiple_rows() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_timestamp_multiple_rows_body(&b);
}

crate::net_tests!(test_timestamp_multiple_rows);


fn test_timestamp_comparison_less_than_body(b: &crate::common::Backend) {
    setup_ts_tbl(b);
    let result = exec(b, "SELECT id FROM ts_tbl WHERE ts < '2000-01-01 00:00:00' ORDER BY id");
    assert_eq!(result.rows.len(), 1, "Only 1990 timestamp is before 2000");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(4)) => {}
        other => panic!("Expected id=4, got {:?}", other),
    }
}

#[test]
fn test_timestamp_comparison_less_than() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_timestamp_comparison_less_than_body(&b);
}

crate::net_tests!(test_timestamp_comparison_less_than);


fn test_timestamp_comparison_greater_than_body(b: &crate::common::Backend) {
    setup_ts_tbl(b);
    let result = exec(b, "SELECT id FROM ts_tbl WHERE ts > '2024-01-01 00:00:00' ORDER BY id");
    assert!(result.rows.len() >= 2, "2024-01-15 and 2024-06-30 are after 2024-01-01");
}

#[test]
fn test_timestamp_comparison_greater_than() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_timestamp_comparison_greater_than_body(&b);
}

crate::net_tests!(test_timestamp_comparison_greater_than);


fn test_timestamp_order_by_asc_body(b: &crate::common::Backend) {
    setup_ts_tbl(b);
    let result = exec(b, "SELECT id FROM ts_tbl ORDER BY ts ASC");
    assert_eq!(result.rows.len(), 5);
    // First should be 1990 (id=4)
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(4)) => {}
        other => panic!("First in ASC order should be id=4 (1990), got {:?}", other),
    }
}

#[test]
fn test_timestamp_order_by_asc() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_timestamp_order_by_asc_body(&b);
}

crate::net_tests!(test_timestamp_order_by_asc);


fn test_timestamp_order_by_desc_body(b: &crate::common::Backend) {
    setup_ts_tbl(b);
    let result = exec(b, "SELECT id FROM ts_tbl ORDER BY ts DESC");
    assert_eq!(result.rows.len(), 5);
    // First should be 2024-06-30 (id=5)
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(5)) => {}
        other => panic!("First in DESC order should be id=5 (2024-06-30), got {:?}", other),
    }
}

#[test]
fn test_timestamp_order_by_desc() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_timestamp_order_by_desc_body(&b);
}

crate::net_tests!(test_timestamp_order_by_desc);


fn test_timestamp_min_body(b: &crate::common::Backend) {
    setup_ts_tbl(b);
    let result = exec(b, "SELECT MIN(ts) FROM ts_tbl");
    assert_eq!(result.rows.len(), 1);
    assert!(result.rows[0].get_by_idx(0).is_some(), "MIN timestamp should exist");
}

#[test]
fn test_timestamp_min() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_timestamp_min_body(&b);
}

crate::net_tests!(test_timestamp_min);


fn test_timestamp_max_body(b: &crate::common::Backend) {
    setup_ts_tbl(b);
    let result = exec(b, "SELECT MAX(ts) FROM ts_tbl");
    assert_eq!(result.rows.len(), 1);
    assert!(result.rows[0].get_by_idx(0).is_some(), "MAX timestamp should exist");
}

#[test]
fn test_timestamp_max() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_timestamp_max_body(&b);
}

crate::net_tests!(test_timestamp_max);


fn test_timestamp_null_handling_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE ts_nullable (id INT, ts TIMESTAMP)");
    exec(b, "INSERT INTO ts_nullable VALUES (1, '2024-01-01 12:00:00')");
    exec(b, "INSERT INTO ts_nullable VALUES (2, NULL)");
    exec(b, "INSERT INTO ts_nullable VALUES (3, '2023-06-15 08:00:00')");

    let n = count_rows(b, "SELECT id FROM ts_nullable WHERE ts IS NULL");
    assert_eq!(n, 1, "One NULL timestamp");

    let n2 = count_rows(b, "SELECT id FROM ts_nullable WHERE ts IS NOT NULL");
    assert_eq!(n2, 2, "Two non-NULL timestamps");
}

#[test]
fn test_timestamp_null_handling() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_timestamp_null_handling_body(&b);
}

crate::net_tests!(test_timestamp_null_handling);


fn test_timestamp_equality_body(b: &crate::common::Backend) {
    setup_ts_tbl(b);
    let result = exec(b, "SELECT id FROM ts_tbl WHERE ts = '2024-01-15 10:30:00'");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(1)) => {}
        other => panic!("Expected id=1, got {:?}", other),
    }
}

#[test]
fn test_timestamp_equality() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_timestamp_equality_body(&b);
}

crate::net_tests!(test_timestamp_equality);


fn test_timestamp_count_body(b: &crate::common::Backend) {
    setup_ts_tbl(b);
    let n = query_int(b, "SELECT COUNT(*) FROM ts_tbl");
    assert_eq!(n, 5);
}

#[test]
fn test_timestamp_count() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_timestamp_count_body(&b);
}

crate::net_tests!(test_timestamp_count);


fn test_timestamp_between_body(b: &crate::common::Backend) {
    setup_ts_tbl(b);
    let result = exec(b,
        "SELECT id FROM ts_tbl WHERE ts BETWEEN '2023-01-01 00:00:00' AND '2024-12-31 23:59:59' ORDER BY id");
    assert!(result.rows.len() >= 3, "2023 and 2024 timestamps should be in range");
}

#[test]
fn test_timestamp_between() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_timestamp_between_body(&b);
}

crate::net_tests!(test_timestamp_between);


fn test_timestamp_midnight_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE ts_midnight (ts TIMESTAMP)");
    exec(b, "INSERT INTO ts_midnight VALUES ('2024-01-01 00:00:00')");
    let result = exec(b, "SELECT ts FROM ts_midnight");
    assert_eq!(result.rows.len(), 1);
}

#[test]
fn test_timestamp_midnight() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_timestamp_midnight_body(&b);
}

crate::net_tests!(test_timestamp_midnight);


fn test_timestamp_end_of_day_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE ts_eod (ts TIMESTAMP)");
    exec(b, "INSERT INTO ts_eod VALUES ('2024-12-31 23:59:59')");
    let result = exec(b, "SELECT ts FROM ts_eod");
    assert_eq!(result.rows.len(), 1);
}

#[test]
fn test_timestamp_end_of_day() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_timestamp_end_of_day_body(&b);
}

crate::net_tests!(test_timestamp_end_of_day);


fn test_timestamp_select_after_date_body(b: &crate::common::Backend) {
    setup_ts_tbl(b);
    // All timestamps after 2010-01-01
    let result = exec(b, "SELECT id FROM ts_tbl WHERE ts >= '2010-01-01 00:00:00' ORDER BY ts");
    assert!(result.rows.len() >= 3, "Timestamps from 2023 and 2024 should be included");
}

#[test]
fn test_timestamp_select_after_date() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_timestamp_select_after_date_body(&b);
}

crate::net_tests!(test_timestamp_select_after_date);

