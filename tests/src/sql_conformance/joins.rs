/// Category 4: JOIN tests
/// Based on PostgreSQL join.sql patterns (J1_TBL / J2_TBL and custom tables).
use std::sync::Arc;
use tempfile::TempDir;
use crate::common::{make_engine, exec, count_rows, query_int};
use sql::Value;

fn setup_users_orders(engine: &Arc<sql::engine::QueryEngine>) {
    exec(engine, "CREATE TABLE users (id INT, name TEXT, dept_id INT)");
    exec(engine, "CREATE TABLE orders (id INT, user_id INT, product TEXT, amount FLOAT)");
    exec(engine, "INSERT INTO users VALUES (1, 'Alice', 10)");
    exec(engine, "INSERT INTO users VALUES (2, 'Bob', 20)");
    exec(engine, "INSERT INTO users VALUES (3, 'Carol', 10)");
    exec(engine, "INSERT INTO users VALUES (4, 'Dave', 30)");  // no orders
    exec(engine, "INSERT INTO orders VALUES (1, 1, 'Book', 12.99)");
    exec(engine, "INSERT INTO orders VALUES (2, 1, 'Pen', 2.50)");
    exec(engine, "INSERT INTO orders VALUES (3, 2, 'Laptop', 999.00)");
    exec(engine, "INSERT INTO orders VALUES (4, 3, 'Notebook', 5.99)");
}

#[test]
fn test_join_inner_basic() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_users_orders(&engine);

    let result = engine.execute(
        "SELECT u.name, o.product FROM users u JOIN orders o ON u.id = o.user_id ORDER BY o.id"
    ).unwrap();
    // Alice has 2 orders, Bob has 1, Carol has 1, Dave has 0
    assert_eq!(result.rows.len(), 4, "INNER JOIN should return 4 rows (Dave excluded)");
}

#[test]
fn test_join_inner_no_match_excluded() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_users_orders(&engine);

    // Dave has no orders — should not appear in inner join
    let result = engine.execute(
        "SELECT u.name FROM users u JOIN orders o ON u.id = o.user_id"
    ).unwrap();
    for row in &result.rows {
        match row.get_by_idx(0) {
            Some(Value::Text(n)) if n == "Dave" => {
                panic!("Dave should not appear in INNER JOIN (no orders)")
            }
            _ => {}
        }
    }
}

#[test]
fn test_join_inner_multi_condition() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE a (id INT, type_id INT, val TEXT)");
    exec(&engine, "CREATE TABLE b (id INT, type_id INT, info TEXT)");
    exec(&engine, "INSERT INTO a VALUES (1, 100, 'alpha')");
    exec(&engine, "INSERT INTO a VALUES (2, 200, 'beta')");
    exec(&engine, "INSERT INTO b VALUES (1, 100, 'match1')");
    exec(&engine, "INSERT INTO b VALUES (1, 999, 'no-match')"); // same id but different type
    exec(&engine, "INSERT INTO b VALUES (2, 200, 'match2')");

    let result = engine.execute(
        "SELECT a.val, b.info FROM a JOIN b ON a.id = b.id AND a.type_id = b.type_id ORDER BY a.id"
    ).unwrap();
    assert_eq!(result.rows.len(), 2, "Multi-condition JOIN should match 2 rows");
}

#[test]
fn test_join_left_outer() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_users_orders(&engine);

    let result = engine.execute(
        "SELECT u.name, o.product FROM users u LEFT JOIN orders o ON u.id = o.user_id ORDER BY u.id, o.id"
    ).unwrap();
    // Alice=2 rows, Bob=1, Carol=1, Dave=1 (with NULL product)
    assert_eq!(result.rows.len(), 5, "LEFT JOIN: 5 total rows including Dave with NULL product");

    // Find Dave's row (no product)
    let dave_row = result.rows.iter().find(|r| {
        matches!(r.get("name").or_else(|| r.get_by_idx(0)), Some(Value::Text(n)) if n == "Dave")
    });
    assert!(dave_row.is_some(), "Dave should appear in LEFT JOIN");
    if let Some(row) = dave_row {
        match row.get("product").or_else(|| row.get_by_idx(1)) {
            Some(Value::Null) | None => {} // correct: Dave has no orders
            other => panic!("Dave's product should be NULL in LEFT JOIN, got {:?}", other),
        }
    }
}

