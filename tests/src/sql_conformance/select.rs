/// Category 1: Basic SELECT tests
/// Based on PostgreSQL select.sql patterns.
use tempfile::TempDir;
use crate::common::{make_engine, exec, count_rows, query_int, Backend};
use sql::Value;

fn setup_products(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE products (id INT, name TEXT, category TEXT, price FLOAT, quantity INT, active INT)");
    exec(b, "INSERT INTO products VALUES (1, 'Apple', 'fruit', 1.50, 100, 1)");
    exec(b, "INSERT INTO products VALUES (2, 'Banana', 'fruit', 0.75, 200, 1)");
    exec(b, "INSERT INTO products VALUES (3, 'Carrot', 'veggie', 0.50, 150, 1)");
    exec(b, "INSERT INTO products VALUES (4, 'Durian', 'fruit', 15.00, 10, 0)");
    exec(b, "INSERT INTO products VALUES (5, 'Eggplant', 'veggie', 2.00, 80, 1)");
}

fn test_select_literal_int_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 42");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(42)) => {}
        other => panic!("expected Int4(42), got {:?}", other),
    }
}

#[test]
fn test_select_literal_int() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_literal_int_body(&b);
}

crate::net_tests!(test_select_literal_int);


fn test_select_literal_text_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 'hello'");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("expected Text('hello'), got {:?}", other),
    }
}

#[test]
fn test_select_literal_text() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_literal_text_body(&b);
}

crate::net_tests!(test_select_literal_text);


fn test_select_literal_bool_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT true");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("expected Bool(true), got {:?}", other),
    }
}

#[test]
fn test_select_literal_bool() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_literal_bool_body(&b);
}

crate::net_tests!(test_select_literal_bool);


fn test_select_literal_arithmetic_body(b: &crate::common::Backend) {
    let result = exec(b, "SELECT 1 + 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(2)) => {}
        other => panic!("expected Int4(2), got {:?}", other),
    }
}

#[test]
fn test_select_literal_arithmetic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_literal_arithmetic_body(&b);
}

crate::net_tests!(test_select_literal_arithmetic);


fn test_select_all_columns_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (a INT, b TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'x')");
    let result = exec(b, "SELECT * FROM t");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].values.len(), 2);
}

#[test]
fn test_select_all_columns() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_all_columns_body(&b);
}

crate::net_tests!(test_select_all_columns);


fn test_select_named_columns_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (a INT, b TEXT, c FLOAT)");
    exec(b, "INSERT INTO t VALUES (1, 'hello', 3.14)");
    let result = exec(b, "SELECT a, b FROM t");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].values.len(), 2);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(1)) => {}
        other => panic!("expected Int4(1), got {:?}", other),
    }
}

#[test]
fn test_select_named_columns() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_named_columns_body(&b);
}

crate::net_tests!(test_select_named_columns);


fn test_select_column_alias_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (name TEXT)");
    exec(b, "INSERT INTO t VALUES ('Alice')");
    let result = exec(b, "SELECT name AS n FROM t");
    assert_eq!(result.rows.len(), 1);
    // Check by index since alias naming may vary
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "Alice"),
        other => panic!("expected Text('Alice'), got {:?}", other),
    }
}

#[test]
fn test_select_column_alias() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_column_alias_body(&b);
}

crate::net_tests!(test_select_column_alias);


fn test_select_arithmetic_expressions_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (price FLOAT, quantity INT)");
    exec(b, "INSERT INTO t VALUES (9.99, 3)");
    let result = exec(b, "SELECT price * quantity AS total FROM t");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => {
            let expected = 9.99 * 3.0;
            assert!((v - expected).abs() < 0.001, "expected ~{}, got {}", expected, v);
        }
        other => panic!("expected Float8, got {:?}", other),
    }
}

#[test]
fn test_select_arithmetic_expressions() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_arithmetic_expressions_body(&b);
}

crate::net_tests!(test_select_arithmetic_expressions);


fn test_select_where_equals_body(b: &crate::common::Backend) {
    setup_products(b);
    let result = exec(b, "SELECT name FROM products WHERE id = 1");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "Apple"),
        other => panic!("expected Apple, got {:?}", other),
    }
}

#[test]
fn test_select_where_equals() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_where_equals_body(&b);
}

crate::net_tests!(test_select_where_equals);


fn test_select_where_comparison_gt_body(b: &crate::common::Backend) {
    setup_products(b);
    // price > 1.0: Apple(1.50), Durian(15.00), Eggplant(2.00) = 3 rows
    let n = count_rows(b, "SELECT name FROM products WHERE price > 1.0");
    assert_eq!(n, 3, "expected 3 products with price > 1.0");
}

