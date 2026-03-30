/// LIMIT / OFFSET / FETCH FIRST tests
/// Ported from PostgreSQL limit.sql patterns using self-contained data
/// (the original limit.sql uses `onek` which is an external data file).
use tempfile::TempDir;
use crate::common::{make_engine, exec, count_rows, query_int, Backend};
use sql::Value;

// ── Setup helpers ─────────────────────────────────────────────────────────────

/// Creates a table with 10 rows: id 1..10, val id*10.
fn setup_ten(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, val INT)");
    for i in 1..=10i32 {
        exec(b, &format!("INSERT INTO t VALUES ({}, {})", i, i * 10));
    }
}

fn get_int(v: &Value) -> i64 {
    match v {
        Value::Int4(n) => *n as i64,
        Value::Int8(n) => *n,
        other => panic!("expected integer, got {:?}", other),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// LIMIT 0 returns no rows.
fn test_limit_zero_body(b: &crate::common::Backend) {
    setup_ten(b);
    let n = count_rows(b, "SELECT * FROM t LIMIT 0");
    assert_eq!(n, 0, "LIMIT 0 must return 0 rows");
}

#[test]
fn test_limit_zero() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_limit_zero_body(&b);
}

crate::net_tests!(test_limit_zero);


/// LIMIT 1 returns exactly 1 row.
fn test_limit_one_body(b: &crate::common::Backend) {
    setup_ten(b);
    let n = count_rows(b, "SELECT * FROM t LIMIT 1");
    assert_eq!(n, 1, "LIMIT 1 must return exactly 1 row");
}

#[test]
fn test_limit_one() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_limit_one_body(&b);
}

crate::net_tests!(test_limit_one);


/// LIMIT 5 returns exactly 5 rows.
fn test_limit_five_body(b: &crate::common::Backend) {
    setup_ten(b);
    let n = count_rows(b, "SELECT * FROM t LIMIT 5");
    assert_eq!(n, 5, "LIMIT 5 must return exactly 5 rows");
}

#[test]
fn test_limit_five() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_limit_five_body(&b);
}

crate::net_tests!(test_limit_five);


/// LIMIT greater than row count returns all rows.
fn test_limit_exceeds_row_count_body(b: &crate::common::Backend) {
    setup_ten(b);
    let n = count_rows(b, "SELECT * FROM t LIMIT 1000");
    assert_eq!(n, 10, "LIMIT > rowcount must return all 10 rows");
}

#[test]
fn test_limit_exceeds_row_count() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_limit_exceeds_row_count_body(&b);
}

crate::net_tests!(test_limit_exceeds_row_count);


/// OFFSET 0 is same as no offset (all rows returned).
fn test_offset_zero_same_as_no_offset_body(b: &crate::common::Backend) {
    setup_ten(b);
    let n = count_rows(b, "SELECT * FROM t OFFSET 0");
    assert_eq!(n, 10, "OFFSET 0 must return all 10 rows");
}

#[test]
fn test_offset_zero_same_as_no_offset() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_offset_zero_same_as_no_offset_body(&b);
}

crate::net_tests!(test_offset_zero_same_as_no_offset);


/// OFFSET 5 skips 5 rows, returns 5.
fn test_offset_five_body(b: &crate::common::Backend) {
    setup_ten(b);
    let n = count_rows(b, "SELECT * FROM t OFFSET 5");
    assert_eq!(n, 5, "OFFSET 5 out of 10 rows => 5 rows remain");
}

#[test]
fn test_offset_five() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_offset_five_body(&b);
}

crate::net_tests!(test_offset_five);


/// OFFSET equal to row count returns 0 rows.
fn test_offset_equal_to_row_count_body(b: &crate::common::Backend) {
    setup_ten(b);
    let n = count_rows(b, "SELECT * FROM t OFFSET 10");
    assert_eq!(n, 0, "OFFSET = rowcount must return 0 rows");
}

#[test]
fn test_offset_equal_to_row_count() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_offset_equal_to_row_count_body(&b);
}

crate::net_tests!(test_offset_equal_to_row_count);


/// OFFSET greater than row count returns 0 rows.
fn test_offset_exceeds_row_count_body(b: &crate::common::Backend) {
    setup_ten(b);
    let n = count_rows(b, "SELECT * FROM t OFFSET 100");
    assert_eq!(n, 0, "OFFSET > rowcount must return 0 rows");
}