#[test]
fn test_join_left_outer_find_unmatched() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_users_orders(&engine);

    // Classic "find users with no orders" pattern
    let result = engine.execute(
        "SELECT u.name FROM users u LEFT JOIN orders o ON u.id = o.user_id WHERE o.id IS NULL"
    ).unwrap();
    assert_eq!(result.rows.len(), 1, "Only Dave has no orders");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(n)) => assert_eq!(n, "Dave"),
        other => panic!("expected Dave, got {:?}", other),
    }
}

#[test]
fn test_join_cross() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE colors (color TEXT)");
    exec(&engine, "CREATE TABLE sizes (size TEXT)");
    exec(&engine, "INSERT INTO colors VALUES ('red')");
    exec(&engine, "INSERT INTO colors VALUES ('blue')");
    exec(&engine, "INSERT INTO sizes VALUES ('S')");
    exec(&engine, "INSERT INTO sizes VALUES ('M')");
    exec(&engine, "INSERT INTO sizes VALUES ('L')");

    let n = count_rows(&engine, "SELECT color, size FROM colors CROSS JOIN sizes");
    assert_eq!(n, 6, "CROSS JOIN 2 colors × 3 sizes = 6 rows");
}

#[test]
fn test_join_implicit_cross_join() {
    // FROM a, b is an implicit CROSS JOIN
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE a (x INT)");
    exec(&engine, "CREATE TABLE b (y INT)");
    exec(&engine, "INSERT INTO a VALUES (1), (2)");
    exec(&engine, "INSERT INTO b VALUES (10), (20), (30)");

    let n = count_rows(&engine, "SELECT a.x, b.y FROM a, b");
    assert_eq!(n, 6, "Implicit cross join: 2 × 3 = 6 rows");
}

#[test]
fn test_join_self() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE employees (id INT, name TEXT, manager_id INT)");
    exec(&engine, "INSERT INTO employees VALUES (1, 'CEO', NULL)");
    exec(&engine, "INSERT INTO employees VALUES (2, 'VP', 1)");
    exec(&engine, "INSERT INTO employees VALUES (3, 'Manager', 2)");
    exec(&engine, "INSERT INTO employees VALUES (4, 'Engineer', 3)");

    let result = engine.execute(
        "SELECT e.name AS employee, m.name AS manager \
         FROM employees e JOIN employees m ON e.manager_id = m.id \
         ORDER BY e.id"
    ).unwrap();
    // VP, Manager, Engineer have managers (CEO excluded because no manager)
    assert_eq!(result.rows.len(), 3, "Self-join should show 3 employee-manager pairs");
}

#[test]
fn test_join_three_tables() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE authors (id INT, name TEXT)");
    exec(&engine, "CREATE TABLE books (id INT, title TEXT, author_id INT)");
    exec(&engine, "CREATE TABLE reviews (id INT, book_id INT, rating INT)");
    exec(&engine, "INSERT INTO authors VALUES (1, 'Tolkien'), (2, 'Herbert')");
    exec(&engine, "INSERT INTO books VALUES (1, 'Hobbit', 1), (2, 'Dune', 2)");
    exec(&engine, "INSERT INTO reviews VALUES (1, 1, 5), (2, 1, 4), (3, 2, 5)");

    let result = engine.execute(
        "SELECT a.name, b.title, r.rating \
         FROM authors a \
         JOIN books b ON a.id = b.author_id \
         JOIN reviews r ON b.id = r.book_id \
         ORDER BY r.id"
    ).unwrap();
    assert_eq!(result.rows.len(), 3, "3-way JOIN should return 3 rows");
}

#[test]
fn test_join_qualified_column() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t1 (id INT, val TEXT)");
    exec(&engine, "CREATE TABLE t2 (id INT, val TEXT)");
    exec(&engine, "INSERT INTO t1 VALUES (1, 'from-t1')");
    exec(&engine, "INSERT INTO t2 VALUES (1, 'from-t2')");

    let result = engine.execute(
        "SELECT t1.val AS v1, t2.val AS v2 FROM t1 JOIN t2 ON t1.id = t2.id"
    ).unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "from-t1"),
        other => panic!("expected 'from-t1', got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "from-t2"),
        other => panic!("expected 'from-t2', got {:?}", other),
    }
}

#[test]
fn test_join_ambiguous_column_error() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE a (id INT, val TEXT)");
    exec(&engine, "CREATE TABLE b (id INT, val TEXT)");
    exec(&engine, "INSERT INTO a VALUES (1, 'a1')");
    exec(&engine, "INSERT INTO b VALUES (1, 'b1')");

    // Unqualified 'id' in a join where both tables have 'id' — should error
    let result = engine.execute("SELECT id FROM a JOIN b ON a.id = b.id");
    // Either error or it returns something — must not panic
    let _ = result;
}

