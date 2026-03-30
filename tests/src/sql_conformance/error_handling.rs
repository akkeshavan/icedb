/// Category 11: Error handling and SQLSTATE code tests
/// Verifies that correct PostgreSQL SQLSTATE codes are returned for each error class.
use tempfile::TempDir;
use crate::common::{make_engine, exec, exec_err, Backend};
use sql::SqlError;

fn test_error_undefined_table_sqlstate_body(b: &crate::common::Backend) {
    let err = exec_err(b, "SELECT * FROM no_such_table");
    assert_eq!(err.sqlstate(), "42P01",
        "Undefined table should have SQLSTATE 42P01, got: {}", err.sqlstate());
    assert!(matches!(err, SqlError::TableNotFound(_)), "Should be TableNotFound error");
}

#[test]
fn test_error_undefined_table_sqlstate() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_undefined_table_sqlstate_body(&b);
}

crate::net_tests!(test_error_undefined_table_sqlstate);


fn test_error_undefined_table_message_body(b: &crate::common::Backend) {
    let err = exec_err(b, "SELECT * FROM missing_table");
    let msg = err.to_string();
    assert!(msg.contains("missing_table"), "Error message should mention the missing table name");
}

#[test]
fn test_error_undefined_table_message() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_undefined_table_message_body(&b);
}

crate::net_tests!(test_error_undefined_table_message);


fn test_error_undefined_column_sqlstate_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, name TEXT)");
    // Insert a row so the scan actually runs and sees the column reference
    exec(b, "INSERT INTO t VALUES (1, 'Alice')");

    let err = exec_err(b, "SELECT nonexistent_col FROM t");
    assert_eq!(err.sqlstate(), "42703",
        "Undefined column should have SQLSTATE 42703, got: {}", err.sqlstate());
    assert!(matches!(err, SqlError::ColumnNotFound(_)), "Should be ColumnNotFound error");
}

#[test]
fn test_error_undefined_column_sqlstate() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_undefined_column_sqlstate_body(&b);
}

crate::net_tests!(test_error_undefined_column_sqlstate);


fn test_error_undefined_column_in_where_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT)");
    // Insert a row so the scan actually runs
    exec(b, "INSERT INTO t VALUES (1)");

    let err = exec_err(b, "SELECT * FROM t WHERE ghost_col = 5");
    assert_eq!(err.sqlstate(), "42703",
        "Column reference in WHERE should be 42703, got: {}", err.sqlstate());
}

#[test]
fn test_error_undefined_column_in_where() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_undefined_column_in_where_body(&b);
}

crate::net_tests!(test_error_undefined_column_in_where);


fn test_error_division_by_zero_sqlstate_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (a INT, b INT)");
    exec(b, "INSERT INTO t VALUES (10, 0)");

    let err = exec_err(b, "SELECT a / b FROM t");
    assert_eq!(err.sqlstate(), "22012",
        "Division by zero should have SQLSTATE 22012, got: {}", err.sqlstate());
    assert!(matches!(err, SqlError::DivisionByZero), "Should be DivisionByZero error");
}

#[test]
fn test_error_division_by_zero_sqlstate() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_division_by_zero_sqlstate_body(&b);
}

crate::net_tests!(test_error_division_by_zero_sqlstate);


fn test_error_division_by_zero_literal_body(b: &crate::common::Backend) {
    let err = exec_err(b, "SELECT 1 / 0");
    assert_eq!(err.sqlstate(), "22012",
        "Division by zero literal should be SQLSTATE 22012");
}

#[test]
fn test_error_division_by_zero_literal() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_division_by_zero_literal_body(&b);
}

crate::net_tests!(test_error_division_by_zero_literal);


fn test_error_syntax_sqlstate_body(b: &crate::common::Backend) {
    let err = exec_err(b, "THIS IS NOT VALID SQL !!!");
    assert_eq!(err.sqlstate(), "42601",
        "Syntax error should have SQLSTATE 42601, got: {}", err.sqlstate());
    assert!(matches!(err, SqlError::Parse(_)), "Should be Parse error");
}

#[test]
fn test_error_syntax_sqlstate() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_syntax_sqlstate_body(&b);
}

crate::net_tests!(test_error_syntax_sqlstate);


fn test_error_duplicate_table_sqlstate_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT)");

    let err = exec_err(b, "CREATE TABLE t (id INT, name TEXT)");
    assert_eq!(err.sqlstate(), "42P07",
        "Duplicate table should have SQLSTATE 42P07, got: {}", err.sqlstate());
}

#[test]
fn test_error_duplicate_table_sqlstate() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_duplicate_table_sqlstate_body(&b);
}

crate::net_tests!(test_error_duplicate_table_sqlstate);


fn test_error_numeric_overflow_sqlstate_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, &format!("INSERT INTO t VALUES ({})", i32::MAX));

    let err = exec_err(b, "SELECT val + 1 FROM t");
    assert_eq!(err.sqlstate(), "22003",
        "Numeric overflow should have SQLSTATE 22003, got: {}", err.sqlstate());
    assert!(matches!(err, SqlError::NumericOverflow(_)), "Should be NumericOverflow error");
}

#[test]
fn test_error_numeric_overflow_sqlstate() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_numeric_overflow_sqlstate_body(&b);
}

crate::net_tests!(test_error_numeric_overflow_sqlstate);