#[test]
fn test_offset_exceeds_row_count() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_offset_exceeds_row_count_body(&b);
}

crate::net_tests!(test_offset_exceeds_row_count);


/// LIMIT + OFFSET together: skip first N, return at most M.
fn test_limit_and_offset_together_body(b: &crate::common::Backend) {
    setup_ten(b);
    let n = count_rows(b, "SELECT * FROM t LIMIT 3 OFFSET 5");
    assert_eq!(n, 3, "LIMIT 3 OFFSET 5: rows 6,7,8 => 3 rows");
}

#[test]
fn test_limit_and_offset_together() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_limit_and_offset_together_body(&b);
}

crate::net_tests!(test_limit_and_offset_together);


/// LIMIT + OFFSET where remaining rows < LIMIT: returns remaining.
fn test_limit_offset_fewer_remaining_body(b: &crate::common::Backend) {
    setup_ten(b);
    // OFFSET 8 leaves 2 rows; LIMIT 5 can only return 2
    let n = count_rows(b, "SELECT * FROM t LIMIT 5 OFFSET 8");
    assert_eq!(n, 2, "LIMIT 5 OFFSET 8: only 2 rows remain");
}

#[test]
fn test_limit_offset_fewer_remaining() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_limit_offset_fewer_remaining_body(&b);
}

crate::net_tests!(test_limit_offset_fewer_remaining);


/// LIMIT ALL returns all rows (PostgreSQL syntax).
fn test_limit_all_body(b: &crate::common::Backend) {
    setup_ten(b);
    let n = count_rows(b, "SELECT * FROM t LIMIT ALL");
    assert_eq!(n, 10, "LIMIT ALL must return all rows");
}

#[test]
fn test_limit_all() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_limit_all_body(&b);
}

crate::net_tests!(test_limit_all);


/// OFFSET without LIMIT returns all rows after the offset.
fn test_offset_without_limit_body(b: &crate::common::Backend) {
    setup_ten(b);
    let n = count_rows(b, "SELECT * FROM t OFFSET 3");
    assert_eq!(n, 7, "OFFSET 3 without LIMIT returns rows 4..10 (7 rows)");
}

#[test]
fn test_offset_without_limit() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_offset_without_limit_body(&b);
}

crate::net_tests!(test_offset_without_limit);


/// FETCH FIRST 3 ROWS ONLY.
fn test_fetch_first_rows_only_body(b: &crate::common::Backend) {
    setup_ten(b);
    let n = count_rows(b, "SELECT * FROM t FETCH FIRST 3 ROWS ONLY");
    assert_eq!(n, 3, "FETCH FIRST 3 ROWS ONLY must return 3 rows");
}

#[test]
fn test_fetch_first_rows_only() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_fetch_first_rows_only_body(&b);
}

crate::net_tests!(test_fetch_first_rows_only);


/// FETCH FIRST 1 ROW ONLY (singular syntax).
fn test_fetch_first_one_row_only_body(b: &crate::common::Backend) {
    setup_ten(b);
    let n = count_rows(b, "SELECT * FROM t FETCH FIRST 1 ROW ONLY");
    assert_eq!(n, 1, "FETCH FIRST 1 ROW ONLY must return 1 row");
}

#[test]
fn test_fetch_first_one_row_only() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_fetch_first_one_row_only_body(&b);
}

crate::net_tests!(test_fetch_first_one_row_only);


/// FETCH FIRST with OFFSET: skip 2, fetch 3.
fn test_fetch_first_with_offset_body(b: &crate::common::Backend) {
    setup_ten(b);
    let n = count_rows(b, "SELECT * FROM t OFFSET 2 FETCH FIRST 3 ROWS ONLY");
    assert_eq!(n, 3, "OFFSET 2 FETCH FIRST 3: rows 3,4,5 => 3 rows");
}

#[test]
fn test_fetch_first_with_offset() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_fetch_first_with_offset_body(&b);
}

crate::net_tests!(test_fetch_first_with_offset);


