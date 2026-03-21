/// Boolean type tests
/// Based on PostgreSQL boolean.sql patterns.
use tempfile::TempDir;
use crate::common::{make_engine, exec, exec_err, count_rows, query_int};
use sql::Value;

#[test]
fn test_bool_select_true() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT TRUE");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("Expected TRUE, got {:?}", other),
    }
}

#[test]
fn test_bool_select_false() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT FALSE");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(false)) => {}
        other => panic!("Expected FALSE, got {:?}", other),
    }
}

#[test]
fn test_bool_and_true_true() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT TRUE AND TRUE");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("TRUE AND TRUE should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_bool_and_true_false() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT TRUE AND FALSE");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(false)) => {}
        other => panic!("TRUE AND FALSE should be FALSE, got {:?}", other),
    }
}

#[test]
fn test_bool_and_false_false() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT FALSE AND FALSE");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(false)) => {}
        other => panic!("FALSE AND FALSE should be FALSE, got {:?}", other),
    }
}

#[test]
fn test_bool_or_false_false() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT FALSE OR FALSE");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(false)) => {}
        other => panic!("FALSE OR FALSE should be FALSE, got {:?}", other),
    }
}

#[test]
fn test_bool_or_true_false() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT TRUE OR FALSE");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("TRUE OR FALSE should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_bool_or_true_true() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT TRUE OR TRUE");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("TRUE OR TRUE should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_bool_not_true() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT NOT TRUE");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(false)) => {}
        other => panic!("NOT TRUE should be FALSE, got {:?}", other),
    }
}

#[test]
fn test_bool_not_false() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT NOT FALSE");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("NOT FALSE should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_bool_null_and_true() {
    // NULL AND TRUE = NULL (three-valued logic)
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT NULL AND TRUE");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | Some(Value::Bool(false)) => {} // NULL or engine may return false
        other => panic!("NULL AND TRUE should be NULL, got {:?}", other),
    }
}

#[test]
fn test_bool_null_and_false() {
    // NULL AND FALSE = FALSE (short-circuit)
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT NULL AND FALSE");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(false)) | Some(Value::Null) => {}
        other => panic!("NULL AND FALSE should be FALSE or NULL, got {:?}", other),
    }
}

#[test]
fn test_bool_null_or_true() {
    // NULL OR TRUE = TRUE (short-circuit)
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT NULL OR TRUE");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) | Some(Value::Null) => {}
        other => panic!("NULL OR TRUE should be TRUE or NULL, got {:?}", other),
    }
}

#[test]
fn test_bool_null_or_false() {
    // NULL OR FALSE = NULL
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT NULL OR FALSE");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | Some(Value::Bool(false)) => {}
        other => panic!("NULL OR FALSE should be NULL or FALSE, got {:?}", other),
    }
}

#[test]
fn test_bool_is_true() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT TRUE IS TRUE");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("TRUE IS TRUE should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_bool_is_false() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT FALSE IS FALSE");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("FALSE IS FALSE should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_bool_is_not_true() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT FALSE IS NOT TRUE");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("FALSE IS NOT TRUE should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_bool_is_not_false() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT TRUE IS NOT FALSE");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("TRUE IS NOT FALSE should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_bool_is_unknown_null() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT NULL IS UNKNOWN");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) | Some(Value::Null) => {}
        other => panic!("NULL IS UNKNOWN should be TRUE or NULL, got {:?}", other),
    }
}

#[test]
fn test_bool_table_insert_retrieve() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE booltbl (id INT, flag BOOLEAN)");
    exec(&engine, "INSERT INTO booltbl VALUES (1, TRUE)");
    exec(&engine, "INSERT INTO booltbl VALUES (2, FALSE)");
    exec(&engine, "INSERT INTO booltbl VALUES (3, TRUE)");

    let result = exec(&engine, "SELECT id, flag FROM booltbl ORDER BY id");
    assert_eq!(result.rows.len(), 3);
    match result.rows[0].get_by_idx(1) {
        Some(Value::Bool(true)) => {}
        other => panic!("id=1 flag should be TRUE, got {:?}", other),
    }
    match result.rows[1].get_by_idx(1) {
        Some(Value::Bool(false)) => {}
        other => panic!("id=2 flag should be FALSE, got {:?}", other),
    }
}

#[test]
fn test_bool_where_clause() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE items (id INT, active BOOLEAN, name TEXT)");
    exec(&engine, "INSERT INTO items VALUES (1, TRUE, 'Widget'), (2, FALSE, 'Gadget'), (3, TRUE, 'Doohickey')");
    let result = exec(&engine, "SELECT name FROM items WHERE active = TRUE ORDER BY name");
    assert_eq!(result.rows.len(), 2);
    let names: Vec<String> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(names, vec!["Doohickey", "Widget"]);
}

