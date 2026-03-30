/// CASE expression tests
/// Based on PostgreSQL case.sql patterns.
use tempfile::TempDir;
use crate::common::{make_engine, exec, query_int, Backend};
use sql::Value;

fn setup_case_tbl(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE case_tbl (i INT, f FLOAT)");
    exec(b, "INSERT INTO case_tbl VALUES (1, 10.1)");
    exec(b, "INSERT INTO case_tbl VALUES (2, 20.2)");
    exec(b, "INSERT INTO case_tbl VALUES (3, -30.3)");
    exec(b, "INSERT INTO case_tbl VALUES (NULL, NULL)");

    exec(b, "CREATE TABLE case2_tbl (i INT, j INT)");
    exec(b, "INSERT INTO case2_tbl VALUES (1, NULL)");
    exec(b, "INSERT INTO case2_tbl VALUES (2, 6)");
    exec(b, "INSERT INTO case2_tbl VALUES (3, -6)");
    exec(b, "INSERT INTO case2_tbl VALUES (2, -6)");
    exec(b, "INSERT INTO case2_tbl VALUES (3, 6)");
}

fn test_case_simple_when_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT CASE 1 WHEN 1 THEN 'one' WHEN 2 THEN 'two' ELSE 'other' END");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "one"),
        other => panic!("Expected 'one', got {:?}", other),
    }
}

#[test]
fn test_case_simple_when() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_case_simple_when_body(&b);
}

crate::net_tests!(test_case_simple_when);


fn test_case_simple_else_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT CASE 5 WHEN 1 THEN 'one' WHEN 2 THEN 'two' ELSE 'other' END");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "other"),
        other => panic!("Expected 'other', got {:?}", other),
    }
}

#[test]
fn test_case_simple_else() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_case_simple_else_body(&b);
}

crate::net_tests!(test_case_simple_else);


fn test_case_searched_positive_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT CASE WHEN 5 > 3 THEN 'yes' ELSE 'no' END");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "yes"),
        other => panic!("Expected 'yes', got {:?}", other),
    }
}

#[test]
fn test_case_searched_positive() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_case_searched_positive_body(&b);
}

crate::net_tests!(test_case_searched_positive);


fn test_case_searched_false_branch_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT CASE WHEN 1 > 5 THEN 'yes' ELSE 'no' END");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "no"),
        other => panic!("Expected 'no', got {:?}", other),
    }
}

#[test]
fn test_case_searched_false_branch() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_case_searched_false_branch_body(&b);
}

crate::net_tests!(test_case_searched_false_branch);


fn test_case_null_result_body(b: &crate::common::Backend) {
    // CASE without ELSE returns NULL when no WHEN matches
    let result = exec(b, "SELECT CASE WHEN 1 = 2 THEN 'match' END");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        other => panic!("Expected NULL (no ELSE), got {:?}", other),
    }
}

#[test]
fn test_case_null_result() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_case_null_result_body(&b);
}

crate::net_tests!(test_case_null_result);


fn test_case_in_select_list_body(b: &crate::common::Backend) {
    setup_case_tbl(b);
    let result = exec(b,
        "SELECT i, CASE WHEN i > 0 THEN 'positive' WHEN i < 0 THEN 'negative' ELSE 'zero or null' END AS sign \
         FROM case_tbl ORDER BY i");
    assert_eq!(result.rows.len(), 4);
}

#[test]
fn test_case_in_select_list() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_case_in_select_list_body(&b);
}

crate::net_tests!(test_case_in_select_list);


fn test_case_in_where_body(b: &crate::common::Backend) {
    setup_case_tbl(b);
    let result = exec(b,
        "SELECT i FROM case_tbl WHERE CASE WHEN i > 2 THEN TRUE ELSE FALSE END = TRUE ORDER BY i");
    assert!(result.rows.len() >= 1, "At least i=3 should pass");
}

#[test]
fn test_case_in_where() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_case_in_where_body(&b);
}

crate::net_tests!(test_case_in_where);


fn test_case_in_order_by_body(b: &crate::common::Backend) {
    setup_case_tbl(b);
    // Order: negatives first, then positives, then NULL
    let result = exec(b,
        "SELECT i FROM case_tbl ORDER BY CASE WHEN i < 0 THEN 0 WHEN i >= 0 THEN 1 ELSE 2 END, i");
    assert_eq!(result.rows.len(), 4);
}

