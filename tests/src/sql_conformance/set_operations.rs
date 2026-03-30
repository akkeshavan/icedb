/// Category 6: Set operation tests (UNION, INTERSECT, EXCEPT)
/// Based on PostgreSQL union.sql patterns.
use tempfile::TempDir;
use crate::common::{make_engine, exec, count_rows, query_int, Backend};
use sql::Value;

fn setup_sets(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE a (id INT, val TEXT)");
    exec(b, "CREATE TABLE b (id INT, val TEXT)");
    exec(b, "INSERT INTO a VALUES (1, 'alpha')");
    exec(b, "INSERT INTO a VALUES (2, 'beta')");
    exec(b, "INSERT INTO a VALUES (3, 'gamma')");
    exec(b, "INSERT INTO b VALUES (2, 'beta')");
    exec(b, "INSERT INTO b VALUES (3, 'gamma')");
    exec(b, "INSERT INTO b VALUES (4, 'delta')");
}

fn test_union_basic_body(b: &crate::common::Backend) {
    setup_sets(b);

    // UNION deduplicates: {1,alpha}, {2,beta}, {3,gamma}, {4,delta} = 4 distinct rows
    let n = count_rows(b, "SELECT id, val FROM a UNION SELECT id, val FROM b");
    assert_eq!(n, 4, "UNION should deduplicate to 4 rows");
}

#[test]
fn test_union_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_basic_body(&b);
}

crate::net_tests!(test_union_basic);


fn test_union_all_keeps_duplicates_body(b: &crate::common::Backend) {
    setup_sets(b);

    // UNION ALL: 3 from a + 3 from b = 6 rows (including 2 duplicates)
    let n = count_rows(b, "SELECT id, val FROM a UNION ALL SELECT id, val FROM b");
    assert_eq!(n, 6, "UNION ALL should keep all 6 rows including duplicates");
}

#[test]
fn test_union_all_keeps_duplicates() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_all_keeps_duplicates_body(&b);
}

crate::net_tests!(test_union_all_keeps_duplicates);


fn test_union_deduplicates_single_column_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t1 (x INT)");
    exec(b, "CREATE TABLE t2 (x INT)");
    exec(b, "INSERT INTO t1 VALUES (1), (2), (3)");
    exec(b, "INSERT INTO t2 VALUES (2), (3), (4)");

    let n = count_rows(b, "SELECT x FROM t1 UNION SELECT x FROM t2");
    assert_eq!(n, 4, "UNION should yield {{1,2,3,4}} = 4 distinct values");
}

#[test]
fn test_union_deduplicates_single_column() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_deduplicates_single_column_body(&b);
}

crate::net_tests!(test_union_deduplicates_single_column);


fn test_union_all_single_column_count_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t1 (x INT)");
    exec(b, "CREATE TABLE t2 (x INT)");
    exec(b, "INSERT INTO t1 VALUES (1), (2), (3)");
    exec(b, "INSERT INTO t2 VALUES (2), (3), (4)");

    let n = count_rows(b, "SELECT x FROM t1 UNION ALL SELECT x FROM t2");
    assert_eq!(n, 6, "UNION ALL: 3 + 3 = 6");
}

#[test]
fn test_union_all_single_column_count() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_all_single_column_count_body(&b);
}

crate::net_tests!(test_union_all_single_column_count);


fn test_intersect_body(b: &crate::common::Backend) {
    setup_sets(b);

    // {2,beta} and {3,gamma} appear in both
    let n = count_rows(b, "SELECT id, val FROM a INTERSECT SELECT id, val FROM b");
    assert_eq!(n, 2, "INTERSECT should return 2 rows in both tables");
}

#[test]
fn test_intersect() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_intersect_body(&b);
}

crate::net_tests!(test_intersect);


fn test_intersect_single_column_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t1 (x INT)");
    exec(b, "CREATE TABLE t2 (x INT)");
    exec(b, "INSERT INTO t1 VALUES (1), (2), (3), (4)");
    exec(b, "INSERT INTO t2 VALUES (2), (4), (6)");

    let result = exec(b, "SELECT x FROM t1 INTERSECT SELECT x FROM t2 ORDER BY x");
    assert_eq!(result.rows.len(), 2, "INTERSECT should return {{2, 4}}");
    let vals: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(vals, vec![2, 4]);
}

