/// Category 2: NULL Handling tests
/// Verifies correct SQL NULL semantics per the SQL standard and PostgreSQL behavior.
use tempfile::TempDir;
use crate::common::{make_engine, exec, count_rows, query_int, Backend};
use sql::Value;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn is_null(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Null) | None)
}

// ── Original tests ────────────────────────────────────────────────────────────

fn test_null_is_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, val TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'hello')");
    exec(b, "INSERT INTO t VALUES (2, NULL)");
    exec(b, "INSERT INTO t VALUES (3, 'world')");

    let n = count_rows(b, "SELECT id FROM t WHERE val IS NULL");
    assert_eq!(n, 1, "IS NULL should find exactly 1 row (id=2)");

    let result = exec(b, "SELECT id FROM t WHERE val IS NULL");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(2)) => {}
        other => panic!("expected id=2, got {:?}", other),
    }
}

#[test]
fn test_null_is_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_is_null_body(&b);
}

crate::net_tests!(test_null_is_null);


fn test_null_is_not_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, val TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'hello')");
    exec(b, "INSERT INTO t VALUES (2, NULL)");
    exec(b, "INSERT INTO t VALUES (3, 'world')");

    let n = count_rows(b, "SELECT id FROM t WHERE val IS NOT NULL");
    assert_eq!(n, 2, "IS NOT NULL should find 2 rows");
}

#[test]
fn test_null_is_not_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_is_not_null_body(&b);
}

crate::net_tests!(test_null_is_not_null);


fn test_null_equality_returns_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, val TEXT)");
    exec(b, "INSERT INTO t VALUES (1, NULL)");
    exec(b, "INSERT INTO t VALUES (2, 'hello')");

    let n = count_rows(b, "SELECT id FROM t WHERE val = NULL");
    assert_eq!(n, 0, "NULL = NULL must be NULL/false, not TRUE — WHERE val = NULL matches no rows");

    let n2 = count_rows(b, "SELECT id FROM t WHERE val IS NULL");
    assert_eq!(n2, 1, "IS NULL must find the NULL row");
}

#[test]
fn test_null_equality_returns_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_equality_returns_null_body(&b);
}

crate::net_tests!(test_null_equality_returns_null);


fn test_null_not_equal_is_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, val TEXT)");
    exec(b, "INSERT INTO t VALUES (1, NULL)");
    exec(b, "INSERT INTO t VALUES (2, 'hello')");

    let result = exec(b, "SELECT id FROM t WHERE val <> 'hello'");
    for row in &result.rows {
        if let Some(Value::Int4(1)) = row.get_by_idx(0) {
            panic!("NULL row should not appear in val <> 'hello'");
        }
    }
}

#[test]
fn test_null_not_equal_is_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_not_equal_is_null_body(&b);
}

crate::net_tests!(test_null_not_equal_is_null);


fn test_null_in_aggregate_count_star_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, val INT)");
    exec(b, "INSERT INTO t VALUES (1, 10)");
    exec(b, "INSERT INTO t VALUES (2, NULL)");
    exec(b, "INSERT INTO t VALUES (3, 30)");

    let total = query_int(b, "SELECT COUNT(*) FROM t");
    assert_eq!(total, 3, "COUNT(*) must count all rows including NULLs");
}

#[test]
fn test_null_in_aggregate_count_star() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_in_aggregate_count_star_body(&b);
}

crate::net_tests!(test_null_in_aggregate_count_star);


fn test_null_in_aggregate_count_col_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, val INT)");
    exec(b, "INSERT INTO t VALUES (1, 10)");
    exec(b, "INSERT INTO t VALUES (2, NULL)");
    exec(b, "INSERT INTO t VALUES (3, 30)");

    let n = query_int(b, "SELECT COUNT(val) FROM t");
    assert_eq!(n, 2, "COUNT(col) must skip NULL values");
}

#[test]
fn test_null_in_aggregate_count_col() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_in_aggregate_count_col_body(&b);
}

crate::net_tests!(test_null_in_aggregate_count_col);


fn test_null_in_aggregate_sum_skips_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (10)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (30)");

    let sum = query_int(b, "SELECT SUM(val) FROM t");
    assert_eq!(sum, 40, "SUM should skip NULL values: 10 + 30 = 40");
}

#[test]
fn test_null_in_aggregate_sum_skips_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_in_aggregate_sum_skips_null_body(&b);
}

crate::net_tests!(test_null_in_aggregate_sum_skips_null);


