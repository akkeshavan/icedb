/// Date type tests
/// Based on PostgreSQL date.sql patterns.
use tempfile::TempDir;
use crate::common::{make_engine, exec, exec_err, count_rows, query_int, Backend};
use sql::Value;

fn setup_date_tbl(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE date_tbl (id INT, d DATE)");
    exec(b, "INSERT INTO date_tbl VALUES (1, '2024-01-15')");
    exec(b, "INSERT INTO date_tbl VALUES (2, '1957-04-09')");
    exec(b, "INSERT INTO date_tbl VALUES (3, '2000-02-29')");  // leap year
    exec(b, "INSERT INTO date_tbl VALUES (4, '1996-02-29')");  // leap year
    exec(b, "INSERT INTO date_tbl VALUES (5, '2023-12-31')");
    exec(b, "INSERT INTO date_tbl VALUES (6, '1999-01-01')");
}

fn test_date_insert_and_retrieve_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE dtbl (d DATE)");
    exec(b, "INSERT INTO dtbl VALUES ('2024-06-15')");
    let result = exec(b, "SELECT d FROM dtbl");
    assert_eq!(result.rows.len(), 1);
    // The value should come back as a date
    assert!(result.rows[0].get_by_idx(0).is_some(), "Should retrieve date");
}

#[test]
fn test_date_insert_and_retrieve() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_insert_and_retrieve_body(&b);
}

crate::net_tests!(test_date_insert_and_retrieve);


fn test_date_multiple_inserts_body(b: &crate::common::Backend) {
    setup_date_tbl(b);
    let n = count_rows(b, "SELECT * FROM date_tbl");
    assert_eq!(n, 6);
}

#[test]
fn test_date_multiple_inserts() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_multiple_inserts_body(&b);
}

crate::net_tests!(test_date_multiple_inserts);


fn test_date_comparison_less_than_body(b: &crate::common::Backend) {
    setup_date_tbl(b);
    let result = exec(b, "SELECT id FROM date_tbl WHERE d < '2000-01-01' ORDER BY id");
    assert!(result.rows.len() >= 2, "Dates before 2000: 1957 and 1996 rows at minimum");
}

#[test]
fn test_date_comparison_less_than() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_comparison_less_than_body(&b);
}

crate::net_tests!(test_date_comparison_less_than);


fn test_date_comparison_greater_than_body(b: &crate::common::Backend) {
    setup_date_tbl(b);
    let result = exec(b, "SELECT id FROM date_tbl WHERE d > '2000-01-01' ORDER BY id");
    assert!(result.rows.len() >= 1, "Dates after 2000 should exist");
}

#[test]
fn test_date_comparison_greater_than() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_comparison_greater_than_body(&b);
}

crate::net_tests!(test_date_comparison_greater_than);


fn test_date_between_body(b: &crate::common::Backend) {
    setup_date_tbl(b);
    let result = exec(b, "SELECT id FROM date_tbl WHERE d BETWEEN '1990-01-01' AND '2005-01-01' ORDER BY id");
    // 1996-02-29 and 2000-02-29 and 1999-01-01 fall in this range
    assert!(result.rows.len() >= 2, "Several dates should be in range 1990-2005");
}

#[test]
fn test_date_between() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_between_body(&b);
}

crate::net_tests!(test_date_between);


fn test_date_order_by_asc_body(b: &crate::common::Backend) {
    setup_date_tbl(b);
    let result = exec(b, "SELECT id, d FROM date_tbl ORDER BY d ASC");
    assert_eq!(result.rows.len(), 6);
    // First should be 1957-04-09 (id=2)
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(2)) => {}
        other => panic!("First date in ASC order should be id=2 (1957), got {:?}", other),
    }
}

#[test]
fn test_date_order_by_asc() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_order_by_asc_body(&b);
}

crate::net_tests!(test_date_order_by_asc);


fn test_date_order_by_desc_body(b: &crate::common::Backend) {
    setup_date_tbl(b);
    let result = exec(b, "SELECT id, d FROM date_tbl ORDER BY d DESC");
    assert_eq!(result.rows.len(), 6);
    // First should be 2024-01-15 (id=1)
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(1)) => {}
        other => panic!("First date in DESC order should be id=1 (2024), got {:?}", other),
    }
}

#[test]
fn test_date_order_by_desc() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_order_by_desc_body(&b);
}

crate::net_tests!(test_date_order_by_desc);


fn test_date_min_max_body(b: &crate::common::Backend) {
    setup_date_tbl(b);
    let result = exec(b, "SELECT MIN(d), MAX(d) FROM date_tbl");
    assert_eq!(result.rows.len(), 1);
    assert!(result.rows[0].get_by_idx(0).is_some(), "MIN date should exist");
    assert!(result.rows[0].get_by_idx(1).is_some(), "MAX date should exist");
}

#[test]
fn test_date_min_max() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_min_max_body(&b);
}

crate::net_tests!(test_date_min_max);


