/// String function tests
/// Based on PostgreSQL strings.sql patterns.
use tempfile::TempDir;
use crate::common::{make_engine, exec, query_int, Backend};
use sql::Value;

fn test_string_concat_pipe_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 'foo' || 'bar'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "foobar"),
        other => panic!("Expected 'foobar', got {:?}", other),
    }
}

#[test]
fn test_string_concat_pipe() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_concat_pipe_body(&b);
}

crate::net_tests!(test_string_concat_pipe);


fn test_string_concat_with_space_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 'hello' || ' ' || 'world'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello world"),
        other => panic!("Expected 'hello world', got {:?}", other),
    }
}

#[test]
fn test_string_concat_with_space() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_concat_with_space_body(&b);
}

crate::net_tests!(test_string_concat_with_space);


fn test_string_concat_with_null_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 'hello' || NULL");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        other => panic!("'hello' || NULL should be NULL, got {:?}", other),
    }
}

#[test]
fn test_string_concat_with_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_concat_with_null_body(&b);
}

crate::net_tests!(test_string_concat_with_null);


fn test_string_upper_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT UPPER('hello')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "HELLO"),
        other => panic!("UPPER('hello') = 'HELLO', got {:?}", other),
    }
}

#[test]
fn test_string_upper() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_upper_body(&b);
}

crate::net_tests!(test_string_upper);


fn test_string_lower_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT LOWER('WORLD')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "world"),
        other => panic!("LOWER('WORLD') = 'world', got {:?}", other),
    }
}

#[test]
fn test_string_lower() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_lower_body(&b);
}

crate::net_tests!(test_string_lower);


fn test_string_upper_mixed_case_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT UPPER('Hello World')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "HELLO WORLD"),
        other => panic!("Expected 'HELLO WORLD', got {:?}", other),
    }
}

#[test]
fn test_string_upper_mixed_case() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_upper_mixed_case_body(&b);
}

crate::net_tests!(test_string_upper_mixed_case);


fn test_string_length_body(b: &crate::common::Backend) {
    let n = query_int(b, "SELECT LENGTH('hello')");
    assert_eq!(n, 5);
}

#[test]
fn test_string_length() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_length_body(&b);
}

crate::net_tests!(test_string_length);


fn test_string_length_empty_body(b: &crate::common::Backend) {
    let n = query_int(b, "SELECT LENGTH('')");
    assert_eq!(n, 0);
}

#[test]
fn test_string_length_empty() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_length_empty_body(&b);
}

crate::net_tests!(test_string_length_empty);


fn test_string_length_null_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT LENGTH(NULL)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) | None => {}
        other => panic!("LENGTH(NULL) should be NULL, got {:?}", other),
    }
}

#[test]
fn test_string_length_null() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_length_null_body(&b);
}

crate::net_tests!(test_string_length_null);


fn test_string_trim_both_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT TRIM('  hello  ')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("TRIM should remove spaces, got {:?}", other),
    }
}

#[test]
fn test_string_trim_both() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_trim_both_body(&b);
}

crate::net_tests!(test_string_trim_both);


fn test_string_trim_both_explicit_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT TRIM(BOTH FROM '  bunch o blanks  ')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "bunch o blanks"),
        other => panic!("TRIM BOTH should remove spaces, got {:?}", other),
    }
}

#[test]
fn test_string_trim_both_explicit() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_trim_both_explicit_body(&b);
}

crate::net_tests!(test_string_trim_both_explicit);


fn test_string_ltrim_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT LTRIM('   hello')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("LTRIM should remove leading spaces, got {:?}", other),
    }
}

#[test]
fn test_string_ltrim() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_ltrim_body(&b);
}

crate::net_tests!(test_string_ltrim);


fn test_string_rtrim_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT RTRIM('hello   ')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("RTRIM should remove trailing spaces, got {:?}", other),
    }
}

#[test]
fn test_string_rtrim() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_rtrim_body(&b);
}