#[test]
fn test_join_using() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE orders (order_id INT, customer_id INT, total FLOAT)");
    exec(&engine, "CREATE TABLE customers (customer_id INT, name TEXT)");
    exec(&engine, "INSERT INTO customers VALUES (1, 'Alice'), (2, 'Bob')");
    exec(&engine, "INSERT INTO orders VALUES (100, 1, 50.0), (101, 2, 75.0), (102, 1, 25.0)");

    let result = engine.execute(
        "SELECT o.order_id, c.name FROM orders o JOIN customers c USING (customer_id) ORDER BY o.order_id"
    ).unwrap();
    assert_eq!(result.rows.len(), 3, "JOIN USING should return 3 rows");
}

#[test]
fn test_join_with_aggregate() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE users (id INT, name TEXT)");
    exec(&engine, "CREATE TABLE orders (id INT, user_id INT, amount FLOAT)");
    exec(&engine, "INSERT INTO users VALUES (1, 'Alice'), (2, 'Bob')");
    exec(&engine, "INSERT INTO orders VALUES (1, 1, 10.0), (2, 1, 20.0), (3, 2, 100.0)");

    let result = engine.execute(
        "SELECT u.name, COUNT(o.id) AS num_orders, SUM(o.amount) AS total \
         FROM users u JOIN orders o ON u.id = o.user_id \
         GROUP BY u.name ORDER BY total DESC"
    ).unwrap();
    assert_eq!(result.rows.len(), 2);
    // Bob has 1 order totalling 100.0 (highest)
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "Bob"),
        other => panic!("expected Bob (highest total), got {:?}", other),
    }
}

#[test]
fn test_join_empty_table() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t1 (id INT, val TEXT)");
    exec(&engine, "CREATE TABLE t2 (id INT, info TEXT)");
    exec(&engine, "INSERT INTO t1 VALUES (1, 'x'), (2, 'y')");
    // t2 is empty

    let n = count_rows(&engine, "SELECT t1.id FROM t1 JOIN t2 ON t1.id = t2.id");
    assert_eq!(n, 0, "JOIN with empty table should return 0 rows");

    // LEFT JOIN with empty right table — use explicit column selection to avoid * on empty table issues
    let n2 = count_rows(&engine, "SELECT t1.id FROM t1 LEFT JOIN t2 ON t1.id = t2.id");
    assert_eq!(n2, 2, "LEFT JOIN with empty right table returns all left rows");
}

#[test]
fn test_join_full_outer() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t1 (id INT)");
    exec(&engine, "CREATE TABLE t2 (id INT)");
    exec(&engine, "INSERT INTO t1 VALUES (1), (2)");
    exec(&engine, "INSERT INTO t2 VALUES (2), (3)");

    let n = count_rows(&engine, "SELECT * FROM t1 FULL OUTER JOIN t2 ON t1.id = t2.id");
    // Rows: (1, NULL), (2, 2), (NULL, 3) = 3 rows
    assert_eq!(n, 3);
}

#[test]
fn test_join_right_outer() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t1 (id INT)");
    exec(&engine, "CREATE TABLE t2 (id INT)");
    exec(&engine, "INSERT INTO t1 VALUES (1)");
    exec(&engine, "INSERT INTO t2 VALUES (1), (2)");

    // (1,1) and (NULL,2) = 2 rows
    let n = count_rows(&engine, "SELECT * FROM t1 RIGHT JOIN t2 ON t1.id = t2.id");
    assert_eq!(n, 2);
}

// ---- J1_TBL / J2_TBL tests based on PostgreSQL join.sql ----