#[test]
fn test_select_where_comparison_gt() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_where_comparison_gt_body(&b);
}

crate::net_tests!(test_select_where_comparison_gt);


fn test_select_where_comparison_lt_body(b: &crate::common::Backend) {
    setup_products(b);
    // price < 1.0: Banana(0.75), Carrot(0.50) = 2 rows
    let n = count_rows(b, "SELECT name FROM products WHERE price < 1.0");
    assert_eq!(n, 2, "expected 2 products with price < 1.0");
}

#[test]
fn test_select_where_comparison_lt() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_where_comparison_lt_body(&b);
}

crate::net_tests!(test_select_where_comparison_lt);


fn test_select_where_comparison_gte_body(b: &crate::common::Backend) {
    setup_products(b);
    // price >= 1.50: Apple(1.50), Durian(15.00), Eggplant(2.00) = 3 rows
    let n = count_rows(b, "SELECT name FROM products WHERE price >= 1.50");
    assert_eq!(n, 3, "expected 3 products with price >= 1.50");
}

#[test]
fn test_select_where_comparison_gte() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_where_comparison_gte_body(&b);
}

crate::net_tests!(test_select_where_comparison_gte);


fn test_select_where_comparison_lte_body(b: &crate::common::Backend) {
    setup_products(b);
    // price <= 1.50: Apple(1.50), Banana(0.75), Carrot(0.50) = 3 rows
    let n = count_rows(b, "SELECT name FROM products WHERE price <= 1.50");
    assert_eq!(n, 3, "expected 3 products with price <= 1.50");
}

#[test]
fn test_select_where_comparison_lte() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_where_comparison_lte_body(&b);
}

crate::net_tests!(test_select_where_comparison_lte);


fn test_select_where_comparison_neq_body(b: &crate::common::Backend) {
    setup_products(b);
    // id <> 1: 4 rows
    let n = count_rows(b, "SELECT name FROM products WHERE id <> 1");
    assert_eq!(n, 4, "expected 4 products with id <> 1");
}

#[test]
fn test_select_where_comparison_neq() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_where_comparison_neq_body(&b);
}

crate::net_tests!(test_select_where_comparison_neq);


fn test_select_where_and_body(b: &crate::common::Backend) {
    setup_products(b);
    // category = 'fruit' AND price < 5.0: Apple(1.50), Banana(0.75) = 2 rows
    let n = count_rows(b,
        "SELECT name FROM products WHERE category = 'fruit' AND price < 5.0");
    assert_eq!(n, 2, "expected 2 cheap fruits");
}

#[test]
fn test_select_where_and() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_where_and_body(&b);
}

crate::net_tests!(test_select_where_and);


fn test_select_where_or_body(b: &crate::common::Backend) {
    setup_products(b);
    // id = 1 OR id = 5: 2 rows
    let n = count_rows(b, "SELECT name FROM products WHERE id = 1 OR id = 5");
    assert_eq!(n, 2);
}

#[test]
fn test_select_where_or() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_where_or_body(&b);
}

crate::net_tests!(test_select_where_or);


fn test_select_where_not_body(b: &crate::common::Backend) {
    setup_products(b);
    // NOT (category = 'fruit'): 2 rows (veggies)
    let n = count_rows(b, "SELECT name FROM products WHERE NOT (category = 'fruit')");
    assert_eq!(n, 2, "expected 2 non-fruit products");
}

#[test]
fn test_select_where_not() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_where_not_body(&b);
}

crate::net_tests!(test_select_where_not);


fn test_select_where_between_body(b: &crate::common::Backend) {
    setup_products(b);
    // id BETWEEN 2 AND 4: 3 rows
    let n = count_rows(b, "SELECT name FROM products WHERE id BETWEEN 2 AND 4");
    assert_eq!(n, 3, "expected 3 products with id BETWEEN 2 AND 4");
}

#[test]
fn test_select_where_between() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_where_between_body(&b);
}

crate::net_tests!(test_select_where_between);


fn test_select_where_like_prefix_body(b: &crate::common::Backend) {
    setup_products(b);
    // name LIKE 'B%': Banana
    let n = count_rows(b, "SELECT name FROM products WHERE name LIKE 'B%'");
    assert_eq!(n, 1, "LIKE 'B%' should match Banana only");
}

#[test]
fn test_select_where_like_prefix() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_where_like_prefix_body(&b);
}

crate::net_tests!(test_select_where_like_prefix);