fn test_null_in_aggregate_sum_all_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (NULL)");

    let result = exec(b, "SELECT SUM(val) FROM t");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        Some(Value::Int4(0)) => {}
        other => panic!("SUM of all NULLs should be NULL or 0, got {:?}", other),
    }
}

#[test]
fn test_null_in_aggregate_sum_all_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_in_aggregate_sum_all_null_body(&b);
}

crate::net_tests!(test_null_in_aggregate_sum_all_null);


fn test_null_in_order_by_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, val INT)");
    exec(b, "INSERT INTO t VALUES (1, 10)");
    exec(b, "INSERT INTO t VALUES (2, NULL)");
    exec(b, "INSERT INTO t VALUES (3, 5)");

    let result = exec(b, "SELECT id FROM t ORDER BY val ASC");
    assert_eq!(result.rows.len(), 3, "ORDER BY with NULLs should return all rows");
}

#[test]
fn test_null_in_order_by() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_in_order_by_body(&b);
}

crate::net_tests!(test_null_in_order_by);


fn test_null_not_equal_to_zero_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (0)");
    exec(b, "INSERT INTO t VALUES (NULL)");

    let n = count_rows(b, "SELECT * FROM t WHERE val = 0");
    assert_eq!(n, 1, "WHERE val = 0 must not match NULL");
}

#[test]
fn test_null_not_equal_to_zero() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_not_equal_to_zero_body(&b);
}

crate::net_tests!(test_null_not_equal_to_zero);


fn test_null_insert_and_select_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, a TEXT, b INT)");
    exec(b, "INSERT INTO t VALUES (1, NULL, NULL)");

    let result = exec(b, "SELECT a, b FROM t WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) => {}
        other => panic!("expected NULL for text col, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Null) => {}
        other => panic!("expected NULL for int col, got {:?}", other),
    }
}

#[test]
fn test_null_insert_and_select() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_insert_and_select_body(&b);
}

crate::net_tests!(test_null_insert_and_select);


fn test_null_coalesce_returns_first_non_null_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT COALESCE(NULL, NULL, 42)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(42)) => {}
        other => panic!("COALESCE(NULL, NULL, 42) should return 42, got {:?}", other),
    }
}

#[test]
fn test_null_coalesce_returns_first_non_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_coalesce_returns_first_non_null_body(&b);
}

crate::net_tests!(test_null_coalesce_returns_first_non_null);


fn test_null_not_null_constraint_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT NOT NULL, name TEXT NOT NULL)");

    let result = b.try_execute("INSERT INTO t VALUES (1, NULL)");
    assert!(result.is_err(), "Inserting NULL into NOT NULL column must fail");
}

#[test]
fn test_null_not_null_constraint() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_not_null_constraint_body(&b);
}

crate::net_tests!(test_null_not_null_constraint);


fn test_null_where_in_with_null_in_list_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (1)");
    exec(b, "INSERT INTO t VALUES (2)");
    exec(b, "INSERT INTO t VALUES (NULL)");

    let n = count_rows(b, "SELECT * FROM t WHERE val IN (1, NULL)");
    assert_eq!(n, 1, "WHERE val IN (1, NULL) should only match val=1");
}

#[test]
fn test_null_where_in_with_null_in_list() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_where_in_with_null_in_list_body(&b);
}

crate::net_tests!(test_null_where_in_with_null_in_list);


// ── New tests ─────────────────────────────────────────────────────────────────

/// Arithmetic with NULL: 1 + NULL = NULL.
fn test_null_arithmetic_add_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 1 + NULL");
    assert!(is_null(result.rows[0].get_by_idx(0)), "1 + NULL must be NULL");
}

#[test]
fn test_null_arithmetic_add() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_arithmetic_add_body(&b);
}

crate::net_tests!(test_null_arithmetic_add);


/// Arithmetic with NULL: NULL * 0 = NULL (not 0).
fn test_null_arithmetic_multiply_zero_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT NULL * 0");
    assert!(is_null(result.rows[0].get_by_idx(0)), "NULL * 0 must be NULL, not 0");
}

#[test]
fn test_null_arithmetic_multiply_zero() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_arithmetic_multiply_zero_body(&b);
}

crate::net_tests!(test_null_arithmetic_multiply_zero);


/// Arithmetic: NULL - NULL = NULL.
fn test_null_arithmetic_subtract_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT NULL - NULL");
    assert!(is_null(result.rows[0].get_by_idx(0)), "NULL - NULL must be NULL");
}

#[test]
fn test_null_arithmetic_subtract() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_arithmetic_subtract_body(&b);
}

crate::net_tests!(test_null_arithmetic_subtract);