#[test]
fn test_case_in_order_by() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_case_in_order_by_body(&b);
}

crate::net_tests!(test_case_in_order_by);


fn test_case_string_categorization_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE scores (id INT, score INT)");
    exec(b, "INSERT INTO scores VALUES (1, 95), (2, 75), (3, 55), (4, 35)");
    let result = exec(b,
        "SELECT id, CASE WHEN score >= 90 THEN 'A' WHEN score >= 70 THEN 'B' WHEN score >= 50 THEN 'C' ELSE 'F' END AS grade \
         FROM scores ORDER BY id");
    assert_eq!(result.rows.len(), 4);
    let grades: Vec<String> = result.rows.iter().map(|r| match r.get_by_idx(1) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(grades, vec!["A", "B", "C", "F"]);
}

#[test]
fn test_case_string_categorization() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_case_string_categorization_body(&b);
}

crate::net_tests!(test_case_string_categorization);


fn test_case_with_null_input_body(b: &crate::common::Backend) {
    // NULL comparison: NULL = anything is NULL (not true/false)
    let result = exec(b,
        "SELECT CASE WHEN NULL = 1 THEN 'yes' ELSE 'no' END");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "no"),
        other => panic!("Expected 'no' (NULL = 1 is not true), got {:?}", other),
    }
}

#[test]
fn test_case_with_null_input() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_case_with_null_input_body(&b);
}

crate::net_tests!(test_case_with_null_input);


fn test_coalesce_first_non_null_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT COALESCE(NULL, NULL, 42, 99)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(42)) => {}
        other => panic!("COALESCE should return first non-NULL = 42, got {:?}", other),
    }
}

#[test]
fn test_coalesce_first_non_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_coalesce_first_non_null_body(&b);
}

crate::net_tests!(test_coalesce_first_non_null);


fn test_coalesce_all_null_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT COALESCE(NULL, NULL, NULL)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        other => panic!("All-NULL COALESCE should return NULL, got {:?}", other),
    }
}

#[test]
fn test_coalesce_all_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_coalesce_all_null_body(&b);
}

crate::net_tests!(test_coalesce_all_null);


fn test_coalesce_single_value_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT COALESCE(7)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(7)) => {}
        other => panic!("COALESCE(7) = 7, got {:?}", other),
    }
}

#[test]
fn test_coalesce_single_value() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_coalesce_single_value_body(&b);
}

crate::net_tests!(test_coalesce_single_value);


fn test_nullif_equal_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT NULLIF(1, 1)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        other => panic!("NULLIF(1,1) should be NULL, got {:?}", other),
    }
}

#[test]
fn test_nullif_equal() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_nullif_equal_body(&b);
}

crate::net_tests!(test_nullif_equal);


fn test_nullif_not_equal_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT NULLIF(1, 2)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(1)) => {}
        other => panic!("NULLIF(1,2) should be 1, got {:?}", other),
    }
}

#[test]
fn test_nullif_not_equal() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_nullif_not_equal_body(&b);
}

crate::net_tests!(test_nullif_not_equal);


fn test_nullif_text_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT NULLIF('hello', 'hello')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        other => panic!("NULLIF equal strings should be NULL, got {:?}", other),
    }
}

#[test]
fn test_nullif_text() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_nullif_text_body(&b);
}

crate::net_tests!(test_nullif_text);


fn test_case_nested_body(b: &crate::common::Backend) {
    let result = exec(b,
        "SELECT CASE WHEN 10 > 5 THEN CASE WHEN 3 > 1 THEN 'inner-true' ELSE 'inner-false' END ELSE 'outer-false' END");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "inner-true"),
        other => panic!("Expected 'inner-true', got {:?}", other),
    }
}

#[test]
fn test_case_nested() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_case_nested_body(&b);
}

crate::net_tests!(test_case_nested);


fn test_case_in_aggregate_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE votes (voter_id INT, candidate TEXT)");
    exec(b, "INSERT INTO votes VALUES (1, 'A'), (2, 'B'), (3, 'A'), (4, 'A'), (5, 'B')");
    let result = exec(b,
        "SELECT \
           SUM(CASE WHEN candidate = 'A' THEN 1 ELSE 0 END) AS a_votes, \
           SUM(CASE WHEN candidate = 'B' THEN 1 ELSE 0 END) AS b_votes \
         FROM votes");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(3)) | Some(Value::Int8(3)) => {}
        other => panic!("A should have 3 votes, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Int4(2)) | Some(Value::Int8(2)) => {}
        other => panic!("B should have 2 votes, got {:?}", other),
    }
}