fn test_select_where_like_suffix_body(b: &crate::common::Backend) {
    setup_products(b);
    // name LIKE '%t': Eggplant, Carrot — any ending in 't'
    let n = count_rows(b, "SELECT name FROM products WHERE name LIKE '%t'");
    assert_eq!(n, 2, "LIKE '%t' should match Eggplant and Carrot");
}

#[test]
fn test_select_where_like_suffix() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_where_like_suffix_body(&b);
}

crate::net_tests!(test_select_where_like_suffix);


fn test_select_where_like_contains_body(b: &crate::common::Backend) {
    setup_products(b);
    // name LIKE '%an%': Banana, Durian, Eggplant
    let n = count_rows(b, "SELECT name FROM products WHERE name LIKE '%an%'");
    assert_eq!(n, 3, "LIKE '%an%' should match Banana, Durian, Eggplant");
}

#[test]
fn test_select_where_like_contains() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_where_like_contains_body(&b);
}

crate::net_tests!(test_select_where_like_contains);


fn test_select_where_ilike_body(b: &crate::common::Backend) {
    setup_products(b);
    // name ILIKE 'apple': case-insensitive match
    let n = count_rows(b, "SELECT name FROM products WHERE name ILIKE 'apple'");
    assert_eq!(n, 1, "ILIKE should match Apple case-insensitively");
}

#[test]
fn test_select_where_ilike() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_where_ilike_body(&b);
}

crate::net_tests!(test_select_where_ilike);


fn test_select_where_in_list_body(b: &crate::common::Backend) {
    setup_products(b);
    let n = count_rows(b, "SELECT name FROM products WHERE id IN (1, 3, 5)");
    assert_eq!(n, 3, "IN (1,3,5) should match 3 rows");
}

#[test]
fn test_select_where_in_list() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_where_in_list_body(&b);
}

crate::net_tests!(test_select_where_in_list);


fn test_select_where_not_in_list_body(b: &crate::common::Backend) {
    setup_products(b);
    let n = count_rows(b, "SELECT name FROM products WHERE id NOT IN (1, 2)");
    assert_eq!(n, 3, "NOT IN (1,2) should return 3 rows");
}

#[test]
fn test_select_where_not_in_list() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_where_not_in_list_body(&b);
}

crate::net_tests!(test_select_where_not_in_list);


fn test_select_order_by_asc_body(b: &crate::common::Backend) {
    setup_products(b);
    let result = exec(b, "SELECT name FROM products ORDER BY name ASC");
    let names: Vec<String> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("expected text, got {:?}", other),
    }).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "ORDER BY ASC should return names in ascending order");
}

#[test]
fn test_select_order_by_asc() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_order_by_asc_body(&b);
}

crate::net_tests!(test_select_order_by_asc);


fn test_select_order_by_desc_body(b: &crate::common::Backend) {
    setup_products(b);
    let result = exec(b, "SELECT name FROM products ORDER BY name DESC");
    let names: Vec<String> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("expected text, got {:?}", other),
    }).collect();
    let mut sorted = names.clone();
    sorted.sort();
    sorted.reverse();
    assert_eq!(names, sorted, "ORDER BY DESC should return names in descending order");
}

#[test]
fn test_select_order_by_desc() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_order_by_desc_body(&b);
}

crate::net_tests!(test_select_order_by_desc);


fn test_select_order_by_multi_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (last_name TEXT, first_name TEXT, score INT)");
    exec(b, "INSERT INTO t VALUES ('Smith', 'Alice', 90)");
    exec(b, "INSERT INTO t VALUES ('Smith', 'Bob', 80)");
    exec(b, "INSERT INTO t VALUES ('Jones', 'Carol', 95)");
    exec(b, "INSERT INTO t VALUES ('Jones', 'Dave', 70)");

    let result = exec(b, "SELECT last_name, first_name FROM t ORDER BY last_name ASC, first_name DESC");
    // Jones-Dave then Jones-Carol (DESC first_name), then Smith-Bob then Smith-Alice
    let pairs: Vec<(String, String)> = result.rows.iter().map(|r| {
        let ln = match r.get_by_idx(0) { Some(Value::Text(s)) => s.clone(), _ => panic!() };
        let fn_ = match r.get_by_idx(1) { Some(Value::Text(s)) => s.clone(), _ => panic!() };
        (ln, fn_)
    }).collect();
    assert_eq!(pairs[0], ("Jones".to_string(), "Dave".to_string()));
    assert_eq!(pairs[1], ("Jones".to_string(), "Carol".to_string()));
    assert_eq!(pairs[2], ("Smith".to_string(), "Bob".to_string()));
    assert_eq!(pairs[3], ("Smith".to_string(), "Alice".to_string()));
}