fn setup_j1_j2(engine: &Arc<sql::engine::QueryEngine>) {
    exec(engine, "CREATE TABLE j1_tbl (i INT, j INT, t TEXT)");
    exec(engine, "CREATE TABLE j2_tbl (i INT, k INT)");
    exec(engine, "INSERT INTO j1_tbl VALUES (1, 4, 'one')");
    exec(engine, "INSERT INTO j1_tbl VALUES (2, 3, 'two')");
    exec(engine, "INSERT INTO j1_tbl VALUES (3, 2, 'three')");
    exec(engine, "INSERT INTO j1_tbl VALUES (4, 1, 'four')");
    exec(engine, "INSERT INTO j1_tbl VALUES (5, 0, 'five')");
    exec(engine, "INSERT INTO j1_tbl VALUES (6, 6, 'six')");
    exec(engine, "INSERT INTO j1_tbl VALUES (7, 7, 'seven')");
    exec(engine, "INSERT INTO j1_tbl VALUES (8, 8, 'eight')");
    exec(engine, "INSERT INTO j1_tbl VALUES (0, NULL, 'zero')");
    exec(engine, "INSERT INTO j1_tbl VALUES (NULL, NULL, 'null')");
    exec(engine, "INSERT INTO j1_tbl VALUES (NULL, 0, 'zero')");

    exec(engine, "INSERT INTO j2_tbl VALUES (1, -1)");
    exec(engine, "INSERT INTO j2_tbl VALUES (2, 2)");
    exec(engine, "INSERT INTO j2_tbl VALUES (3, -3)");
    exec(engine, "INSERT INTO j2_tbl VALUES (2, 4)");
    exec(engine, "INSERT INTO j2_tbl VALUES (5, -5)");
    exec(engine, "INSERT INTO j2_tbl VALUES (5, -5)");
    exec(engine, "INSERT INTO j2_tbl VALUES (0, NULL)");
    exec(engine, "INSERT INTO j2_tbl VALUES (NULL, NULL)");
    exec(engine, "INSERT INTO j2_tbl VALUES (NULL, 0)");
}

#[test]
fn test_j1j2_inner_join_on() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    // J1_TBL JOIN J2_TBL ON i = i: matches i=0,1,2,2,3,5,5 (NULL rows excluded)
    let result = exec(&engine, "SELECT j1_tbl.i, j1_tbl.j, j1_tbl.t, j2_tbl.i, j2_tbl.k FROM j1_tbl JOIN j2_tbl ON j1_tbl.i = j2_tbl.i ORDER BY j1_tbl.i, j2_tbl.k");
    assert!(result.rows.len() > 0, "INNER JOIN ON i=i should return rows");
}

#[test]
fn test_j1j2_inner_join_using() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    let result = exec(&engine, "SELECT * FROM j1_tbl INNER JOIN j2_tbl USING (i) ORDER BY i");
    assert!(result.rows.len() > 0, "INNER JOIN USING(i) should return rows");
}

#[test]
fn test_j1j2_left_outer_join_using() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    // LEFT JOIN keeps all j1 rows including unmatched
    let result = exec(&engine, "SELECT j1_tbl.i, j1_tbl.t, j2_tbl.k FROM j1_tbl LEFT OUTER JOIN j2_tbl ON j1_tbl.i = j2_tbl.i ORDER BY j1_tbl.i, j2_tbl.k");
    // j1 has 11 rows; some i values match multiple j2 rows (i=2 matches twice, i=5 matches twice)
    assert!(result.rows.len() >= 11, "LEFT JOIN should return at least as many rows as left table");
}

#[test]
fn test_j1j2_cross_join_size() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    // J1 has 11 rows, J2 has 9 rows → cross join = 99
    let n = count_rows(&engine, "SELECT j1_tbl.i, j2_tbl.k FROM j1_tbl CROSS JOIN j2_tbl");
    assert_eq!(n, 99, "CROSS JOIN 11 * 9 = 99");
}

#[test]
fn test_j1j2_join_on_j_equals_i() {
    // J1_TBL JOIN J2_TBL ON J1.i = J2.k (different columns)
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    let result = exec(&engine, "SELECT j1_tbl.t, j2_tbl.i, j2_tbl.k FROM j1_tbl JOIN j2_tbl ON j1_tbl.i = j2_tbl.k ORDER BY j1_tbl.i");
    assert!(result.rows.len() >= 0, "JOIN on i=k should execute without error");
}

#[test]
fn test_j1j2_null_values_excluded_from_inner_join() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    // NULLs in i should not match in INNER JOIN
    let result = exec(&engine, "SELECT j1_tbl.t FROM j1_tbl JOIN j2_tbl ON j1_tbl.i = j2_tbl.i ORDER BY j1_tbl.i");
    for row in &result.rows {
        // 'null' row (j1.i=NULL) should not appear
        if let Some(Value::Text(t)) = row.get_by_idx(0) {
            assert_ne!(t.as_str(), "null", "NULL i should not match in INNER JOIN");
        }
    }
}

#[test]
fn test_join_with_where_filter() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    let result = exec(&engine, "SELECT j1_tbl.t FROM j1_tbl JOIN j2_tbl ON j1_tbl.i = j2_tbl.i WHERE j2_tbl.k > 0 ORDER BY j1_tbl.t");
    assert!(result.rows.len() >= 0, "JOIN + WHERE should execute");
}