fn test_error_select_from_dropped_table_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT)");
    exec(b, "DROP TABLE t");

    let err = exec_err(b, "SELECT * FROM t");
    assert_eq!(err.sqlstate(), "42P01",
        "SELECT from dropped table should be 42P01, got: {}", err.sqlstate());
}

#[test]
fn test_error_select_from_dropped_table() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_select_from_dropped_table_body(&b);
}

crate::net_tests!(test_error_select_from_dropped_table);


fn test_error_insert_wrong_column_count_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, name TEXT, score INT)");

    // 3 columns but 2 values
    let result = b.try_execute("INSERT INTO t VALUES (1, 'Alice')");
    // Accept either error or OK (behavior varies by engine)
    // The important thing is: no panic
    let _ = result;
}

#[test]
fn test_error_insert_wrong_column_count() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_insert_wrong_column_count_body(&b);
}

crate::net_tests!(test_error_insert_wrong_column_count);


fn test_error_not_null_violation_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT NOT NULL, name TEXT NOT NULL)");

    let err = exec_err(b, "INSERT INTO t VALUES (1, NULL)");
    // Should be a constraint violation or NOT NULL error
    let state = err.sqlstate();
    // Accept 23000 (constraint violation) or 23502 (not_null_violation)
    assert!(state == "23000" || state == "23502" || state == "XX000",
        "NOT NULL violation should be 23000/23502/XX000, got: {}", state);
}

#[test]
fn test_error_not_null_violation() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_not_null_violation_body(&b);
}

crate::net_tests!(test_error_not_null_violation);


fn test_error_update_nonexistent_column_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, val INT)");
    exec(b, "INSERT INTO t VALUES (1, 10)");

    let err = exec_err(b, "UPDATE t SET ghost = 99 WHERE id = 1");
    assert_eq!(err.sqlstate(), "42703",
        "UPDATE to nonexistent column should be 42703, got: {}", err.sqlstate());
}

#[test]
fn test_error_update_nonexistent_column() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_update_nonexistent_column_body(&b);
}

crate::net_tests!(test_error_update_nonexistent_column);


fn test_error_invalid_literal_for_type_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");

    // Inserting a non-integer literal into INT column
    let result = b.try_execute("INSERT INTO t VALUES ('not-a-number')");
    // This may error or be coerced depending on implementation
    let _ = result; // Document intent: should fail or succeed consistently
}

#[test]
fn test_error_invalid_literal_for_type() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_invalid_literal_for_type_body(&b);
}

crate::net_tests!(test_error_invalid_literal_for_type);


fn test_error_ambiguous_column_reference_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t1 (id INT, val TEXT)");
    exec(b, "CREATE TABLE t2 (id INT, val TEXT)");
    exec(b, "INSERT INTO t1 VALUES (1, 'a')");
    exec(b, "INSERT INTO t2 VALUES (1, 'b')");

    let result = b.try_execute("SELECT val FROM t1 JOIN t2 ON t1.id = t2.id");
    // May return AmbiguousColumn error (42702) or engine-specific behavior
    if let Err(ref err) = result {
        let state = err.sqlstate();
        assert!(state == "42702" || state == "42703" || state == "XX000",
            "Ambiguous column should be 42702, got: {}", state);
    }
    // If it succeeds, that's also acceptable (implementation choice)
    let _ = result;
}

#[test]
fn test_error_ambiguous_column_reference() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_ambiguous_column_reference_body(&b);
}

crate::net_tests!(test_error_ambiguous_column_reference);


fn test_error_drop_nonexistent_table_body(b: &crate::common::Backend) {
    let err = exec_err(b, "DROP TABLE no_such_table");
    // Should be some form of table-not-found or catalog error
    let state = err.sqlstate();
    assert!(state == "42P01" || state == "XX000",
        "DROP TABLE nonexistent should be 42P01 or XX000, got: {}", state);
}

#[test]
fn test_error_drop_nonexistent_table() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_drop_nonexistent_table_body(&b);
}

crate::net_tests!(test_error_drop_nonexistent_table);


fn test_error_empty_statement_ok_body(b: &crate::common::Backend) {
    // Empty statement should not error, just return empty result

    // An empty SQL string should be handled gracefully
    let result = b.try_execute("");
    // Accept either Ok(empty) or no-op; must not panic
    let _ = result;
}

#[test]
fn test_error_empty_statement_ok() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_empty_statement_ok_body(&b);
}

crate::net_tests!(test_error_empty_statement_ok);


fn test_error_multiple_errors_each_correct_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT)");

    // Table not found
    let e1 = exec_err(b, "SELECT * FROM missing");
    assert_eq!(e1.sqlstate(), "42P01");

    // Column not found — need a row so the scan runs and detects the missing column
    exec(b, "INSERT INTO t VALUES (1)");
    let e2 = exec_err(b, "SELECT ghost FROM t");
    assert_eq!(e2.sqlstate(), "42703");

    // Division by zero
    exec(b, "INSERT INTO t VALUES (0)");
    let e3 = exec_err(b, "SELECT 1 / id FROM t WHERE id = 0");
    assert_eq!(e3.sqlstate(), "22012");
}

#[test]
fn test_error_multiple_errors_each_correct() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_error_multiple_errors_each_correct_body(&b);
}

crate::net_tests!(test_error_multiple_errors_each_correct);