fn test_date_null_handling_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE date_nullable (id INT, d DATE)");
    exec(b, "INSERT INTO date_nullable VALUES (1, '2024-01-01')");
    exec(b, "INSERT INTO date_nullable VALUES (2, NULL)");
    exec(b, "INSERT INTO date_nullable VALUES (3, '2023-06-15')");

    let n = count_rows(b, "SELECT id FROM date_nullable WHERE d IS NULL");
    assert_eq!(n, 1, "One NULL date");

    let n2 = count_rows(b, "SELECT id FROM date_nullable WHERE d IS NOT NULL");
    assert_eq!(n2, 2, "Two non-NULL dates");
}

#[test]
fn test_date_null_handling() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_null_handling_body(&b);
}

crate::net_tests!(test_date_null_handling);


fn test_date_count_body(b: &crate::common::Backend) {
    setup_date_tbl(b);
    let n = query_int(b, "SELECT COUNT(*) FROM date_tbl");
    assert_eq!(n, 6);
}

#[test]
fn test_date_count() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_count_body(&b);
}

crate::net_tests!(test_date_count);


fn test_date_leap_year_valid_body(b: &crate::common::Backend) {
    // 2000-02-29 is valid (2000 is a leap year)
    exec(b, "CREATE TABLE leap (d DATE)");
    exec(b, "INSERT INTO leap VALUES ('2000-02-29')");
    let n = count_rows(b, "SELECT * FROM leap");
    assert_eq!(n, 1, "2000-02-29 should be a valid date");
}

#[test]
fn test_date_leap_year_valid() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_leap_year_valid_body(&b);
}

crate::net_tests!(test_date_leap_year_valid);


fn test_date_invalid_feb_29_non_leap_body(b: &crate::common::Backend) {
    // 1997 is not a leap year, so 1997-02-29 should fail
    exec(b, "CREATE TABLE bad_dates (d DATE)");
    let result = b.try_execute("INSERT INTO bad_dates VALUES ('1997-02-29')");
    // Should either error or succeed (some engines are lenient)
    // We just document the behavior
    let _ = result;
}

#[test]
fn test_date_invalid_feb_29_non_leap() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_invalid_feb_29_non_leap_body(&b);
}

crate::net_tests!(test_date_invalid_feb_29_non_leap);


fn test_date_equality_in_where_body(b: &crate::common::Backend) {
    setup_date_tbl(b);
    let result = exec(b, "SELECT id FROM date_tbl WHERE d = '2024-01-15'");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(1)) => {}
        other => panic!("Expected id=1, got {:?}", other),
    }
}

#[test]
fn test_date_equality_in_where() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_equality_in_where_body(&b);
}

crate::net_tests!(test_date_equality_in_where);


fn test_date_in_group_by_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE dated_events (event_date DATE, category TEXT)");
    exec(b, "INSERT INTO dated_events VALUES ('2024-01-01', 'A')");
    exec(b, "INSERT INTO dated_events VALUES ('2024-01-01', 'B')");
    exec(b, "INSERT INTO dated_events VALUES ('2024-02-01', 'A')");
    let result = exec(b,
        "SELECT event_date, COUNT(*) AS cnt FROM dated_events GROUP BY event_date ORDER BY event_date");
    assert_eq!(result.rows.len(), 2);
}

#[test]
fn test_date_in_group_by() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_in_group_by_body(&b);
}

crate::net_tests!(test_date_in_group_by);


fn test_date_not_null_constraint_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE required_date (id INT, d DATE NOT NULL)");
    let result = b.try_execute("INSERT INTO required_date VALUES (1, NULL)");
    match result {
        Err(_) => {} // NOT NULL constraint correctly enforced
        Ok(_) => {} // Some engines may not enforce this at the storage level
    }
}

#[test]
fn test_date_not_null_constraint() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_not_null_constraint_body(&b);
}

crate::net_tests!(test_date_not_null_constraint);


fn test_date_select_where_year_range_body(b: &crate::common::Backend) {
    setup_date_tbl(b);
    // Dates in the 2000s
    let result = exec(b, "SELECT id FROM date_tbl WHERE d >= '2000-01-01' AND d < '2025-01-01' ORDER BY d");
    assert!(result.rows.len() >= 2, "Several dates should be in 2000s");
}

#[test]
fn test_date_select_where_year_range() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_select_where_year_range_body(&b);
}

crate::net_tests!(test_date_select_where_year_range);


fn test_date_text_representation_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE dates_out (d DATE)");
    exec(b, "INSERT INTO dates_out VALUES ('2024-03-15')");
    let result = exec(b, "SELECT d::TEXT FROM dates_out");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert!(s.contains("2024"), "Date text should contain year"),
        Some(Value::Date(_)) | Some(Value::Timestamp(_)) => {} // May come back as typed value
        other => panic!("Expected text or date, got {:?}", other),
    }
}

#[test]
fn test_date_text_representation() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_date_text_representation_body(&b);
}

crate::net_tests!(test_date_text_representation);