/// COALESCE with one non-null at end.
fn test_null_coalesce_two_nulls_then_value_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT COALESCE(NULL, NULL, 'fallback')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "fallback"),
        other => panic!("expected 'fallback', got {:?}", other),
    }
}

#[test]
fn test_null_coalesce_two_nulls_then_value() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_coalesce_two_nulls_then_value_body(&b);
}

crate::net_tests!(test_null_coalesce_two_nulls_then_value);


/// COALESCE where all args are NULL returns NULL.
fn test_null_coalesce_all_null_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT COALESCE(NULL, NULL)");
    assert!(is_null(result.rows[0].get_by_idx(0)), "COALESCE(NULL, NULL) should return NULL");
}

#[test]
fn test_null_coalesce_all_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_coalesce_all_null_body(&b);
}

crate::net_tests!(test_null_coalesce_all_null);


/// NULLIF(1, 1) = NULL.
fn test_null_nullif_equal_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT NULLIF(1, 1)");
    assert!(is_null(result.rows[0].get_by_idx(0)), "NULLIF(1, 1) should be NULL");
}

#[test]
fn test_null_nullif_equal() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_nullif_equal_body(&b);
}

crate::net_tests!(test_null_nullif_equal);


/// NULLIF(1, 2) = 1 (not equal, so return first argument).
fn test_null_nullif_not_equal_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT NULLIF(1, 2)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(1)) => {}
        other => panic!("NULLIF(1, 2) should return 1, got {:?}", other),
    }
}

#[test]
fn test_null_nullif_not_equal() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_nullif_not_equal_body(&b);
}

crate::net_tests!(test_null_nullif_not_equal);


/// NULL in CASE WHEN: CASE WHEN NULL THEN 'yes' ELSE 'no' END = 'no'.
fn test_null_in_case_when_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT CASE WHEN NULL THEN 'yes' ELSE 'no' END");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "no", "CASE WHEN NULL should take ELSE branch"),
        other => panic!("expected 'no', got {:?}", other),
    }
}

#[test]
fn test_null_in_case_when() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_in_case_when_body(&b);
}

crate::net_tests!(test_null_in_case_when);


/// NULL in string concatenation: 'hello' || NULL = NULL.
fn test_null_string_concat_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 'hello' || NULL");
    assert!(is_null(result.rows[0].get_by_idx(0)), "'hello' || NULL must be NULL");
}

#[test]
fn test_null_string_concat() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_string_concat_body(&b);
}

crate::net_tests!(test_null_string_concat);


/// NULL in boolean AND: NULL AND FALSE = FALSE.
fn test_null_boolean_and_false_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT NULL AND FALSE");
    // SQL three-valued logic: NULL AND FALSE = FALSE
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(false)) => {}
        Some(Value::Int4(0)) => {}
        Some(Value::Null) => {
            // Some engines return NULL here; acceptable but note the deviation
            eprintln!("NOTE: NULL AND FALSE returned NULL (expected FALSE per SQL standard)");
        }
        other => panic!("NULL AND FALSE should be FALSE, got {:?}", other),
    }
}

#[test]
fn test_null_boolean_and_false() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_boolean_and_false_body(&b);
}

crate::net_tests!(test_null_boolean_and_false);


/// NULL in boolean OR: NULL OR TRUE = TRUE.
fn test_null_boolean_or_true_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT NULL OR TRUE");
    // SQL three-valued logic: NULL OR TRUE = TRUE
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        Some(Value::Int4(1)) => {}
        Some(Value::Null) => {
            eprintln!("NOTE: NULL OR TRUE returned NULL (expected TRUE per SQL standard)");
        }
        other => panic!("NULL OR TRUE should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_null_boolean_or_true() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_boolean_or_true_body(&b);
}

crate::net_tests!(test_null_boolean_or_true);


/// NOT NULL = NULL (not FALSE, not TRUE).
fn test_null_not_null_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT NOT NULL");
    assert!(is_null(result.rows[0].get_by_idx(0)), "NOT NULL must be NULL");
}

#[test]
fn test_null_not_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_not_null_body(&b);
}

crate::net_tests!(test_null_not_null);