/// LIMIT with ORDER BY gives deterministic results.
fn test_limit_with_order_by_deterministic_body(b: &crate::common::Backend) {
    setup_ten(b);

    let result = exec(b, "SELECT id FROM t ORDER BY id ASC LIMIT 3");
    assert_eq!(result.rows.len(), 3, "LIMIT 3 with ORDER BY must return 3 rows");
    // First 3 rows should be id=1, 2, 3
    assert_eq!(get_int(result.rows[0].get_by_idx(0).unwrap()), 1, "First row: id=1");
    assert_eq!(get_int(result.rows[1].get_by_idx(0).unwrap()), 2, "Second row: id=2");
    assert_eq!(get_int(result.rows[2].get_by_idx(0).unwrap()), 3, "Third row: id=3");
}

#[test]
fn test_limit_with_order_by_deterministic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_limit_with_order_by_deterministic_body(&b);
}

crate::net_tests!(test_limit_with_order_by_deterministic);


/// LIMIT with ORDER BY DESC gives last N rows in reverse order.
fn test_limit_order_by_desc_body(b: &crate::common::Backend) {
    setup_ten(b);

    let result = exec(b, "SELECT id FROM t ORDER BY id DESC LIMIT 3");
    assert_eq!(result.rows.len(), 3);
    assert_eq!(get_int(result.rows[0].get_by_idx(0).unwrap()), 10, "First row DESC: id=10");
    assert_eq!(get_int(result.rows[1].get_by_idx(0).unwrap()), 9, "Second row DESC: id=9");
    assert_eq!(get_int(result.rows[2].get_by_idx(0).unwrap()), 8, "Third row DESC: id=8");
}

#[test]
fn test_limit_order_by_desc() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_limit_order_by_desc_body(&b);
}

crate::net_tests!(test_limit_order_by_desc);


/// OFFSET + ORDER BY: skip first 3 sorted rows, verify the rest start at the right value.
fn test_offset_with_order_by_body(b: &crate::common::Backend) {
    setup_ten(b);

    let result = exec(b, "SELECT id FROM t ORDER BY id ASC OFFSET 7");
    assert_eq!(result.rows.len(), 3, "OFFSET 7 of 10 sorted rows => 3 remaining");
    assert_eq!(get_int(result.rows[0].get_by_idx(0).unwrap()), 8, "First remaining id=8");
    assert_eq!(get_int(result.rows[1].get_by_idx(0).unwrap()), 9);
    assert_eq!(get_int(result.rows[2].get_by_idx(0).unwrap()), 10);
}

#[test]
fn test_offset_with_order_by() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_offset_with_order_by_body(&b);
}

crate::net_tests!(test_offset_with_order_by);


/// LIMIT on empty table returns 0 rows without error.
fn test_limit_on_empty_table_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT)");
    let n = count_rows(b, "SELECT * FROM t LIMIT 5");
    assert_eq!(n, 0, "LIMIT on empty table returns 0 rows");
}

#[test]
fn test_limit_on_empty_table() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_limit_on_empty_table_body(&b);
}

crate::net_tests!(test_limit_on_empty_table);


/// LIMIT 1 OFFSET 0 returns exactly the first row.
fn test_limit_1_offset_0_first_row_body(b: &crate::common::Backend) {
    setup_ten(b);
    let result = exec(b, "SELECT id FROM t ORDER BY id ASC LIMIT 1 OFFSET 0");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(get_int(result.rows[0].get_by_idx(0).unwrap()), 1, "First row (LIMIT 1 OFFSET 0) is id=1");
}

#[test]
fn test_limit_1_offset_0_first_row() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_limit_1_offset_0_first_row_body(&b);
}

crate::net_tests!(test_limit_1_offset_0_first_row);


/// Page-by-page pagination pattern using LIMIT + OFFSET.
fn test_pagination_limit_offset_body(b: &crate::common::Backend) {
    setup_ten(b);

    // Page 1: rows 1-3
    let p1 = exec(b, "SELECT id FROM t ORDER BY id LIMIT 3 OFFSET 0");
    assert_eq!(p1.rows.len(), 3);
    assert_eq!(get_int(p1.rows[0].get_by_idx(0).unwrap()), 1);

    // Page 2: rows 4-6
    let p2 = exec(b, "SELECT id FROM t ORDER BY id LIMIT 3 OFFSET 3");
    assert_eq!(p2.rows.len(), 3);
    assert_eq!(get_int(p2.rows[0].get_by_idx(0).unwrap()), 4);

    // Page 3: rows 7-9
    let p3 = exec(b, "SELECT id FROM t ORDER BY id LIMIT 3 OFFSET 6");
    assert_eq!(p3.rows.len(), 3);
    assert_eq!(get_int(p3.rows[0].get_by_idx(0).unwrap()), 7);

    // Page 4: row 10 only
    let p4 = exec(b, "SELECT id FROM t ORDER BY id LIMIT 3 OFFSET 9");
    assert_eq!(p4.rows.len(), 1);
    assert_eq!(get_int(p4.rows[0].get_by_idx(0).unwrap()), 10);
}