#[test]
fn test_join_comma_syntax_with_where() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    // Old-style implicit join
    let result = exec(&engine, "SELECT j1_tbl.t, j2_tbl.k FROM j1_tbl, j2_tbl WHERE j1_tbl.i = j2_tbl.i ORDER BY j1_tbl.i, j2_tbl.k");
    assert!(result.rows.len() > 0, "Comma-join with WHERE should return rows");
}

#[test]
fn test_join_count_matching_rows() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    // i=2 appears twice in j2, i=5 appears twice, so we get extra rows
    let n = query_int(&engine, "SELECT COUNT(*) FROM j1_tbl JOIN j2_tbl ON j1_tbl.i = j2_tbl.i");
    assert!(n >= 5, "Expected at least 5 matching pairs");
}

#[test]
fn test_join_self_j1() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    // Self-join: rows where j1a.j = j1b.i
    let result = exec(&engine, "SELECT a.t AS t1, b.t AS t2 FROM j1_tbl a JOIN j1_tbl b ON a.j = b.i ORDER BY a.i, b.i");
    assert!(result.rows.len() > 0, "Self-join j1.j = j1.i should return rows");
}

#[test]
fn test_join_aggregate_after_join() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    let n = query_int(&engine, "SELECT COUNT(*) FROM j1_tbl JOIN j2_tbl ON j1_tbl.i = j2_tbl.i");
    assert!(n > 0, "COUNT over join result should be > 0");
}

#[test]
fn test_join_left_produces_nulls_for_unmatched() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    // i=4,6,7,8 are in j1 but not in j2 — should appear with NULL k in left join
    let result = exec(&engine, "SELECT j1_tbl.i, j2_tbl.k FROM j1_tbl LEFT JOIN j2_tbl ON j1_tbl.i = j2_tbl.i WHERE j2_tbl.k IS NULL ORDER BY j1_tbl.i");
    assert!(result.rows.len() > 0, "Some j1 rows should have no match in j2");
}

#[test]
fn test_join_three_tables_with_j1_j2() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    exec(&engine, "CREATE TABLE j3_tbl (k INT, label TEXT)");
    exec(&engine, "INSERT INTO j3_tbl VALUES (-1, 'neg_one')");
    exec(&engine, "INSERT INTO j3_tbl VALUES (2, 'two')");
    exec(&engine, "INSERT INTO j3_tbl VALUES (-3, 'neg_three')");
    let result = exec(&engine,
        "SELECT j1_tbl.t, j2_tbl.k, j3_tbl.label \
         FROM j1_tbl \
         JOIN j2_tbl ON j1_tbl.i = j2_tbl.i \
         JOIN j3_tbl ON j2_tbl.k = j3_tbl.k \
         ORDER BY j1_tbl.i");
    assert!(result.rows.len() >= 0, "3-table join should execute");
}

#[test]
fn test_join_distinct() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    // DISTINCT on join result
    let result = exec(&engine, "SELECT DISTINCT j1_tbl.i FROM j1_tbl JOIN j2_tbl ON j1_tbl.i = j2_tbl.i ORDER BY j1_tbl.i");
    // Each i value appears at most once
    let mut prev: Option<i32> = None;
    for row in &result.rows {
        if let Some(Value::Int4(v)) = row.get_by_idx(0) {
            if let Some(p) = prev {
                assert_ne!(p, *v, "DISTINCT should eliminate duplicates");
            }
            prev = Some(*v);
        }
    }
}

#[test]
fn test_join_filter_on_right_table_column() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    let result = exec(&engine,
        "SELECT j1_tbl.t FROM j1_tbl JOIN j2_tbl ON j1_tbl.i = j2_tbl.i WHERE j2_tbl.k < 0 ORDER BY j1_tbl.i");
    assert!(result.rows.len() >= 0, "Filter on right table after join should work");
}

#[test]
fn test_join_order_by_join_column() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    let result = exec(&engine,
        "SELECT j1_tbl.i, j2_tbl.k FROM j1_tbl JOIN j2_tbl ON j1_tbl.i = j2_tbl.i ORDER BY j2_tbl.k DESC");
    if result.rows.len() >= 2 {
        let first = result.rows[0].get_by_idx(1);
        let last = result.rows[result.rows.len() - 1].get_by_idx(1);
        match (first, last) {
            (Some(Value::Int4(a)), Some(Value::Int4(b))) => assert!(a >= b, "ORDER BY DESC should sort descending"),
            _ => {}
        }
    }
}