#[test]
fn test_intersect_single_column() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_intersect_single_column_body(&b);
}

crate::net_tests!(test_intersect_single_column);


fn test_except_basic_body(b: &crate::common::Backend) {
    setup_sets(b);

    // a - b = {1,alpha}
    let result = exec(b, "SELECT id, val FROM a EXCEPT SELECT id, val FROM b");
    assert_eq!(result.rows.len(), 1, "EXCEPT should return 1 row");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(1)) => {}
        other => panic!("expected id=1, got {:?}", other),
    }
}

#[test]
fn test_except_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_except_basic_body(&b);
}

crate::net_tests!(test_except_basic);


fn test_except_single_column_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t1 (x INT)");
    exec(b, "CREATE TABLE t2 (x INT)");
    for i in 1..=5 {
        exec(b, &format!("INSERT INTO t1 VALUES ({})", i));
    }
    for i in 3..=7 {
        exec(b, &format!("INSERT INTO t2 VALUES ({})", i));
    }

    let result = exec(b, "SELECT x FROM t1 EXCEPT SELECT x FROM t2 ORDER BY x");
    assert_eq!(result.rows.len(), 2, "EXCEPT: {{1,2,3,4,5}} - {{3,4,5,6,7}} = {{1,2}}");
    let vals: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(vals, vec![1, 2]);
}

#[test]
fn test_except_single_column() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_except_single_column_body(&b);
}

crate::net_tests!(test_except_single_column);


fn test_union_with_order_by_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t1 (x INT)");
    exec(b, "CREATE TABLE t2 (x INT)");
    exec(b, "INSERT INTO t1 VALUES (3), (1)");
    exec(b, "INSERT INTO t2 VALUES (4), (2)");

    let result = exec(b, "SELECT x FROM t1 UNION SELECT x FROM t2 ORDER BY x");
    assert_eq!(result.rows.len(), 4);
    let vals: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(vals, vec![1, 2, 3, 4], "UNION ORDER BY should sort correctly");
}

#[test]
fn test_union_with_order_by() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_with_order_by_body(&b);
}

crate::net_tests!(test_union_with_order_by);


fn test_union_with_where_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t1 (x INT)");
    exec(b, "CREATE TABLE t2 (x INT)");
    for i in 1..=5 { exec(b, &format!("INSERT INTO t1 VALUES ({})", i)); }
    for i in 6..=10 { exec(b, &format!("INSERT INTO t2 VALUES ({})", i)); }

    // Only rows > 3 from t1 and > 7 from t2
    let result = exec(b, "SELECT x FROM t1 WHERE x > 3 UNION SELECT x FROM t2 WHERE x > 7 ORDER BY x");
    // t1: 4, 5; t2: 8, 9, 10 → 5 rows
    assert_eq!(result.rows.len(), 5);
}

#[test]
fn test_union_with_where() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_with_where_body(&b);
}

crate::net_tests!(test_union_with_where);


fn test_union_three_selects_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (x INT)");
    exec(b, "INSERT INTO t VALUES (1), (2), (3)");

    // UNION of three identical selects → deduplicated = 3 rows
    let n = count_rows(b,
        "SELECT x FROM t UNION SELECT x FROM t UNION SELECT x FROM t");
    assert_eq!(n, 3, "UNION of 3 identical selects → 3 distinct rows");
}

#[test]
fn test_union_three_selects() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_three_selects_body(&b);
}

crate::net_tests!(test_union_three_selects);


fn test_union_all_three_selects_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (x INT)");
    exec(b, "INSERT INTO t VALUES (1), (2), (3)");

    // UNION ALL: 3+3+3 = 9 rows
    let n = count_rows(b,
        "SELECT x FROM t UNION ALL SELECT x FROM t UNION ALL SELECT x FROM t");
    assert_eq!(n, 9, "UNION ALL of 3 identical selects = 9 rows");
}