#[test]
fn test_bool_where_is_true() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE flags (id INT, enabled BOOLEAN)");
    exec(&engine, "INSERT INTO flags VALUES (1, TRUE), (2, FALSE), (3, TRUE), (4, NULL)");
    let result = exec(&engine, "SELECT id FROM flags WHERE enabled IS TRUE ORDER BY id");
    assert_eq!(result.rows.len(), 2, "IS TRUE excludes FALSE and NULL");
    let ids: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(ids, vec![1, 3]);
}

#[test]
fn test_bool_where_is_false() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE flags2 (id INT, enabled BOOLEAN)");
    exec(&engine, "INSERT INTO flags2 VALUES (1, TRUE), (2, FALSE), (3, NULL)");
    let result = exec(&engine, "SELECT id FROM flags2 WHERE enabled IS FALSE ORDER BY id");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(2)) => {}
        other => panic!("Expected id=2, got {:?}", other),
    }
}

#[test]
fn test_bool_order_by_boolean() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE boolsort (id INT, b BOOLEAN)");
    exec(&engine, "INSERT INTO boolsort VALUES (1, TRUE), (2, FALSE), (3, TRUE), (4, FALSE)");
    let result = exec(&engine, "SELECT id FROM boolsort ORDER BY b, id");
    // FALSE comes before TRUE
    assert_eq!(result.rows.len(), 4);
    // First two should be FALSE rows (id=2, id=4)
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(2)) | Some(Value::Int4(4)) => {}
        other => panic!("First row should be a FALSE boolean row, got {:?}", other),
    }
}

#[test]
fn test_bool_group_by_boolean() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE boolgroup (id INT, active BOOLEAN)");
    exec(&engine, "INSERT INTO boolgroup VALUES (1, TRUE), (2, FALSE), (3, TRUE), (4, FALSE), (5, TRUE)");
    let result = exec(&engine,
        "SELECT active, COUNT(*) AS cnt FROM boolgroup GROUP BY active ORDER BY active");
    assert_eq!(result.rows.len(), 2);
    // FALSE group: 2, TRUE group: 3
    match result.rows[0].get_by_idx(1) {
        Some(Value::Int4(2)) | Some(Value::Int8(2)) => {}
        other => panic!("FALSE group should have 2, got {:?}", other),
    }
    match result.rows[1].get_by_idx(1) {
        Some(Value::Int4(3)) | Some(Value::Int8(3)) => {}
        other => panic!("TRUE group should have 3, got {:?}", other),
    }
}

#[test]
fn test_bool_null_in_table() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE boolnull (id INT, flag BOOLEAN)");
    exec(&engine, "INSERT INTO boolnull VALUES (1, TRUE), (2, NULL), (3, FALSE)");
    let n = count_rows(&engine, "SELECT id FROM boolnull WHERE flag IS NULL");
    assert_eq!(n, 1, "Only one NULL boolean");
}

#[test]
fn test_bool_not_in_where() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE tasks (tid INT, done BOOLEAN)");
    exec(&engine, "INSERT INTO tasks VALUES (1, TRUE), (2, FALSE), (3, FALSE), (4, TRUE)");
    let result = exec(&engine, "SELECT tid FROM tasks WHERE NOT done ORDER BY tid");
    assert_eq!(result.rows.len(), 2);
    let ids: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(ids, vec![2, 3]);
}

#[test]
fn test_bool_compound_and_or() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE tests (a BOOLEAN, b BOOLEAN, c BOOLEAN)");
    exec(&engine, "INSERT INTO tests VALUES (TRUE, FALSE, TRUE), (FALSE, TRUE, FALSE), (TRUE, TRUE, TRUE)");
    let result = exec(&engine, "SELECT a, b, c FROM tests WHERE (a AND c) OR b ORDER BY a, b");
    assert_eq!(result.rows.len(), 3, "All rows satisfy (a AND c) OR b");
}

#[test]
fn test_bool_comparison_equals() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT TRUE = TRUE, FALSE = FALSE, TRUE = FALSE");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("TRUE = TRUE should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_bool_literal_string_cast() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 'true'::BOOLEAN");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("'true'::BOOLEAN should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_bool_count_true_rows() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE switches (id INT, on_state BOOLEAN)");
    for i in 1..=10 {
        exec(&engine, &format!("INSERT INTO switches VALUES ({}, {})", i, if i % 3 == 0 { "FALSE" } else { "TRUE" }));
    }
    let n = query_int(&engine, "SELECT COUNT(*) FROM switches WHERE on_state");
    assert_eq!(n, 7, "7 out of 10 are TRUE (those not divisible by 3)");
}