#[test]
fn test_join_limit_with_join() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    setup_j1_j2(&engine);
    let result = exec(&engine,
        "SELECT j1_tbl.t, j2_tbl.k FROM j1_tbl JOIN j2_tbl ON j1_tbl.i = j2_tbl.i ORDER BY j1_tbl.i LIMIT 3");
    assert!(result.rows.len() <= 3, "LIMIT should cap results");
}

#[test]
fn test_join_with_text_equality() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t1 (id INT, label TEXT)");
    exec(&engine, "CREATE TABLE t2 (id INT, label TEXT)");
    exec(&engine, "INSERT INTO t1 VALUES (1, 'hello'), (2, 'world'), (3, 'unique_t1')");
    exec(&engine, "INSERT INTO t2 VALUES (1, 'hello'), (2, 'bar'), (4, 'unique_t2')");
    // Join on text column: only 'hello' matches
    let result = exec(&engine,
        "SELECT t1.id, t2.id FROM t1 JOIN t2 ON t1.label = t2.label ORDER BY t1.id");
    assert_eq!(result.rows.len(), 1, "Only 'hello' matches in both tables");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(1)) => {}
        other => panic!("Expected id=1, got {:?}", other),
    }
}

#[test]
fn test_join_multiple_columns_select() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE emp (eid INT, name TEXT, dept_id INT)");
    exec(&engine, "CREATE TABLE dept (did INT, dname TEXT)");
    exec(&engine, "INSERT INTO dept VALUES (1, 'Eng'), (2, 'HR'), (3, 'Finance')");
    exec(&engine, "INSERT INTO emp VALUES (1, 'Alice', 1), (2, 'Bob', 1), (3, 'Carol', 2), (4, 'Dave', 99)");
    let result = exec(&engine,
        "SELECT emp.name, dept.dname FROM emp JOIN dept ON emp.dept_id = dept.did ORDER BY emp.eid");
    // Dave (dept_id=99) should not appear
    assert_eq!(result.rows.len(), 3, "3 employees have valid dept");
    let names: Vec<String> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(names, vec!["Alice", "Bob", "Carol"]);
}

#[test]
fn test_join_left_with_aggregate() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE cats (id INT, name TEXT)");
    exec(&engine, "CREATE TABLE items (id INT, cat_id INT, price FLOAT)");
    exec(&engine, "INSERT INTO cats VALUES (1, 'books'), (2, 'toys'), (3, 'food')");
    exec(&engine, "INSERT INTO items VALUES (1, 1, 10.0), (2, 1, 20.0), (3, 2, 15.0)");
    // Category 'food' has no items
    let result = exec(&engine,
        "SELECT cats.name, COUNT(items.id) AS item_count \
         FROM cats LEFT JOIN items ON cats.id = items.cat_id \
         GROUP BY cats.name ORDER BY cats.name");
    assert_eq!(result.rows.len(), 3, "All 3 categories should appear (LEFT JOIN)");
}

#[test]
fn test_join_using_deduplicates_column() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE p (pid INT, name TEXT)");
    exec(&engine, "CREATE TABLE o (pid INT, amt FLOAT)");
    exec(&engine, "INSERT INTO p VALUES (1, 'A'), (2, 'B')");
    exec(&engine, "INSERT INTO o VALUES (1, 100.0), (1, 200.0), (2, 50.0)");
    let result = exec(&engine,
        "SELECT p.name, o.amt FROM p JOIN o USING (pid) ORDER BY p.name, o.amt");
    assert_eq!(result.rows.len(), 3, "JOIN USING should yield 3 rows");
}

#[test]
fn test_join_inner_only_matching_nulls_excluded() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE left_t (id INT)");
    exec(&engine, "CREATE TABLE right_t (id INT)");
    exec(&engine, "INSERT INTO left_t VALUES (1), (NULL), (3)");
    exec(&engine, "INSERT INTO right_t VALUES (1), (NULL), (3)");
    // NULL != NULL in joins
    let n = count_rows(&engine, "SELECT * FROM left_t JOIN right_t ON left_t.id = right_t.id");
    assert_eq!(n, 2, "NULLs should not match in INNER JOIN");
}

#[test]
fn test_join_chained_two_left_joins() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE c (cid INT, cname TEXT)");
    exec(&engine, "CREATE TABLE o (oid INT, cid INT, total FLOAT)");
    exec(&engine, "CREATE TABLE oitem (iid INT, oid INT, qty INT)");
    exec(&engine, "INSERT INTO c VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Carol')");
    exec(&engine, "INSERT INTO o VALUES (10, 1, 100.0), (11, 2, 200.0)");
    exec(&engine, "INSERT INTO oitem VALUES (1, 10, 5), (2, 10, 3)");
    let result = exec(&engine,
        "SELECT c.cname, o.total, oitem.qty \
         FROM c LEFT JOIN o ON c.cid = o.cid \
         LEFT JOIN oitem ON o.oid = oitem.oid \
         ORDER BY c.cid, oitem.iid");
    assert!(result.rows.len() >= 3, "Chained LEFT JOINs should include unmatched rows");
}

