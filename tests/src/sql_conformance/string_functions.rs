/// String function tests
/// Based on PostgreSQL strings.sql patterns.
use tempfile::TempDir;
use crate::common::{make_engine, exec, query_int};
use sql::Value;

#[test]
fn test_string_concat_pipe() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 'foo' || 'bar'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "foobar"),
        other => panic!("Expected 'foobar', got {:?}", other),
    }
}

#[test]
fn test_string_concat_with_space() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 'hello' || ' ' || 'world'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello world"),
        other => panic!("Expected 'hello world', got {:?}", other),
    }
}

#[test]
fn test_string_concat_with_null() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 'hello' || NULL");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        other => panic!("'hello' || NULL should be NULL, got {:?}", other),
    }
}

#[test]
fn test_string_upper() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT UPPER('hello')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "HELLO"),
        other => panic!("UPPER('hello') = 'HELLO', got {:?}", other),
    }
}

#[test]
fn test_string_lower() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT LOWER('WORLD')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "world"),
        other => panic!("LOWER('WORLD') = 'world', got {:?}", other),
    }
}

#[test]
fn test_string_upper_mixed_case() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT UPPER('Hello World')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "HELLO WORLD"),
        other => panic!("Expected 'HELLO WORLD', got {:?}", other),
    }
}

#[test]
fn test_string_length() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT LENGTH('hello')");
    assert_eq!(n, 5);
}

#[test]
fn test_string_length_empty() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT LENGTH('')");
    assert_eq!(n, 0);
}

#[test]
fn test_string_length_null() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT LENGTH(NULL)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        other => panic!("LENGTH(NULL) should be NULL, got {:?}", other),
    }
}

#[test]
fn test_string_trim_both() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT TRIM('  hello  ')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("TRIM should remove spaces, got {:?}", other),
    }
}

#[test]
#[ignore = "TRIM(BOTH FROM ...) not parsed by sqlparser-rs 0.53"]
fn test_string_trim_both_explicit() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT TRIM(BOTH FROM '  bunch o blanks  ')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "bunch o blanks"),
        other => panic!("TRIM BOTH should remove spaces, got {:?}", other),
    }
}

#[test]
fn test_string_ltrim() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT LTRIM('   hello')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("LTRIM should remove leading spaces, got {:?}", other),
    }
}

#[test]
fn test_string_rtrim() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT RTRIM('hello   ')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("RTRIM should remove trailing spaces, got {:?}", other),
    }
}

#[test]
fn test_string_substring_basic() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT SUBSTRING('hello', 2, 3)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "ell"),
        other => panic!("SUBSTRING('hello', 2, 3) = 'ell', got {:?}", other),
    }
}

#[test]
fn test_string_substring_from_start() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT SUBSTRING('1234567890' FROM 3)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "34567890"),
        other => panic!("SUBSTRING FROM 3 = '34567890', got {:?}", other),
    }
}

#[test]
fn test_string_substring_from_for() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT SUBSTRING('1234567890' FROM 4 FOR 3)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "456"),
        other => panic!("SUBSTRING FROM 4 FOR 3 = '456', got {:?}", other),
    }
}

#[test]
fn test_string_position_in() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT POSITION('lo' IN 'hello')");
    assert_eq!(n, 4, "POSITION('lo' IN 'hello') = 4");
}

#[test]
fn test_string_position_not_found() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let n = query_int(&engine, "SELECT POSITION('xyz' IN 'hello')");
    assert_eq!(n, 0, "POSITION returns 0 when not found");
}

#[test]
fn test_string_replace() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT REPLACE('hello world', 'world', 'earth')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello earth"),
        other => panic!("REPLACE should work, got {:?}", other),
    }
}

#[test]
fn test_string_replace_abcdef() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT REPLACE('abcdef', 'de', '45')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "abc45f"),
        other => panic!("REPLACE('abcdef','de','45') = 'abc45f', got {:?}", other),
    }
}