#[test]
fn test_select_order_by_multi() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_order_by_multi_body(&b);
}

crate::net_tests!(test_select_order_by_multi);


fn test_select_limit_body(b: &crate::common::Backend) {
    setup_products(b);
    let n = count_rows(b, "SELECT * FROM products LIMIT 3");
    assert_eq!(n, 3, "LIMIT 3 should return exactly 3 rows");
}

#[test]
fn test_select_limit() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_limit_body(&b);
}

crate::net_tests!(test_select_limit);


fn test_select_limit_offset_body(b: &crate::common::Backend) {
    setup_products(b);
    // 5 total rows, LIMIT 2 OFFSET 3 should return rows 4 and 5
    let n = count_rows(b, "SELECT * FROM products ORDER BY id LIMIT 2 OFFSET 3");
    assert_eq!(n, 2, "LIMIT 2 OFFSET 3 should return 2 rows");
}

#[test]
fn test_select_limit_offset() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_limit_offset_body(&b);
}

crate::net_tests!(test_select_limit_offset);


fn test_select_limit_beyond_table_body(b: &crate::common::Backend) {
    setup_products(b);
    // LIMIT 100 when only 5 rows: return all 5
    let n = count_rows(b, "SELECT * FROM products LIMIT 100");
    assert_eq!(n, 5, "LIMIT beyond table size should return all rows");
}

#[test]
fn test_select_limit_beyond_table() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_limit_beyond_table_body(&b);
}

crate::net_tests!(test_select_limit_beyond_table);


fn test_select_distinct_body(b: &crate::common::Backend) {
    setup_products(b);
    // 2 categories: fruit, veggie
    let n = count_rows(b, "SELECT DISTINCT category FROM products");
    assert_eq!(n, 2, "DISTINCT should return 2 categories");
}

#[test]
fn test_select_distinct() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_distinct_body(&b);
}

crate::net_tests!(test_select_distinct);


fn test_select_distinct_multi_column_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (a INT, b INT)");
    exec(b, "INSERT INTO t VALUES (1, 1)");
    exec(b, "INSERT INTO t VALUES (1, 2)");
    exec(b, "INSERT INTO t VALUES (1, 1)"); // duplicate of first
    exec(b, "INSERT INTO t VALUES (2, 1)");
    let n = count_rows(b, "SELECT DISTINCT a, b FROM t");
    assert_eq!(n, 3, "DISTINCT a,b should return 3 unique pairs");
}

#[test]
fn test_select_distinct_multi_column() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_distinct_multi_column_body(&b);
}

crate::net_tests!(test_select_distinct_multi_column);


fn test_select_case_expression_body(b: &crate::common::Backend) {
    setup_products(b);
    let result = exec(b, "SELECT name, CASE WHEN price > 5.0 THEN 'expensive' ELSE 'cheap' END AS tier \
         FROM products ORDER BY id");
    assert_eq!(result.rows.len(), 5);
    // Durian (price=15.0) should be 'expensive'
    let tier_durian = match result.rows[3].get_by_idx(1) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("expected Text, got {:?}", other),
    };
    assert_eq!(tier_durian, "expensive", "Durian (price=15.0) should be 'expensive'");
    // Apple (price=1.50) should be 'cheap'
    let tier_apple = match result.rows[0].get_by_idx(1) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("expected Text, got {:?}", other),
    };
    assert_eq!(tier_apple, "cheap");
}

#[test]
fn test_select_case_expression() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_case_expression_body(&b);
}

crate::net_tests!(test_select_case_expression);


fn test_select_coalesce_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, label TEXT)");
    exec(b, "INSERT INTO t VALUES (1, 'hello')");
    exec(b, "INSERT INTO t VALUES (2, NULL)");

    let result = exec(b, "SELECT id, COALESCE(label, 'default') FROM t ORDER BY id");
    // Row 2 has NULL label → should get 'default'
    match result.rows[1].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "default", "COALESCE should replace NULL with 'default'"),
        other => panic!("expected Text, got {:?}", other),
    }
    // Row 1 has 'hello' → stays 'hello'
    match result.rows[0].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("expected Text('hello'), got {:?}", other),
    }
}

#[test]
fn test_select_coalesce() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_coalesce_body(&b);
}

crate::net_tests!(test_select_coalesce);