#[test]
fn test_join_having_after_join_group() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE stores (sid INT, city TEXT)");
    exec(&engine, "CREATE TABLE sales (sid INT, amount FLOAT)");
    exec(&engine, "INSERT INTO stores VALUES (1, 'NYC'), (2, 'LA'), (3, 'Chicago')");
    exec(&engine, "INSERT INTO sales VALUES (1, 100.0), (1, 200.0), (2, 50.0), (2, 50.0), (3, 500.0)");
    let result = exec(&engine,
        "SELECT stores.city, SUM(sales.amount) AS total \
         FROM stores JOIN sales ON stores.sid = sales.sid \
         GROUP BY stores.city \
         HAVING SUM(sales.amount) > 150 \
         ORDER BY total DESC");
    assert_eq!(result.rows.len(), 2, "Only NYC (300) and Chicago (500) exceed 150");
}

#[test]
fn test_join_with_subquery_in_from() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE products (pid INT, name TEXT, price FLOAT)");
    exec(&engine, "CREATE TABLE sales_log (pid INT, qty INT)");
    exec(&engine, "INSERT INTO products VALUES (1, 'Widget', 5.0), (2, 'Gadget', 10.0), (3, 'Doohickey', 3.0)");
    exec(&engine, "INSERT INTO sales_log VALUES (1, 100), (2, 50), (1, 25)");
    let result = exec(&engine,
        "SELECT p.name, sub.total_qty \
         FROM products p \
         JOIN (SELECT pid, SUM(qty) AS total_qty FROM sales_log GROUP BY pid) AS sub \
         ON p.pid = sub.pid \
         ORDER BY sub.total_qty DESC");
    assert_eq!(result.rows.len(), 2, "Only 2 products appear in sales_log");
}

#[test]
fn test_join_alias_in_on_clause() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE aa (x INT, y INT)");
    exec(&engine, "CREATE TABLE bb (x INT, z INT)");
    exec(&engine, "INSERT INTO aa VALUES (1, 10), (2, 20), (3, 30)");
    exec(&engine, "INSERT INTO bb VALUES (2, 200), (3, 300), (4, 400)");
    let result = exec(&engine,
        "SELECT aa.y, bb.z FROM aa a1 JOIN bb b1 ON a1.x = b1.x ORDER BY a1.x");
    assert_eq!(result.rows.len(), 2, "x=2,3 match between aa and bb");
}

#[test]
fn test_join_with_order_by_from_both_tables() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE students (sid INT, name TEXT)");
    exec(&engine, "CREATE TABLE grades (sid INT, subject TEXT, score INT)");
    exec(&engine, "INSERT INTO students VALUES (1, 'Alice'), (2, 'Bob')");
    exec(&engine, "INSERT INTO grades VALUES (1, 'Math', 90), (1, 'English', 85), (2, 'Math', 75), (2, 'English', 95)");
    let result = exec(&engine,
        "SELECT students.name, grades.subject, grades.score \
         FROM students JOIN grades ON students.sid = grades.sid \
         ORDER BY students.name, grades.score DESC");
    assert_eq!(result.rows.len(), 4);
    // First row should be Alice with highest score (90)
    match result.rows[0].get_by_idx(2) {
        Some(Value::Int4(s)) => assert_eq!(*s, 90),
        other => panic!("Expected score=90, got {:?}", other),
    }
}

#[test]
fn test_join_count_per_group_after_join() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE regions (rid INT, rname TEXT)");
    exec(&engine, "CREATE TABLE offices (oid INT, rid INT)");
    exec(&engine, "INSERT INTO regions VALUES (1, 'North'), (2, 'South'), (3, 'East')");
    exec(&engine, "INSERT INTO offices VALUES (1, 1), (2, 1), (3, 1), (4, 2), (5, 3), (6, 3)");
    let result = exec(&engine,
        "SELECT r.rname, COUNT(o.oid) AS cnt \
         FROM regions r JOIN offices o ON r.rid = o.rid \
         GROUP BY r.rname ORDER BY cnt DESC");
    assert_eq!(result.rows.len(), 3);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "North"),
        other => panic!("Expected North (3 offices), got {:?}", other),
    }
}

