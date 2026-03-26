/// Tests for Array and JSON/JSONB types.
use tempfile::TempDir;
use crate::common::{make_engine, exec, count_rows, query_int};
use sql::Value;

// ── Array type tests ───────────────────────────────────────────────────────────

#[test]
fn test_array_create_and_insert() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE tags (id INT, labels TEXT[])");
    exec(&engine, "INSERT INTO tags VALUES (1, ARRAY['foo','bar','baz'])");
    let result = exec(&engine, "SELECT labels FROM tags WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Array(v)) => {
            assert_eq!(v.len(), 3, "expected 3 elements");
            assert_eq!(v[0], Value::Text("foo".to_string()));
            assert_eq!(v[1], Value::Text("bar".to_string()));
            assert_eq!(v[2], Value::Text("baz".to_string()));
        }
        other => panic!("expected Array, got {:?}", other),
    }
}

#[test]
fn test_array_length() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE tags (id INT, labels TEXT[])");
    exec(&engine, "INSERT INTO tags VALUES (1, ARRAY['foo','bar','baz'])");
    let result = exec(&engine, "SELECT array_length(labels, 1) FROM tags WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(3)) => {}
        other => panic!("expected 3, got {:?}", other),
    }
}

#[test]
fn test_array_subscript() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE tags (id INT, labels TEXT[])");
    exec(&engine, "INSERT INTO tags VALUES (1, ARRAY['foo','bar','baz'])");
    let result = exec(&engine, "SELECT labels[1] FROM tags WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "foo"),
        other => panic!("expected 'foo', got {:?}", other),
    }
}

#[test]
fn test_array_to_string() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE tags (id INT, labels TEXT[])");
    exec(&engine, "INSERT INTO tags VALUES (1, ARRAY['foo','bar','baz'])");
    let result = exec(&engine, "SELECT array_to_string(labels, ',') FROM tags WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "foo,bar,baz"),
        other => panic!("expected 'foo,bar,baz', got {:?}", other),
    }
}

#[test]
fn test_array_agg() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE nums (v INT)");
    exec(&engine, "INSERT INTO nums VALUES (1), (2), (3)");
    let result = exec(&engine, "SELECT array_agg(v) FROM nums");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Array(v)) => {
            assert_eq!(v.len(), 3, "expected 3 elements in array_agg result");
        }
        other => panic!("expected Array, got {:?}", other),
    }
}

#[test]
fn test_array_int() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE arr_test (id INT, vals INT[])");
    exec(&engine, "INSERT INTO arr_test VALUES (1, ARRAY[10, 20, 30])");
    let result = exec(&engine, "SELECT array_length(vals, 1) FROM arr_test WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(3)) => {}
        other => panic!("expected 3, got {:?}", other),
    }
}

#[test]
fn test_array_persistence() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE tags (id INT, labels TEXT[])");
    exec(&engine, "INSERT INTO tags VALUES (1, ARRAY['foo','bar'])");
    exec(&engine, "INSERT INTO tags VALUES (2, ARRAY['baz'])");
    let result = exec(&engine, "SELECT id, array_length(labels, 1) FROM tags ORDER BY id");
    assert_eq!(result.rows.len(), 2);
    match result.rows[0].get_by_idx(1) {
        Some(Value::Int4(2)) => {}
        other => panic!("expected array_length=2, got {:?}", other),
    }
    match result.rows[1].get_by_idx(1) {
        Some(Value::Int4(1)) => {}
        other => panic!("expected array_length=1, got {:?}", other),
    }
}

// ── JSON type tests ────────────────────────────────────────────────────────────

#[test]
fn test_json_create_and_insert() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE docs (id INT, data JSON)");
    exec(&engine, "INSERT INTO docs VALUES (1, '{\"name\":\"Alice\",\"age\":30}')");
    let result = exec(&engine, "SELECT data FROM docs WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Json(s)) => {
            assert!(s.contains("Alice"), "expected Alice in json: {}", s);
        }
        other => panic!("expected Json, got {:?}", other),
    }
}

#[test]
fn test_json_arrow_operator() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE docs (id INT, data JSON)");
    exec(&engine, "INSERT INTO docs VALUES (1, '{\"name\":\"Alice\",\"age\":30}')");
    let result = exec(&engine, "SELECT data->>'name' FROM docs WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "Alice"),
        other => panic!("expected 'Alice', got {:?}", other),
    }
}

#[test]
fn test_json_extract_path_text() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE docs (id INT, data JSON)");
    exec(&engine, "INSERT INTO docs VALUES (1, '{\"name\":\"Alice\",\"age\":30}')");
    let result = exec(&engine, "SELECT json_extract_path_text(data, 'name') FROM docs WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "Alice"),
        other => panic!("expected 'Alice', got {:?}", other),
    }
}

#[test]
fn test_json_typeof() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE docs (id INT, data JSON)");
    exec(&engine, "INSERT INTO docs VALUES (1, '{\"name\":\"Alice\",\"age\":30}')");
    let result = exec(&engine, "SELECT json_typeof(data) FROM docs WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "object"),
        other => panic!("expected 'object', got {:?}", other),
    }
}

#[test]
fn test_jsonb_type() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE docs (id INT, data JSONB)");
    exec(&engine, "INSERT INTO docs VALUES (1, '{\"x\":1}')");
    let result = exec(&engine, "SELECT data->>'x' FROM docs WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "1"),
        other => panic!("expected '1', got {:?}", other),
    }
}

#[test]
fn test_json_build_object() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT json_build_object('name', 'Bob', 'age', 25)");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Json(s)) => {
            // The json string should contain "Bob"
            assert!(s.contains("Bob"), "expected 'Bob' in json_build_object result: {}", s);
        }
        other => panic!("expected Json, got {:?}", other),
    }
}

#[test]
fn test_json_persistence() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE docs (id INT, data JSON)");
    exec(&engine, "INSERT INTO docs VALUES (1, '{\"name\":\"Alice\"}')");
    exec(&engine, "INSERT INTO docs VALUES (2, '{\"name\":\"Bob\"}')");
    let result = exec(&engine, "SELECT data->>'name' FROM docs ORDER BY id");
    assert_eq!(result.rows.len(), 2);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "Alice"),
        other => panic!("expected 'Alice', got {:?}", other),
    }
    match result.rows[1].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "Bob"),
        other => panic!("expected 'Bob', got {:?}", other),
    }
}