#[test]
fn test_union_all_three_selects() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_all_three_selects_body(&b);
}

crate::net_tests!(test_union_all_three_selects);


fn test_union_empty_table_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t1 (x INT)");
    exec(b, "CREATE TABLE t2 (x INT)");
    exec(b, "INSERT INTO t1 VALUES (1), (2)");
    // t2 empty

    let n = count_rows(b, "SELECT x FROM t1 UNION SELECT x FROM t2");
    assert_eq!(n, 2, "UNION with empty table should just return the non-empty side");
}

#[test]
fn test_union_empty_table() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_empty_table_body(&b);
}

crate::net_tests!(test_union_empty_table);


fn test_except_no_difference_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t1 (x INT)");
    exec(b, "CREATE TABLE t2 (x INT)");
    exec(b, "INSERT INTO t1 VALUES (1), (2)");
    exec(b, "INSERT INTO t2 VALUES (1), (2), (3)");

    // t1 EXCEPT t2 = empty
    let n = count_rows(b, "SELECT x FROM t1 EXCEPT SELECT x FROM t2");
    assert_eq!(n, 0, "EXCEPT with superset right operand = 0 rows");
}

#[test]
fn test_except_no_difference() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_except_no_difference_body(&b);
}

crate::net_tests!(test_except_no_difference);


fn test_intersect_empty_result_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t1 (x INT)");
    exec(b, "CREATE TABLE t2 (x INT)");
    exec(b, "INSERT INTO t1 VALUES (1), (2)");
    exec(b, "INSERT INTO t2 VALUES (3), (4)");

    let n = count_rows(b, "SELECT x FROM t1 INTERSECT SELECT x FROM t2");
    assert_eq!(n, 0, "INTERSECT of disjoint sets = 0 rows");
}

#[test]
fn test_intersect_empty_result() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_intersect_empty_result_body(&b);
}

crate::net_tests!(test_intersect_empty_result);


fn test_union_mixed_literals_body(b: &crate::common::Backend) {
    // Test UNION with a table and a literal subquery
    exec(b, "CREATE TABLE t (val TEXT)");
    exec(b, "INSERT INTO t VALUES ('existing')");

    let result = exec(b, "SELECT val FROM t UNION SELECT 'new_value' AS val ORDER BY val");
    assert_eq!(result.rows.len(), 2, "UNION table + literal should return 2 rows");
}

#[test]
fn test_union_mixed_literals() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_mixed_literals_body(&b);
}

crate::net_tests!(test_union_mixed_literals);


// ---- Additional set operation tests from PostgreSQL union.sql ----

fn test_union_literals_1_2_ordered_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 1 AS two UNION SELECT 2 ORDER BY 1");
    assert_eq!(result.rows.len(), 2);
    let vals: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(vals, vec![1, 2]);
}

#[test]
fn test_union_literals_1_2_ordered() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_literals_1_2_ordered_body(&b);
}

crate::net_tests!(test_union_literals_1_2_ordered);


fn test_union_literals_dedup_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 1 AS one UNION SELECT 1 ORDER BY 1");
    assert_eq!(result.rows.len(), 1, "UNION deduplicates identical rows");
}

#[test]
fn test_union_literals_dedup() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_literals_dedup_body(&b);
}

crate::net_tests!(test_union_literals_dedup);


fn test_union_all_literals_keeps_dups_body(b: &crate::common::Backend) {
    let n = count_rows(b, "SELECT 1 AS two UNION ALL SELECT 2");
    assert_eq!(n, 2);
}

#[test]
fn test_union_all_literals_keeps_dups() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_all_literals_keeps_dups_body(&b);
}

crate::net_tests!(test_union_all_literals_keeps_dups);


fn test_union_all_same_literal_keeps_dup_body(b: &crate::common::Backend) {
    let n = count_rows(b, "SELECT 1 AS two UNION ALL SELECT 1");
    assert_eq!(n, 2, "UNION ALL keeps both copies");
}

#[test]
fn test_union_all_same_literal_keeps_dup() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_all_same_literal_keeps_dup_body(&b);
}