#[test]
fn test_pagination_limit_offset() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_pagination_limit_offset_body(&b);
}

crate::net_tests!(test_pagination_limit_offset);


/// NULL LIMIT should return all rows or produce an error — document icedb behavior.
fn test_null_limit_behavior_body(b: &crate::common::Backend) {
    setup_ten(b);
    // PostgreSQL: LIMIT NULL is treated as LIMIT ALL (return all rows)
    let result = b.try_execute("SELECT * FROM t LIMIT NULL");
    match result {
        Ok(r) => {
            // Engine treats NULL limit as no limit
            assert_eq!(r.rows.len(), 10, "LIMIT NULL should return all rows (like LIMIT ALL)");
        }
        Err(_) => {
            // Also acceptable: engine rejects NULL as limit value
        }
    }
}

#[test]
fn test_null_limit_behavior() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_null_limit_behavior_body(&b);
}

crate::net_tests!(test_null_limit_behavior);


/// LIMIT with a subquery/aggregation still applies limit to final output.
fn test_limit_on_aggregate_result_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (grp TEXT, val INT)");
    exec(b, "INSERT INTO t VALUES ('a', 1)");
    exec(b, "INSERT INTO t VALUES ('b', 2)");
    exec(b, "INSERT INTO t VALUES ('c', 3)");
    exec(b, "INSERT INTO t VALUES ('d', 4)");
    exec(b, "INSERT INTO t VALUES ('e', 5)");

    let result = exec(b, "SELECT grp, SUM(val) FROM t GROUP BY grp ORDER BY grp LIMIT 3");
    assert_eq!(result.rows.len(), 3, "LIMIT 3 on GROUP BY result should return 3 groups");
}

#[test]
fn test_limit_on_aggregate_result() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_limit_on_aggregate_result_body(&b);
}

crate::net_tests!(test_limit_on_aggregate_result);


/// FETCH FIRST 0 ROWS ONLY returns empty result.
fn test_fetch_first_zero_rows_body(b: &crate::common::Backend) {
    setup_ten(b);
    let n = count_rows(b, "SELECT * FROM t FETCH FIRST 0 ROWS ONLY");
    assert_eq!(n, 0, "FETCH FIRST 0 ROWS ONLY must return 0 rows");
}

#[test]
fn test_fetch_first_zero_rows() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_fetch_first_zero_rows_body(&b);
}

crate::net_tests!(test_fetch_first_zero_rows);


/// LIMIT in subquery only limits the subquery, not the outer.
fn test_limit_in_subquery_body(b: &crate::common::Backend) {
    setup_ten(b);

    // Subquery selects top-3 ids; outer counts them
    let result = exec(b, "SELECT COUNT(*) FROM (SELECT id FROM t ORDER BY id LIMIT 3) AS sub");
    let n = match result.rows[0].get_by_idx(0).unwrap() {
        Value::Int8(v) => *v,
        Value::Int4(v) => *v as i64,
        other => panic!("expected count, got {:?}", other),
    };
    assert_eq!(n, 3, "Subquery LIMIT 3 should produce 3 rows for outer COUNT");
}

#[test]
fn test_limit_in_subquery() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_limit_in_subquery_body(&b);
}

crate::net_tests!(test_limit_in_subquery);


/// LIMIT with WHERE clause: limit applies after filtering.
fn test_limit_with_where_clause_body(b: &crate::common::Backend) {
    setup_ten(b);

    // Filter val > 50 (id > 5) => 5 rows; LIMIT 2 => 2 rows
    let n = count_rows(b, "SELECT * FROM t WHERE val > 50 ORDER BY id LIMIT 2");
    assert_eq!(n, 2, "LIMIT 2 after WHERE val > 50 must return 2 rows");
}

#[test]
fn test_limit_with_where_clause() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_limit_with_where_clause_body(&b);
}

crate::net_tests!(test_limit_with_where_clause);