crate::net_tests!(test_string_rtrim);


fn test_string_substring_basic_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT SUBSTRING('hello', 2, 3)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "ell"),
        other => panic!("SUBSTRING('hello', 2, 3) = 'ell', got {:?}", other),
    }
}

#[test]
fn test_string_substring_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_substring_basic_body(&b);
}

crate::net_tests!(test_string_substring_basic);


fn test_string_substring_from_start_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT SUBSTRING('1234567890' FROM 3)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "34567890"),
        other => panic!("SUBSTRING FROM 3 = '34567890', got {:?}", other),
    }
}

#[test]
fn test_string_substring_from_start() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_substring_from_start_body(&b);
}

crate::net_tests!(test_string_substring_from_start);


fn test_string_substring_from_for_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT SUBSTRING('1234567890' FROM 4 FOR 3)");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "456"),
        other => panic!("SUBSTRING FROM 4 FOR 3 = '456', got {:?}", other),
    }
}

#[test]
fn test_string_substring_from_for() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_substring_from_for_body(&b);
}

crate::net_tests!(test_string_substring_from_for);


fn test_string_position_in_body(b: &crate::common::Backend) {
    let n = query_int(b, "SELECT POSITION('lo' IN 'hello')");
    assert_eq!(n, 4, "POSITION('lo' IN 'hello') = 4");
}

#[test]
fn test_string_position_in() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_position_in_body(&b);
}

crate::net_tests!(test_string_position_in);


fn test_string_position_not_found_body(b: &crate::common::Backend) {
    let n = query_int(b, "SELECT POSITION('xyz' IN 'hello')");
    assert_eq!(n, 0, "POSITION returns 0 when not found");
}

#[test]
fn test_string_position_not_found() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_position_not_found_body(&b);
}

crate::net_tests!(test_string_position_not_found);


fn test_string_replace_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT REPLACE('hello world', 'world', 'earth')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello earth"),
        other => panic!("REPLACE should work, got {:?}", other),
    }
}

#[test]
fn test_string_replace() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_replace_body(&b);
}

crate::net_tests!(test_string_replace);


fn test_string_replace_abcdef_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT REPLACE('abcdef', 'de', '45')");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "abc45f"),
        other => panic!("REPLACE('abcdef','de','45') = 'abc45f', got {:?}", other),
    }
}

#[test]
fn test_string_replace_abcdef() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_replace_abcdef_body(&b);
}

crate::net_tests!(test_string_replace_abcdef);


fn test_string_like_prefix_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 'hawkeye' LIKE 'h%'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("'hawkeye' LIKE 'h%%' should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_string_like_prefix() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_like_prefix_body(&b);
}

crate::net_tests!(test_string_like_prefix);


fn test_string_like_suffix_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 'hawkeye' LIKE '%eye'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("LIKE suffix match failed, got {:?}", other),
    }
}

#[test]
fn test_string_like_suffix() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_like_suffix_body(&b);
}

crate::net_tests!(test_string_like_suffix);


fn test_string_like_underscore_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 'indio' LIKE '_ndio'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("LIKE '_ndio' should match 'indio', got {:?}", other),
    }
}

#[test]
fn test_string_like_underscore() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_like_underscore_body(&b);
}

crate::net_tests!(test_string_like_underscore);


fn test_string_not_like_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 'hawkeye' NOT LIKE 'H%'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("NOT LIKE case-sensitive should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_string_not_like() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_not_like_body(&b);
}

crate::net_tests!(test_string_not_like);


fn test_string_ilike_case_insensitive_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 'hawkeye' ILIKE 'H%'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("ILIKE should match case-insensitively, got {:?}", other),
    }
}

#[test]
fn test_string_ilike_case_insensitive() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_ilike_case_insensitive_body(&b);
}

crate::net_tests!(test_string_ilike_case_insensitive);


fn test_string_ilike_mixed_case_pattern_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 'Hello' ILIKE 'hello'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("ILIKE should match 'Hello' ~ 'hello', got {:?}", other),
    }
}