crate::net_tests!(test_union_all_same_literal_keeps_dup);


fn test_union_three_literals_ordered_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 1 AS three UNION SELECT 2 UNION SELECT 3 ORDER BY 1");
    assert_eq!(result.rows.len(), 3);
    let vals: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(vals, vec![1, 2, 3]);
}

#[test]
fn test_union_three_literals_ordered() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_three_literals_ordered_body(&b);
}

crate::net_tests!(test_union_three_literals_ordered);


fn test_union_three_with_dup_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 1 AS two UNION SELECT 2 UNION SELECT 2 ORDER BY 1");
    assert_eq!(result.rows.len(), 2, "1 and 2, duplicate 2 removed");
}

#[test]
fn test_union_three_with_dup() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_three_with_dup_body(&b);
}

crate::net_tests!(test_union_three_with_dup);


fn test_union_three_union_all_second_body(b: &crate::common::Backend) {
    // SELECT 1 UNION SELECT 2 UNION ALL SELECT 2 → 3 rows: 1, 2, 2
    let result = exec(b, "SELECT 1 AS three UNION SELECT 2 UNION ALL SELECT 2 ORDER BY 1");
    assert_eq!(result.rows.len(), 3);
}

#[test]
fn test_union_three_union_all_second() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_three_union_all_second_body(&b);
}

crate::net_tests!(test_union_three_union_all_second);


fn test_union_float_and_int_type_compat_body(b: &crate::common::Backend) {
    // 1.1 UNION 2 — type coercion
    let result = exec(b, "SELECT 1.1 AS two UNION SELECT 2 ORDER BY 1");
    assert_eq!(result.rows.len(), 2);
}

#[test]
fn test_union_float_and_int_type_compat() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_float_and_int_type_compat_body(&b);
}

crate::net_tests!(test_union_float_and_int_type_compat);


fn test_union_float_literals_ordered_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 1.1 AS two UNION SELECT 2.2 ORDER BY 1");
    assert_eq!(result.rows.len(), 2);
}

#[test]
fn test_union_float_literals_ordered() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_float_literals_ordered_body(&b);
}

crate::net_tests!(test_union_float_literals_ordered);


fn test_union_all_float_int_mixed_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 1.1 AS two UNION ALL SELECT 2 ORDER BY 1");
    assert_eq!(result.rows.len(), 2);
}

#[test]
fn test_union_all_float_int_mixed() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_all_float_int_mixed_body(&b);
}

crate::net_tests!(test_union_all_float_int_mixed);


fn test_intersect_literals_body(b: &crate::common::Backend) {
    let n = count_rows(b, "SELECT 1 INTERSECT SELECT 1");
    assert_eq!(n, 1, "1 INTERSECT 1 = one row");
}

#[test]
fn test_intersect_literals() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_intersect_literals_body(&b);
}

crate::net_tests!(test_intersect_literals);


fn test_intersect_empty_when_no_match_body(b: &crate::common::Backend) {
    let n = count_rows(b, "SELECT 1 INTERSECT SELECT 2");
    assert_eq!(n, 0, "1 INTERSECT 2 = empty");
}

#[test]
fn test_intersect_empty_when_no_match() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_intersect_empty_when_no_match_body(&b);
}

crate::net_tests!(test_intersect_empty_when_no_match);


fn test_except_literals_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 1 EXCEPT SELECT 2");
    assert_eq!(result.rows.len(), 1, "1 EXCEPT 2 = one row");
}

#[test]
fn test_except_literals() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_except_literals_body(&b);
}

crate::net_tests!(test_except_literals);


fn test_except_same_value_empty_body(b: &crate::common::Backend) {
    let n = count_rows(b, "SELECT 1 EXCEPT SELECT 1");
    assert_eq!(n, 0, "1 EXCEPT 1 = empty");
}

#[test]
fn test_except_same_value_empty() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_except_same_value_empty_body(&b);
}

crate::net_tests!(test_except_same_value_empty);