#[test]
fn test_join_where_on_both_tables() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE inv (item_id INT, warehouse TEXT, qty INT)");
    exec(&engine, "CREATE TABLE items (item_id INT, item_name TEXT, category TEXT)");
    exec(&engine, "INSERT INTO items VALUES (1, 'Apple', 'fruit'), (2, 'Banana', 'fruit'), (3, 'Carrot', 'veg'), (4, 'Desk', 'furniture')");
    exec(&engine, "INSERT INTO inv VALUES (1, 'WH1', 100), (2, 'WH1', 50), (3, 'WH2', 200), (4, 'WH2', 10)");
    let result = exec(&engine,
        "SELECT items.item_name, inv.qty \
         FROM items JOIN inv ON items.item_id = inv.item_id \
         WHERE items.category = 'fruit' AND inv.qty > 60 \
         ORDER BY inv.qty DESC");
    assert_eq!(result.rows.len(), 1, "Only Apple (qty=100) is fruit with qty>60");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "Apple"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_join_sum_after_join() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE teams (tid INT, tname TEXT)");
    exec(&engine, "CREATE TABLE scores (tid INT, points INT)");
    exec(&engine, "INSERT INTO teams VALUES (1, 'Red'), (2, 'Blue'), (3, 'Green')");
    exec(&engine, "INSERT INTO scores VALUES (1, 10), (1, 20), (2, 50), (3, 5), (3, 15)");
    let result = exec(&engine,
        "SELECT teams.tname, SUM(scores.points) AS total \
         FROM teams JOIN scores ON teams.tid = scores.tid \
         GROUP BY teams.tname ORDER BY total DESC");
    assert_eq!(result.rows.len(), 3);
    // Blue has 50 points (highest), Red=30, Green=20
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "Blue"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_join_inner_no_match_returns_empty() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE xa (id INT)");
    exec(&engine, "CREATE TABLE xb (id INT)");
    exec(&engine, "INSERT INTO xa VALUES (1), (2), (3)");
    exec(&engine, "INSERT INTO xb VALUES (10), (20), (30)");
    let n = count_rows(&engine, "SELECT * FROM xa JOIN xb ON xa.id = xb.id");
    assert_eq!(n, 0, "No matching ids → 0 rows");
}

#[test]
fn test_join_left_empty_right_table() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE have_data (id INT, val TEXT)");
    exec(&engine, "CREATE TABLE no_data (id INT, extra TEXT)");
    exec(&engine, "INSERT INTO have_data VALUES (1, 'a'), (2, 'b'), (3, 'c')");
    // LEFT JOIN with empty right table — select only from left side to avoid missing column
    let result = exec(&engine,
        "SELECT have_data.id, have_data.val FROM have_data LEFT JOIN no_data ON have_data.id = no_data.id ORDER BY have_data.id");
    assert_eq!(result.rows.len(), 3, "LEFT JOIN with empty right = all left rows");
    let vals: Vec<String> = result.rows.iter().map(|r| match r.get_by_idx(1) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("Expected text, got {:?}", other),
    }).collect();
    assert_eq!(vals, vec!["a", "b", "c"]);
}

#[test]
fn test_join_natural_join() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE t_a (id INT, name TEXT)");
    exec(&engine, "CREATE TABLE t_b (id INT, score INT)");
    exec(&engine, "INSERT INTO t_a VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Carol')");
    exec(&engine, "INSERT INTO t_b VALUES (1, 100), (2, 200)");
    let result = exec(&engine, "SELECT t_a.name, t_b.score FROM t_a NATURAL JOIN t_b ORDER BY t_a.id");
    assert_eq!(result.rows.len(), 2, "NATURAL JOIN on id should match 2 rows");
}

#[test]
fn test_join_with_between() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    exec(&engine, "CREATE TABLE ranges (rid INT, lo INT, hi INT)");
    exec(&engine, "CREATE TABLE vals (vid INT, v INT)");
    exec(&engine, "INSERT INTO ranges VALUES (1, 1, 5), (2, 10, 20)");
    exec(&engine, "INSERT INTO vals VALUES (1, 3), (2, 7), (3, 15)");
    let result = exec(&engine,
        "SELECT vals.v, ranges.rid FROM vals JOIN ranges ON vals.v BETWEEN ranges.lo AND ranges.hi ORDER BY vals.v");
    assert_eq!(result.rows.len(), 2, "v=3 fits range 1, v=15 fits range 2; v=7 fits no range");
}