fn test_select_nullif_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (0)");
    exec(b, "INSERT INTO t VALUES (5)");

    // NULLIF(val, 0): val=0 becomes NULL, val=5 stays 5
    let result = exec(b, "SELECT NULLIF(val, 0) FROM t ORDER BY val");
    assert_eq!(result.rows.len(), 2);
    // val=0 row
    match result.rows[0].get_by_idx(0) {
        Some(Value::Null) => {} // correct
        other => panic!("NULLIF(0, 0) should be NULL, got {:?}", other),
    }
    // val=5 row
    match result.rows[1].get_by_idx(0) {
        Some(Value::Int4(5)) => {} // correct
        other => panic!("NULLIF(5, 0) should be 5, got {:?}", other),
    }
}

#[test]
fn test_select_nullif() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_nullif_body(&b);
}

crate::net_tests!(test_select_nullif);


fn test_select_table_alias_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE products (id INT, name TEXT)");
    exec(b, "INSERT INTO products VALUES (1, 'Widget')");
    // Using table alias in SELECT
    let result = exec(b, "SELECT p.id, p.name FROM products p");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(1)) => {}
        other => panic!("expected Int4(1), got {:?}", other),
    }
}

#[test]
fn test_select_table_alias() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_table_alias_body(&b);
}

crate::net_tests!(test_select_table_alias);


fn test_select_empty_table_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT)");
    let n = count_rows(b, "SELECT * FROM t");
    assert_eq!(n, 0, "SELECT from empty table should return 0 rows");
}

#[test]
fn test_select_empty_table() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_empty_table_body(&b);
}

crate::net_tests!(test_select_empty_table);


fn test_select_string_concat_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (first TEXT, last TEXT)");
    exec(b, "INSERT INTO t VALUES ('John', 'Doe')");
    // String concatenation with ||
    let result = exec(b, "SELECT first || ' ' || last AS full_name FROM t");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "John Doe"),
        other => panic!("expected 'John Doe', got {:?}", other),
    }
}

#[test]
fn test_select_string_concat() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_string_concat_body(&b);
}

crate::net_tests!(test_select_string_concat);


fn test_select_window_function_rank_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (name TEXT, score INT)");
    exec(b, "INSERT INTO t VALUES ('Alice', 90)");
    exec(b, "INSERT INTO t VALUES ('Bob', 85)");
    exec(b, "INSERT INTO t VALUES ('Carol', 90)");
    // RANK() OVER (ORDER BY score DESC)
    let _result = exec(b, "SELECT name, RANK() OVER (ORDER BY score DESC) AS rnk FROM t");
}

#[test]
fn test_select_window_function_rank() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_window_function_rank_body(&b);
}

crate::net_tests!(test_select_window_function_rank);


fn test_select_generate_series_body(b: &crate::common::Backend) {
    let n = count_rows(b, "SELECT * FROM GENERATE_SERIES(1, 10)");
    assert_eq!(n, 10);
}

#[test]
fn test_select_generate_series() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_generate_series_body(&b);
}

crate::net_tests!(test_select_generate_series);


/// Verify that SELECT returns the correct number of total rows across multiple inserts.
fn test_select_row_count_consistency_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT)");
    for i in 1..=50 {
        exec(b, &format!("INSERT INTO t VALUES ({})", i));
    }
    let n = count_rows(b, "SELECT * FROM t");
    assert_eq!(n, 50, "Should see all 50 inserted rows");
}

#[test]
fn test_select_row_count_consistency() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_row_count_consistency_body(&b);
}

crate::net_tests!(test_select_row_count_consistency);


/// Verify ORDER BY + LIMIT + OFFSET together work correctly.
fn test_select_order_limit_offset_combined_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT)");
    for i in 1..=10 {
        exec(b, &format!("INSERT INTO t VALUES ({})", i));
    }
    // ORDER BY id ASC, LIMIT 3 OFFSET 5: should get ids 6,7,8
    let result = exec(b, "SELECT id FROM t ORDER BY id ASC LIMIT 3 OFFSET 5");
    assert_eq!(result.rows.len(), 3);
    let ids: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("expected Int4, got {:?}", other),
    }).collect();
    assert_eq!(ids, vec![6, 7, 8]);
}

#[test]
fn test_select_order_limit_offset_combined() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_order_limit_offset_combined_body(&b);
}

crate::net_tests!(test_select_order_limit_offset_combined);


/// SELECT without FROM (literal expression only).
fn test_select_no_from_body(b: &crate::common::Backend) {
    let val = query_int(b, "SELECT 2 + 3");
    assert_eq!(val, 5);
}

#[test]
fn test_select_no_from() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_select_no_from_body(&b);
}

crate::net_tests!(test_select_no_from);