fn test_union_then_intersect_body(b: &crate::common::Backend) {
    // (SELECT 1,2,3 UNION SELECT 4,5,6) INTERSECT SELECT 4,5,6 → {4,5,6}
    let result = exec(b, "(SELECT 1, 2, 3 UNION SELECT 4, 5, 6) INTERSECT SELECT 4, 5, 6");
    assert_eq!(result.rows.len(), 1, "UNION then INTERSECT");
}

#[test]
fn test_union_then_intersect() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_then_intersect_body(&b);
}

crate::net_tests!(test_union_then_intersect);


fn test_union_then_except_body(b: &crate::common::Backend) {
    // (1,2,3 UNION 4,5,6) EXCEPT (4,5,6) → {1,2,3}
    let result = exec(b, "(SELECT 1, 2, 3 UNION SELECT 4, 5, 6) EXCEPT SELECT 4, 5, 6");
    assert_eq!(result.rows.len(), 1);
}

#[test]
fn test_union_then_except() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_then_except_body(&b);
}

crate::net_tests!(test_union_then_except);


fn test_union_with_text_column_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE words1 (w TEXT)");
    exec(b, "CREATE TABLE words2 (w TEXT)");
    exec(b, "INSERT INTO words1 VALUES ('apple'), ('banana'), ('cherry')");
    exec(b, "INSERT INTO words2 VALUES ('banana'), ('date'), ('elderberry')");
    let result = exec(b, "SELECT w FROM words1 UNION SELECT w FROM words2 ORDER BY w");
    assert_eq!(result.rows.len(), 5, "5 distinct words");
}

#[test]
fn test_union_with_text_column() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_with_text_column_body(&b);
}

crate::net_tests!(test_union_with_text_column);


fn test_intersect_with_text_column_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE words1 (w TEXT)");
    exec(b, "CREATE TABLE words2 (w TEXT)");
    exec(b, "INSERT INTO words1 VALUES ('apple'), ('banana'), ('cherry')");
    exec(b, "INSERT INTO words2 VALUES ('banana'), ('date'), ('cherry')");
    let result = exec(b, "SELECT w FROM words1 INTERSECT SELECT w FROM words2 ORDER BY w");
    assert_eq!(result.rows.len(), 2, "banana and cherry are in both");
}

#[test]
fn test_intersect_with_text_column() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_intersect_with_text_column_body(&b);
}

crate::net_tests!(test_intersect_with_text_column);


fn test_except_with_text_column_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE words1 (w TEXT)");
    exec(b, "CREATE TABLE words2 (w TEXT)");
    exec(b, "INSERT INTO words1 VALUES ('apple'), ('banana'), ('cherry')");
    exec(b, "INSERT INTO words2 VALUES ('banana'), ('cherry')");
    let result = exec(b, "SELECT w FROM words1 EXCEPT SELECT w FROM words2 ORDER BY w");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "apple"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_except_with_text_column() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_except_with_text_column_body(&b);
}

crate::net_tests!(test_except_with_text_column);


fn test_union_count_via_derived_table_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t1 (x INT)");
    exec(b, "CREATE TABLE t2 (x INT)");
    exec(b, "INSERT INTO t1 VALUES (1), (2), (3)");
    exec(b, "INSERT INTO t2 VALUES (4), (5), (6)");
    let n = query_int(b, "SELECT COUNT(*) FROM (SELECT x FROM t1 UNION ALL SELECT x FROM t2) AS combined");
    assert_eq!(n, 6);
}

#[test]
fn test_union_count_via_derived_table() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_count_via_derived_table_body(&b);
}

crate::net_tests!(test_union_count_via_derived_table);


fn test_union_with_limit_applied_after_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t1 (x INT)");
    exec(b, "CREATE TABLE t2 (x INT)");
    for i in 1..=5 { exec(b, &format!("INSERT INTO t1 VALUES ({})", i)); }
    for i in 6..=10 { exec(b, &format!("INSERT INTO t2 VALUES ({})", i)); }
    let result = exec(b, "SELECT x FROM t1 UNION SELECT x FROM t2 ORDER BY x LIMIT 4");
    assert_eq!(result.rows.len(), 4, "LIMIT 4 applies to UNION result");
}