#[test]
fn test_string_like_prefix() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 'hawkeye' LIKE 'h%'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("'hawkeye' LIKE 'h%%' should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_string_like_suffix() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 'hawkeye' LIKE '%eye'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("LIKE suffix match failed, got {:?}", other),
    }
}

#[test]
fn test_string_like_underscore() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 'indio' LIKE '_ndio'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("LIKE '_ndio' should match 'indio', got {:?}", other),
    }
}

#[test]
fn test_string_not_like() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 'hawkeye' NOT LIKE 'H%'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("NOT LIKE case-sensitive should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_string_ilike_case_insensitive() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 'hawkeye' ILIKE 'H%'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("ILIKE should match case-insensitively, got {:?}", other),
    }
}

#[test]
fn test_string_ilike_mixed_case_pattern() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 'Hello' ILIKE 'hello'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("ILIKE should match 'Hello' ~ 'hello', got {:?}", other),
    }
}

#[test]
fn test_string_comparison_less() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 'abc' < 'abd'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("'abc' < 'abd' should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_string_comparison_equal() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = exec(&engine, "SELECT 'abc' = 'abc'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("'abc' = 'abc' should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_string_in_group_by() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE words (w TEXT)");
    exec(&engine, "INSERT INTO words VALUES ('apple'), ('banana'), ('apple'), ('cherry'), ('banana'), ('apple')");
    let result = exec(&engine, "SELECT w, COUNT(*) AS cnt FROM words GROUP BY w ORDER BY w");
    assert_eq!(result.rows.len(), 3);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "apple"),
        other => panic!("{:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Int4(3)) | Some(Value::Int8(3)) => {}
        other => panic!("apple should appear 3 times, got {:?}", other),
    }
}

#[test]
fn test_string_in_order_by() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE names (name TEXT)");
    exec(&engine, "INSERT INTO names VALUES ('Charlie'), ('Alice'), ('Bob')");
    let result = exec(&engine, "SELECT name FROM names ORDER BY name ASC");
    let names: Vec<String> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(names, vec!["Alice", "Bob", "Charlie"]);
}

#[test]
fn test_string_long_storage() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE long_text (id INT, content TEXT)");
    let long_str = "a".repeat(1000);
    exec(&engine, &format!("INSERT INTO long_text VALUES (1, '{}')", long_str));
    let result = exec(&engine, "SELECT LENGTH(content) FROM long_text WHERE id = 1");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(1000)) | Some(Value::Int8(1000)) => {}
        other => panic!("Expected length=1000, got {:?}", other),
    }
}

#[test]
fn test_string_concat_from_table() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE names2 (first TEXT, last TEXT)");
    exec(&engine, "INSERT INTO names2 VALUES ('John', 'Doe'), ('Jane', 'Smith')");
    let result = exec(&engine, "SELECT first || ' ' || last AS full_name FROM names2 ORDER BY last");
    assert_eq!(result.rows.len(), 2);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "John Doe"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_string_like_table_filter() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE products (name TEXT)");
    exec(&engine, "INSERT INTO products VALUES ('Apple iPhone'), ('Apple iPad'), ('Samsung Galaxy'), ('Google Pixel')");
    let result = exec(&engine, "SELECT name FROM products WHERE name LIKE 'Apple%' ORDER BY name");
    assert_eq!(result.rows.len(), 2);
}

#[test]
fn test_string_upper_in_where() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE mixed_case (id INT, label TEXT)");
    exec(&engine, "INSERT INTO mixed_case VALUES (1, 'Hello'), (2, 'HELLO'), (3, 'World')");
    let result = exec(&engine, "SELECT id FROM mixed_case WHERE UPPER(label) = 'HELLO' ORDER BY id");
    assert_eq!(result.rows.len(), 2, "Both 'Hello' and 'HELLO' should match UPPER = 'HELLO'");
}