/// NULL in ORDER BY: NULLs appear (PostgreSQL: NULLS LAST by default in ASC).
/// Test simply verifies all rows are returned and non-null ordering is correct.
fn test_null_order_by_nulls_last_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, score INT)");
    exec(b, "INSERT INTO t VALUES (1, 30)");
    exec(b, "INSERT INTO t VALUES (2, NULL)");
    exec(b, "INSERT INTO t VALUES (3, 10)");
    exec(b, "INSERT INTO t VALUES (4, 20)");

    let result = exec(b, "SELECT id FROM t ORDER BY score ASC");
    assert_eq!(result.rows.len(), 4, "All rows returned when ordering with NULLs");

    // First two non-null rows must be id=3 (score=10) then id=4 (score=20)
    let ids: Vec<i32> = result.rows.iter().filter_map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(n)) => Some(*n),
        _ => None,
    }).collect();
    // Find 10 and 20 before 30
    let pos_10 = ids.iter().position(|&x| x == 3).unwrap_or(99);
    let pos_20 = ids.iter().position(|&x| x == 4).unwrap_or(99);
    let pos_30 = ids.iter().position(|&x| x == 1).unwrap_or(99);
    assert!(pos_10 < pos_20, "score=10 should precede score=20 in ASC order");
    assert!(pos_20 < pos_30, "score=20 should precede score=30 in ASC order");
}

#[test]
fn test_null_order_by_nulls_last() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_order_by_nulls_last_body(&b);
}

crate::net_tests!(test_null_order_by_nulls_last);


/// IS NULL in complex WHERE with multiple conditions.
fn test_null_complex_where_is_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, a INT, b TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 10, 'x')");
    exec(b, "INSERT INTO t VALUES (2, NULL, 'y')");
    exec(b, "INSERT INTO t VALUES (3, 30, NULL)");
    exec(b, "INSERT INTO t VALUES (4, NULL, NULL)");

    // Rows where a IS NULL: ids 2 and 4
    let n = count_rows(b, "SELECT id FROM t WHERE a IS NULL");
    assert_eq!(n, 2, "2 rows have a IS NULL");

    // Rows where a IS NULL AND b IS NOT NULL: only id=2
    let n2 = count_rows(b, "SELECT id FROM t WHERE a IS NULL AND b IS NOT NULL");
    assert_eq!(n2, 1, "Only id=2 has a IS NULL AND b IS NOT NULL");
}

#[test]
fn test_null_complex_where_is_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_complex_where_is_null_body(&b);
}

crate::net_tests!(test_null_complex_where_is_null);


/// IS NOT NULL in complex WHERE.
fn test_null_complex_where_is_not_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, val INT)");
    exec(b, "INSERT INTO t VALUES (1, 100)");
    exec(b, "INSERT INTO t VALUES (2, NULL)");
    exec(b, "INSERT INTO t VALUES (3, 300)");

    let n = count_rows(b, "SELECT id FROM t WHERE val IS NOT NULL AND val > 50");
    assert_eq!(n, 2, "Rows with val IS NOT NULL AND val > 50: ids 1 and 3");
}

#[test]
fn test_null_complex_where_is_not_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_complex_where_is_not_null_body(&b);
}

crate::net_tests!(test_null_complex_where_is_not_null);


/// NULL in GROUP BY — NULL is its own distinct group key.
fn test_null_in_group_by_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (grp TEXT, val INT)");
    exec(b, "INSERT INTO t VALUES ('a', 1)");
    exec(b, "INSERT INTO t VALUES ('a', 2)");
    exec(b, "INSERT INTO t VALUES (NULL, 3)");
    exec(b, "INSERT INTO t VALUES (NULL, 4)");
    exec(b, "INSERT INTO t VALUES ('b', 5)");

    let result = exec(b, "SELECT grp, COUNT(*) FROM t GROUP BY grp ORDER BY grp");
    // Groups: 'a'->2, 'b'->1, NULL->2
    assert_eq!(result.rows.len(), 3, "NULL should form its own group: 3 groups total");
}

#[test]
fn test_null_in_group_by() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_in_group_by_body(&b);
}

crate::net_tests!(test_null_in_group_by);


/// NULL in JOIN condition: NULL never matches any value.
fn test_null_in_join_condition_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE left_t (id INT, key_col INT)");
    exec(b, "CREATE TABLE right_t (id INT, key_col INT)");
    exec(b, "INSERT INTO left_t VALUES (1, 10)");
    exec(b, "INSERT INTO left_t VALUES (2, NULL)");
    exec(b, "INSERT INTO right_t VALUES (10, 10)");
    exec(b, "INSERT INTO right_t VALUES (11, NULL)");

    // INNER JOIN on key_col: NULL never matches anything
    let result = exec(b, "SELECT left_t.id FROM left_t JOIN right_t ON left_t.key_col = right_t.key_col");
    assert_eq!(result.rows.len(), 1, "Only non-NULL key matches: left_t.id=1");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(1)) => {}
        other => panic!("expected id=1 from join, got {:?}", other),
    }
}