#[test]
fn test_union_with_limit_applied_after() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_with_limit_applied_after_body(&b);
}

crate::net_tests!(test_union_with_limit_applied_after);


fn test_union_distinct_column_values_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE categories (cat TEXT)");
    exec(b, "CREATE TABLE products (cat TEXT)");
    exec(b, "INSERT INTO categories VALUES ('A'), ('B'), ('C')");
    exec(b, "INSERT INTO products VALUES ('B'), ('C'), ('D'), ('E')");
    let result = exec(b, "SELECT cat FROM categories UNION SELECT cat FROM products ORDER BY cat");
    assert_eq!(result.rows.len(), 5, "A,B,C,D,E = 5 distinct values");
}

#[test]
fn test_union_distinct_column_values() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_distinct_column_values_body(&b);
}

crate::net_tests!(test_union_distinct_column_values);


fn test_union_all_preserves_order_with_order_by_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE scores (s INT)");
    exec(b, "INSERT INTO scores VALUES (5), (3), (1), (4), (2)");
    let result = exec(b, "SELECT s FROM scores UNION ALL SELECT s FROM scores ORDER BY s");
    assert_eq!(result.rows.len(), 10, "UNION ALL doubles rows");
    let vals: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    // Should be sorted: 1,1,2,2,3,3,4,4,5,5
    assert_eq!(vals[0], 1);
    assert_eq!(vals[9], 5);
}

#[test]
fn test_union_all_preserves_order_with_order_by() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_all_preserves_order_with_order_by_body(&b);
}

crate::net_tests!(test_union_all_preserves_order_with_order_by);


fn test_intersect_all_keeps_multiple_copies_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t1 (x INT)");
    exec(b, "CREATE TABLE t2 (x INT)");
    exec(b, "INSERT INTO t1 VALUES (1), (1), (2)");
    exec(b, "INSERT INTO t2 VALUES (1), (1), (1), (2)");
    // INTERSECT ALL: min of counts for each value: 1→min(2,3)=2, 2→min(1,1)=1 → 3 rows
    let n = count_rows(b, "SELECT x FROM t1 INTERSECT ALL SELECT x FROM t2");
    assert!(n >= 2, "INTERSECT ALL should keep min occurrence count");
}

#[test]
fn test_intersect_all_keeps_multiple_copies() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_intersect_all_keeps_multiple_copies_body(&b);
}

crate::net_tests!(test_intersect_all_keeps_multiple_copies);


fn test_except_all_behavior_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t1 (x INT)");
    exec(b, "CREATE TABLE t2 (x INT)");
    exec(b, "INSERT INTO t1 VALUES (1), (1), (2), (3)");
    exec(b, "INSERT INTO t2 VALUES (1)");
    // EXCEPT ALL: removes one copy of 1; result: 1, 2, 3
    let result = exec(b, "SELECT x FROM t1 EXCEPT ALL SELECT x FROM t2 ORDER BY x");
    assert_eq!(result.rows.len(), 3, "One 1 removed, leaving 1, 2, 3");
}

#[test]
fn test_except_all_behavior() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_except_all_behavior_body(&b);
}

crate::net_tests!(test_except_all_behavior);


fn test_union_column_naming_body(b: &crate::common::Backend) {
    // Column name comes from first SELECT
    exec(b, "CREATE TABLE t1 (alpha INT)");
    exec(b, "CREATE TABLE t2 (beta INT)");
    exec(b, "INSERT INTO t1 VALUES (1)");
    exec(b, "INSERT INTO t2 VALUES (2)");
    let result = exec(b, "SELECT alpha FROM t1 UNION SELECT beta FROM t2 ORDER BY alpha");
    assert_eq!(result.rows.len(), 2);
}

#[test]
fn test_union_column_naming() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_column_naming_body(&b);
}

crate::net_tests!(test_union_column_naming);