#[test]
fn test_string_ilike_mixed_case_pattern() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_ilike_mixed_case_pattern_body(&b);
}

crate::net_tests!(test_string_ilike_mixed_case_pattern);


fn test_string_comparison_less_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 'abc' < 'abd'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("'abc' < 'abd' should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_string_comparison_less() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_comparison_less_body(&b);
}

crate::net_tests!(test_string_comparison_less);


fn test_string_comparison_equal_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 'abc' = 'abc'");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("'abc' = 'abc' should be TRUE, got {:?}", other),
    }
}

#[test]
fn test_string_comparison_equal() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_comparison_equal_body(&b);
}

crate::net_tests!(test_string_comparison_equal);


fn test_string_in_group_by_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE words (w TEXT)");
    exec(b, "INSERT INTO words VALUES ('apple'), ('banana'), ('apple'), ('cherry'), ('banana'), ('apple')");
    let result = exec(b, "SELECT w, COUNT(*) AS cnt FROM words GROUP BY w ORDER BY w");
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
fn test_string_in_group_by() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_in_group_by_body(&b);
}

crate::net_tests!(test_string_in_group_by);


fn test_string_in_order_by_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE names (name TEXT)");
    exec(b, "INSERT INTO names VALUES ('Charlie'), ('Alice'), ('Bob')");
    let result = exec(b, "SELECT name FROM names ORDER BY name ASC");
    let names: Vec<String> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(names, vec!["Alice", "Bob", "Charlie"]);
}

#[test]
fn test_string_in_order_by() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_in_order_by_body(&b);
}

crate::net_tests!(test_string_in_order_by);


fn test_string_long_storage_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE long_text (id INT, content TEXT)");
    let long_str = "a".repeat(1000);
    exec(b, &format!("INSERT INTO long_text VALUES (1, '{}')", long_str));
    let result = exec(b, "SELECT LENGTH(content) FROM long_text WHERE id = 1");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(1000)) | Some(Value::Int8(1000)) => {}
        other => panic!("Expected length=1000, got {:?}", other),
    }
}

#[test]
fn test_string_long_storage() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_long_storage_body(&b);
}

crate::net_tests!(test_string_long_storage);


fn test_string_concat_from_table_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE names2 (first TEXT, last TEXT)");
    exec(b, "INSERT INTO names2 VALUES ('John', 'Doe'), ('Jane', 'Smith')");
    let result = exec(b, "SELECT first || ' ' || last AS full_name FROM names2 ORDER BY last");
    assert_eq!(result.rows.len(), 2);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "John Doe"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_string_concat_from_table() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_concat_from_table_body(&b);
}

crate::net_tests!(test_string_concat_from_table);


fn test_string_like_table_filter_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE products (name TEXT)");
    exec(b, "INSERT INTO products VALUES ('Apple iPhone'), ('Apple iPad'), ('Samsung Galaxy'), ('Google Pixel')");
    let result = exec(b, "SELECT name FROM products WHERE name LIKE 'Apple%' ORDER BY name");
    assert_eq!(result.rows.len(), 2);
}

#[test]
fn test_string_like_table_filter() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_like_table_filter_body(&b);
}

crate::net_tests!(test_string_like_table_filter);


fn test_string_upper_in_where_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE mixed_case (id INT, label TEXT)");
    exec(b, "INSERT INTO mixed_case VALUES (1, 'Hello'), (2, 'HELLO'), (3, 'World')");
    let result = exec(b, "SELECT id FROM mixed_case WHERE UPPER(label) = 'HELLO' ORDER BY id");
    assert_eq!(result.rows.len(), 2, "Both 'Hello' and 'HELLO' should match UPPER = 'HELLO'");
}

#[test]
fn test_string_upper_in_where() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_string_upper_in_where_body(&b);
}

crate::net_tests!(test_string_upper_in_where);