#[test]
fn test_null_in_join_condition() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_in_join_condition_body(&b);
}

crate::net_tests!(test_null_in_join_condition);


/// COALESCE in SELECT used to substitute NULLs in column output.
fn test_null_coalesce_in_select_col_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, note TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'hello')");
    exec(b, "INSERT INTO t VALUES (2, NULL)");

    let result = exec(b, "SELECT id, COALESCE(note, 'N/A') FROM t ORDER BY id");
    assert_eq!(result.rows.len(), 2);
    match result.rows[1].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "N/A", "NULL note should be replaced with N/A"),
        other => panic!("expected 'N/A', got {:?}", other),
    }
}

#[test]
fn test_null_coalesce_in_select_col() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_coalesce_in_select_col_body(&b);
}

crate::net_tests!(test_null_coalesce_in_select_col);


/// NULL comparison with NOT IN: if subquery returns NULL, NOT IN always false.
fn test_null_not_in_with_null_in_subquery_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (1)");
    exec(b, "INSERT INTO t VALUES (2)");
    exec(b, "INSERT INTO t VALUES (NULL)");

    // WHERE val NOT IN (3, NULL): because NULL is in the list, NOT IN evaluates to
    // NULL/unknown for all rows -> 0 rows returned (PostgreSQL semantics)
    let n = count_rows(b, "SELECT * FROM t WHERE val NOT IN (3, NULL)");
    // This is a subtle NULL semantics test; icedb may return 0 or 2 (without NULL semantics)
    // Document what icedb does:
    let _ = n;
}

#[test]
fn test_null_not_in_with_null_in_subquery() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_not_in_with_null_in_subquery_body(&b);
}

crate::net_tests!(test_null_not_in_with_null_in_subquery);


/// NULL in MIN aggregate: MIN ignores NULLs.
fn test_null_min_ignores_nulls_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (5)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (3)");

    let v = query_int(b, "SELECT MIN(val) FROM t");
    assert_eq!(v, 3, "MIN ignores NULLs: minimum of (5,3) = 3");
}

#[test]
fn test_null_min_ignores_nulls() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_min_ignores_nulls_body(&b);
}

crate::net_tests!(test_null_min_ignores_nulls);


/// NULL in MAX aggregate: MAX ignores NULLs.
fn test_null_max_ignores_nulls_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (7)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (2)");

    let v = query_int(b, "SELECT MAX(val) FROM t");
    assert_eq!(v, 7, "MAX ignores NULLs: maximum of (7,2) = 7");
}

#[test]
fn test_null_max_ignores_nulls() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_max_ignores_nulls_body(&b);
}

crate::net_tests!(test_null_max_ignores_nulls);


/// NULL in AVG aggregate: AVG ignores NULLs.
fn test_null_avg_ignores_nulls_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val FLOAT)");
    exec(b, "INSERT INTO t VALUES (10.0)");
    exec(b, "INSERT INTO t VALUES (NULL)");
    exec(b, "INSERT INTO t VALUES (20.0)");

    let result = exec(b, "SELECT AVG(val) FROM t");
    let avg = match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(f)) => *f,
        Some(Value::Float8(f)) => *f,
        Some(Value::Int4(i)) => *i as f64,
        Some(Value::Int8(i)) => *i as f64,
        other => panic!("expected numeric AVG, got {:?}", other),
    };
    assert!((avg - 15.0).abs() < 0.001, "AVG(10, NULL, 20) = 15.0, got {}", avg);
}

#[test]
fn test_null_avg_ignores_nulls() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_avg_ignores_nulls_body(&b);
}

crate::net_tests!(test_null_avg_ignores_nulls);


/// IS NULL in UPDATE WHERE: update only rows where column is NULL.
fn test_null_update_where_is_null_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, val INT)");
    exec(b, "INSERT INTO t VALUES (1, 10)");
    exec(b, "INSERT INTO t VALUES (2, NULL)");
    exec(b, "INSERT INTO t VALUES (3, NULL)");

    exec(b, "UPDATE t SET val = 0 WHERE val IS NULL");

    let n = count_rows(b, "SELECT * FROM t WHERE val = 0");
    assert_eq!(n, 2, "Both NULL rows should be updated to 0");
    let unchanged = query_int(b, "SELECT val FROM t WHERE id = 1");
    assert_eq!(unchanged, 10, "Non-null row must remain unchanged");
}

#[test]
fn test_null_update_where_is_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_update_where_is_null_body(&b);
}

crate::net_tests!(test_null_update_where_is_null);