fn test_union_multi_column_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE p1 (x INT, y TEXT)");
    exec(b, "CREATE TABLE p2 (x INT, y TEXT)");
    exec(b, "INSERT INTO p1 VALUES (1, 'a'), (2, 'b')");
    exec(b, "INSERT INTO p2 VALUES (2, 'b'), (3, 'c')");
    let result = exec(b, "SELECT x, y FROM p1 UNION SELECT x, y FROM p2 ORDER BY x");
    assert_eq!(result.rows.len(), 3, "3 distinct (x,y) pairs");
}

#[test]
fn test_union_multi_column() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_multi_column_body(&b);
}

crate::net_tests!(test_union_multi_column);


fn test_set_ops_nested_parentheses_body(b: &crate::common::Backend) {
    // Nested set operations
    let result = exec(b, "SELECT 1 UNION (SELECT 2 UNION SELECT 3) ORDER BY 1");
    assert_eq!(result.rows.len(), 3);
}

#[test]
fn test_set_ops_nested_parentheses() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_set_ops_nested_parentheses_body(&b);
}

crate::net_tests!(test_set_ops_nested_parentheses);


fn test_union_where_filters_before_union_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE data (cat TEXT, val INT)");
    exec(b, "INSERT INTO data VALUES ('A', 1), ('A', 2), ('B', 10), ('B', 20), ('C', 100)");
    let result = exec(b,
        "SELECT val FROM data WHERE cat = 'A' UNION SELECT val FROM data WHERE cat = 'C' ORDER BY val");
    assert_eq!(result.rows.len(), 3, "A values 1,2 UNION C value 100 = 3 rows");
}

#[test]
fn test_union_where_filters_before_union() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_where_filters_before_union_body(&b);
}

crate::net_tests!(test_union_where_filters_before_union);


fn test_intersect_three_sets_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE s1 (v INT)");
    exec(b, "CREATE TABLE s2 (v INT)");
    exec(b, "CREATE TABLE s3 (v INT)");
    exec(b, "INSERT INTO s1 VALUES (1), (2), (3), (4)");
    exec(b, "INSERT INTO s2 VALUES (2), (3), (4), (5)");
    exec(b, "INSERT INTO s3 VALUES (3), (4), (5), (6)");
    let result = exec(b,
        "SELECT v FROM s1 INTERSECT SELECT v FROM s2 INTERSECT SELECT v FROM s3 ORDER BY v");
    assert_eq!(result.rows.len(), 2, "Common to all three: 3 and 4");
    let vals: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(vals, vec![3, 4]);
}

#[test]
fn test_intersect_three_sets() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_intersect_three_sets_body(&b);
}

crate::net_tests!(test_intersect_three_sets);


fn test_union_with_aggregate_subquery_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE revenue (dept TEXT, amt FLOAT)");
    exec(b, "INSERT INTO revenue VALUES ('eng', 1000.0), ('eng', 2000.0), ('hr', 500.0)");
    let result = exec(b,
        "SELECT dept, SUM(amt) AS total FROM revenue WHERE dept = 'eng' GROUP BY dept \
         UNION \
         SELECT dept, SUM(amt) AS total FROM revenue WHERE dept = 'hr' GROUP BY dept \
         ORDER BY total DESC");
    assert_eq!(result.rows.len(), 2);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "eng"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_union_with_aggregate_subquery() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_union_with_aggregate_subquery_body(&b);
}

crate::net_tests!(test_union_with_aggregate_subquery);


fn test_except_leaves_unique_values_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE all_users (uid INT)");
    exec(b, "CREATE TABLE banned (uid INT)");
    for i in 1..=10 { exec(b, &format!("INSERT INTO all_users VALUES ({})", i)); }
    exec(b, "INSERT INTO banned VALUES (2), (5), (7), (9)");
    let result = exec(b, "SELECT uid FROM all_users EXCEPT SELECT uid FROM banned ORDER BY uid");
    assert_eq!(result.rows.len(), 6, "10 - 4 banned = 6 users");
    let ids: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(ids, vec![1, 3, 4, 6, 8, 10]);
}

#[test]
fn test_except_leaves_unique_values() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_except_leaves_unique_values_body(&b);
}

crate::net_tests!(test_except_leaves_unique_values);