#[test]
fn test_case_in_aggregate() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_case_in_aggregate_body(&b);
}

crate::net_tests!(test_case_in_aggregate);


fn test_case_update_with_case_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE prices (id INT, price FLOAT, category TEXT)");
    exec(b, "INSERT INTO prices VALUES (1, 10.0, 'basic'), (2, 50.0, 'premium'), (3, 100.0, 'luxury')");
    exec(b,
        "UPDATE prices SET price = CASE \
           WHEN category = 'basic' THEN price * 1.1 \
           WHEN category = 'premium' THEN price * 1.05 \
           ELSE price \
         END");
    let result = exec(b, "SELECT id, price FROM prices ORDER BY id");
    assert_eq!(result.rows.len(), 3);
    // basic: 10 * 1.1 = 11, premium: 50 * 1.05 = 52.5, luxury: 100
    match result.rows[0].get_by_idx(1) {
        Some(Value::Float8(v)) => assert!((*v - 11.0).abs() < 0.01, "basic price should be 11"),
        Some(Value::Int4(11)) => {}
        other => panic!("Expected ~11.0, got {:?}", other),
    }
}

#[test]
fn test_case_update_with_case() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_case_update_with_case_body(&b);
}

crate::net_tests!(test_case_update_with_case);


fn test_case_multiple_when_branches_body(b: &crate::common::Backend) {
    let result = exec(b,
        "SELECT CASE 3 \
           WHEN 1 THEN 'one' \
           WHEN 2 THEN 'two' \
           WHEN 3 THEN 'three' \
           WHEN 4 THEN 'four' \
           ELSE 'other' \
         END");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "three"),
        other => panic!("Expected 'three', got {:?}", other),
    }
}

#[test]
fn test_case_multiple_when_branches() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_case_multiple_when_branches_body(&b);
}

crate::net_tests!(test_case_multiple_when_branches);


fn test_case_in_case2_tbl_body(b: &crate::common::Backend) {
    setup_case_tbl(b);
    let result = exec(b,
        "SELECT i, j, \
           CASE WHEN i = 1 THEN 'one' WHEN i = 2 THEN 'two' ELSE 'other' END AS label \
         FROM case2_tbl ORDER BY i, j");
    assert_eq!(result.rows.len(), 5);
}

#[test]
fn test_case_in_case2_tbl() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_case_in_case2_tbl_body(&b);
}

crate::net_tests!(test_case_in_case2_tbl);


fn test_coalesce_from_table_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE maybe_null (id INT, val INT, default_val INT)");
    exec(b, "INSERT INTO maybe_null VALUES (1, NULL, 99), (2, 42, 99), (3, NULL, 77)");
    let result = exec(b, "SELECT id, COALESCE(val, default_val) AS effective_val FROM maybe_null ORDER BY id");
    assert_eq!(result.rows.len(), 3);
    match result.rows[0].get_by_idx(1) {
        Some(Value::Int4(99)) => {}
        other => panic!("id=1 should coalesce to 99, got {:?}", other),
    }
    match result.rows[1].get_by_idx(1) {
        Some(Value::Int4(42)) => {}
        other => panic!("id=2 val=42 should return 42, got {:?}", other),
    }
}

#[test]
fn test_coalesce_from_table() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_coalesce_from_table_body(&b);
}

crate::net_tests!(test_coalesce_from_table);


fn test_case_with_boolean_result_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE tasks (tid INT, priority TEXT)");
    exec(b, "INSERT INTO tasks VALUES (1, 'high'), (2, 'low'), (3, 'medium'), (4, 'high')");
    let n = query_int(b,
        "SELECT COUNT(*) FROM tasks WHERE CASE WHEN priority = 'high' THEN TRUE ELSE FALSE END = TRUE");
    assert_eq!(n, 2, "Two high-priority tasks");
}

#[test]
fn test_case_with_boolean_result() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_case_with_boolean_result_body(&b);
}

crate::net_tests!(test_case_with_boolean_result);

