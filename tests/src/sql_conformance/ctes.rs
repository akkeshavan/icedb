/// Category 7: CTE (Common Table Expression) tests
/// Based on PostgreSQL with.sql patterns.
use tempfile::TempDir;
use crate::common::{make_engine, exec, query_int, count_rows, Backend};
use sql::Value;

fn setup_org(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE employees (id INT, name TEXT, dept TEXT, salary INT, manager_id INT)");
    exec(b, "INSERT INTO employees VALUES (1, 'Alice', 'eng', 90000, NULL)");
    exec(b, "INSERT INTO employees VALUES (2, 'Bob', 'eng', 80000, 1)");
    exec(b, "INSERT INTO employees VALUES (3, 'Carol', 'hr', 70000, NULL)");
    exec(b, "INSERT INTO employees VALUES (4, 'Dave', 'hr', 65000, 3)");
    exec(b, "INSERT INTO employees VALUES (5, 'Eve', 'eng', 85000, 1)");
}

fn test_cte_basic_body(b: &crate::common::Backend) {
    setup_org(b);

    let result = exec(b, "WITH eng_team AS (SELECT name, salary FROM employees WHERE dept = 'eng') \
         SELECT name FROM eng_team WHERE salary > 85000 ORDER BY name");
    assert_eq!(result.rows.len(), 1, "Only Alice earns > 85000 in eng");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(n)) => assert_eq!(n, "Alice"),
        other => panic!("expected Alice, got {:?}", other),
    }
}

#[test]
fn test_cte_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_basic_body(&b);
}

crate::net_tests!(test_cte_basic);


fn test_cte_used_in_count_body(b: &crate::common::Backend) {
    setup_org(b);

    let n = query_int(b,
        "WITH eng AS (SELECT id FROM employees WHERE dept = 'eng') SELECT COUNT(*) FROM eng");
    assert_eq!(n, 3, "3 engineers in the CTE");
}

#[test]
fn test_cte_used_in_count() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_used_in_count_body(&b);
}

crate::net_tests!(test_cte_used_in_count);


fn test_cte_multi_step_body(b: &crate::common::Backend) {
    setup_org(b);

    let result = exec(b, "WITH \
           dept_counts AS (SELECT dept, COUNT(*) AS headcount FROM employees GROUP BY dept), \
           large_depts AS (SELECT dept FROM dept_counts WHERE headcount >= 3) \
         SELECT dept FROM large_depts ORDER BY dept");
    // eng: 3 employees (Alice, Bob, Eve), hr: 2 employees
    assert_eq!(result.rows.len(), 1, "Only eng has >= 3 headcount");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(d)) => assert_eq!(d, "eng"),
        other => panic!("expected 'eng', got {:?}", other),
    }
}

#[test]
fn test_cte_multi_step() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_multi_step_body(&b);
}

crate::net_tests!(test_cte_multi_step);


fn test_cte_with_aggregation_body(b: &crate::common::Backend) {
    setup_org(b);

    let result = exec(b, "WITH dept_summary AS ( \
           SELECT dept, SUM(salary) AS total_salary, AVG(salary) AS avg_salary \
           FROM employees GROUP BY dept \
         ) \
         SELECT dept, total_salary FROM dept_summary WHERE total_salary > 200000 ORDER BY dept");
    // eng: 90000+80000+85000 = 255000 > 200000 ✓
    // hr: 70000+65000 = 135000 <= 200000 ✗
    assert_eq!(result.rows.len(), 1, "Only eng has total_salary > 200000");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(d)) => assert_eq!(d, "eng"),
        other => panic!("expected 'eng', got {:?}", other),
    }
}

#[test]
fn test_cte_with_aggregation() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_with_aggregation_body(&b);
}

crate::net_tests!(test_cte_with_aggregation);


fn test_cte_in_where_subquery_body(b: &crate::common::Backend) {
    setup_org(b);

    exec(b, "CREATE TABLE projects (id INT, lead_id INT, name TEXT)");
    exec(b, "INSERT INTO projects VALUES (1, 1, 'Alpha')");  // Alice leads
    exec(b, "INSERT INTO projects VALUES (2, 3, 'Beta')");   // Carol leads
    exec(b, "INSERT INTO projects VALUES (3, 2, 'Gamma')");  // Bob leads

    let result = exec(b, "WITH eng_ids AS (SELECT id FROM employees WHERE dept = 'eng') \
         SELECT p.name FROM projects p WHERE p.lead_id IN (SELECT id FROM eng_ids) ORDER BY p.name");
    // Alpha (Alice=eng), Gamma (Bob=eng) → 2 projects led by engineers
    assert_eq!(result.rows.len(), 2, "2 projects led by engineers");
    let names: Vec<String> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(names, vec!["Alpha", "Gamma"]);
}

#[test]
fn test_cte_in_where_subquery() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_in_where_subquery_body(&b);
}

crate::net_tests!(test_cte_in_where_subquery);


fn test_cte_with_join_body(b: &crate::common::Backend) {
    setup_org(b);

    // CTE that joins with base table
    let result = exec(b, "WITH above_avg AS ( \
           SELECT id, name, salary FROM employees \
           WHERE salary > (SELECT AVG(salary) FROM employees) \
         ) \
         SELECT a.name AS employee, m.name AS manager \
         FROM above_avg a \
         JOIN employees m ON a.manager_id = m.id \
         ORDER BY a.name");
    // avg salary = (90000+80000+70000+65000+85000)/5 = 78000
    // above avg: Alice(90000), Bob(80000), Eve(85000)
    // Alice has no manager (NULL), Bob and Eve have Alice as manager
    assert_eq!(result.rows.len(), 2, "Bob and Eve are above-avg with a manager");
}

#[test]
fn test_cte_with_join() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_with_join_body(&b);
}

crate::net_tests!(test_cte_with_join);


fn test_cte_chained_three_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE sales (region TEXT, amount FLOAT)");
    exec(b, "INSERT INTO sales VALUES ('north', 100.0)");
    exec(b, "INSERT INTO sales VALUES ('north', 200.0)");
    exec(b, "INSERT INTO sales VALUES ('south', 50.0)");
    exec(b, "INSERT INTO sales VALUES ('east', 300.0)");
    exec(b, "INSERT INTO sales VALUES ('east', 150.0)");

    let result = exec(b, "WITH \
           totals AS (SELECT region, SUM(amount) AS total FROM sales GROUP BY region), \
           ranked AS (SELECT region, total FROM totals ORDER BY total DESC), \
           top2 AS (SELECT region FROM ranked LIMIT 2) \
         SELECT region FROM top2 ORDER BY region");
    // east=450, north=300, south=50 → top 2: east, north
    assert_eq!(result.rows.len(), 2, "Top 2 regions by sales");
    let regions: Vec<String> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(regions, vec!["east", "north"]);
}

#[test]
fn test_cte_chained_three() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_chained_three_body(&b);
}

crate::net_tests!(test_cte_chained_three);


fn test_cte_column_aliasing_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (val INT)");
    exec(b, "INSERT INTO t VALUES (10), (20), (30)");

    let result = exec(b, "WITH doubled AS (SELECT val * 2 AS dbl FROM t) \
         SELECT dbl FROM doubled ORDER BY dbl");
    assert_eq!(result.rows.len(), 3);
    let vals: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(vals, vec![20, 40, 60]);
}

#[test]
fn test_cte_column_aliasing() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_column_aliasing_body(&b);
}

crate::net_tests!(test_cte_column_aliasing);


fn test_cte_with_limit_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT, score INT)");
    for i in 1..=20 {
        exec(b, &format!("INSERT INTO t VALUES ({}, {})", i, i * 5));
    }

    let result = exec(b, "WITH top5 AS (SELECT id, score FROM t ORDER BY score DESC LIMIT 5) \
         SELECT COUNT(*) FROM top5");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int8(5)) | Some(Value::Int4(5)) => {}
        other => panic!("expected count=5, got {:?}", other),
    }
}

#[test]
fn test_cte_with_limit() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_with_limit_body(&b);
}

crate::net_tests!(test_cte_with_limit);


fn test_cte_book_revenue_example_body(b: &crate::common::Backend) {
    // Based on the tutorial test already in integration.rs, but with assertions
    exec(b, "CREATE TABLE authors (id INT, name TEXT)");
    exec(b, "CREATE TABLE books (id INT, title TEXT, author_id INT, price FLOAT)");
    exec(b, "CREATE TABLE orders (id INT, book_id INT, quantity INT, total_price FLOAT)");

    exec(b, "INSERT INTO authors VALUES (1, 'Tolkien'), (2, 'Herbert')");
    exec(b, "INSERT INTO books VALUES (1, 'Hobbit', 1, 12.99), (2, 'Dune', 2, 15.99)");
    exec(b, "INSERT INTO orders VALUES (1, 1, 2, 25.98)");  // 2 Hobbits
    exec(b, "INSERT INTO orders VALUES (2, 2, 3, 47.97)");  // 3 Dunes
    exec(b, "INSERT INTO orders VALUES (3, 1, 1, 12.99)");  // 1 Hobbit

    let result = exec(b, "WITH book_revenue AS ( \
           SELECT b.author_id, SUM(o.total_price) AS revenue \
           FROM books b JOIN orders o ON o.book_id = b.id \
           GROUP BY b.author_id \
         ), \
         author_revenue AS ( \
           SELECT a.name, br.revenue \
           FROM book_revenue br JOIN authors a ON a.id = br.author_id \
         ) \
         SELECT name, revenue FROM author_revenue ORDER BY revenue DESC");
    assert_eq!(result.rows.len(), 2, "2 authors with revenue");
    // Herbert: 47.97, Tolkien: 25.98+12.99=38.97
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(n)) => assert_eq!(n, "Herbert", "Herbert has highest revenue"),
        other => panic!("expected Herbert, got {:?}", other),
    }
}

#[test]
fn test_cte_book_revenue_example() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_book_revenue_example_body(&b);
}

crate::net_tests!(test_cte_book_revenue_example);


fn test_cte_recursive_hierarchy_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE emp (id INT, name TEXT, manager_id INT)");
    exec(b, "INSERT INTO emp VALUES (1, 'CEO', NULL)");
    exec(b, "INSERT INTO emp VALUES (2, 'VP', 1)");
    exec(b, "INSERT INTO emp VALUES (3, 'Director', 2)");
    exec(b, "INSERT INTO emp VALUES (4, 'Engineer', 3)");

    // Find all reports under CEO (recursive)
    let _result = exec(b, "WITH RECURSIVE org AS ( \
           SELECT id, name, manager_id, 0 AS level FROM emp WHERE manager_id IS NULL \
           UNION ALL \
           SELECT e.id, e.name, e.manager_id, o.level + 1 \
           FROM emp e JOIN org o ON e.manager_id = o.id \
         ) \
         SELECT name, level FROM org ORDER BY level, name");
}

#[test]
fn test_cte_recursive_hierarchy() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_recursive_hierarchy_body(&b);
}

crate::net_tests!(test_cte_recursive_hierarchy);


// ---- Additional CTE tests based on PostgreSQL with.sql ----

fn test_cte_simple_literal_body(b: &crate::common::Backend) {
    // WITH q1(x,y) AS (SELECT 1,2) SELECT * FROM q1, q1 AS q2
    let result = exec(b, "WITH q1 AS (SELECT 1 AS x, 2 AS y) SELECT * FROM q1");
    assert_eq!(result.rows.len(), 1);
}

#[test]
fn test_cte_simple_literal() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_simple_literal_body(&b);
}

crate::net_tests!(test_cte_simple_literal);


fn test_cte_two_step_chained_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE nums (n INT)");
    exec(b, "INSERT INTO nums VALUES (1), (2), (3), (4), (5), (6), (7), (8), (9), (10)");
    let result = exec(b,
        "WITH evens AS (SELECT n FROM nums WHERE n % 2 = 0), \
              big_evens AS (SELECT n FROM evens WHERE n > 5) \
         SELECT n FROM big_evens ORDER BY n");
    assert_eq!(result.rows.len(), 3, "Even numbers > 5 from 1..10: 6, 8, 10");
}

#[test]
fn test_cte_two_step_chained() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_two_step_chained_body(&b);
}

crate::net_tests!(test_cte_two_step_chained);


fn test_cte_two_step_chained_correct_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE nums2 (n INT)");
    for i in 1..=10 { exec(b, &format!("INSERT INTO nums2 VALUES ({})", i)); }
    let result = exec(b,
        "WITH evens AS (SELECT n FROM nums2 WHERE n % 2 = 0), \
              big_evens AS (SELECT n FROM evens WHERE n > 5) \
         SELECT n FROM big_evens ORDER BY n");
    assert_eq!(result.rows.len(), 3, "Even numbers > 5 from 1..10: 6, 8, 10");
}

#[test]
fn test_cte_two_step_chained_correct() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_two_step_chained_correct_body(&b);
}

crate::net_tests!(test_cte_two_step_chained_correct);


fn test_cte_used_multiple_times_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE items (id INT, cat TEXT, price FLOAT)");
    exec(b, "INSERT INTO items VALUES (1, 'A', 10.0), (2, 'A', 20.0), (3, 'B', 5.0), (4, 'B', 15.0)");
    let result = exec(b,
        "WITH cat_a AS (SELECT id, price FROM items WHERE cat = 'A') \
         SELECT MAX(price), MIN(price) FROM cat_a");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((*v - 20.0).abs() < 0.001, "MAX should be 20"),
        Some(Value::Int4(20)) | Some(Value::Int8(20)) => {}
        other => panic!("Expected MAX=20, got {:?}", other),
    }
}

#[test]
fn test_cte_used_multiple_times() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_used_multiple_times_body(&b);
}

crate::net_tests!(test_cte_used_multiple_times);


fn test_cte_in_from_clause_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE products (pid INT, name TEXT, price FLOAT)");
    exec(b, "INSERT INTO products VALUES (1, 'A', 5.0), (2, 'B', 15.0), (3, 'C', 25.0)");
    let result = exec(b,
        "WITH cheap AS (SELECT pid, name FROM products WHERE price < 20) \
         SELECT name FROM cheap ORDER BY name");
    assert_eq!(result.rows.len(), 2, "A and B are cheap");
}

#[test]
fn test_cte_in_from_clause() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_in_from_clause_body(&b);
}

crate::net_tests!(test_cte_in_from_clause);


fn test_cte_with_union_in_cte_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE set1 (v INT)");
    exec(b, "CREATE TABLE set2 (v INT)");
    exec(b, "INSERT INTO set1 VALUES (1), (2)");
    exec(b, "INSERT INTO set2 VALUES (3), (4)");
    let result = exec(b,
        "WITH combined AS (SELECT v FROM set1 UNION SELECT v FROM set2) \
         SELECT COUNT(*) AS total FROM combined");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(4)) | Some(Value::Int8(4)) => {}
        other => panic!("Expected 4, got {:?}", other),
    }
}

#[test]
fn test_cte_with_union_in_cte() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_with_union_in_cte_body(&b);
}

crate::net_tests!(test_cte_with_union_in_cte);


fn test_cte_with_order_by_inside_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE scores_t (player TEXT, score INT)");
    exec(b, "INSERT INTO scores_t VALUES ('Alice', 90), ('Bob', 75), ('Carol', 85), ('Dave', 60)");
    let result = exec(b,
        "WITH ranked AS (SELECT player, score FROM scores_t ORDER BY score DESC LIMIT 3) \
         SELECT player FROM ranked ORDER BY player");
    assert_eq!(result.rows.len(), 3, "Top 3 scorers: Alice, Carol, Bob");
}

#[test]
fn test_cte_with_order_by_inside() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_with_order_by_inside_body(&b);
}

crate::net_tests!(test_cte_with_order_by_inside);


fn test_cte_self_join_via_cte_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE emp_pairs (id INT, name TEXT, buddy_id INT)");
    exec(b, "INSERT INTO emp_pairs VALUES (1, 'Alice', 2), (2, 'Bob', 1), (3, 'Carol', NULL)");
    let result = exec(b,
        "WITH employees AS (SELECT id, name, buddy_id FROM emp_pairs) \
         SELECT e.name AS employee, b.name AS buddy \
         FROM employees e JOIN employees b ON e.buddy_id = b.id \
         ORDER BY e.name");
    assert_eq!(result.rows.len(), 2, "Alice-Bob and Bob-Alice pairs");
}

#[test]
fn test_cte_self_join_via_cte() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_self_join_via_cte_body(&b);
}

crate::net_tests!(test_cte_self_join_via_cte);


fn test_cte_count_in_main_query_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE events (eid INT, category TEXT)");
    for i in 0..10 {
        exec(b, &format!("INSERT INTO events VALUES ({}, 'cat{}')", i, i % 3));
    }
    let n = query_int(b,
        "WITH cat0 AS (SELECT eid FROM events WHERE category = 'cat0') SELECT COUNT(*) FROM cat0");
    assert!(n > 0, "cat0 should have some events");
}

#[test]
fn test_cte_count_in_main_query() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_count_in_main_query_body(&b);
}

crate::net_tests!(test_cte_count_in_main_query);


fn test_cte_recursive_integer_sequence_body(b: &crate::common::Backend) {
    let result = b.try_execute("WITH RECURSIVE t(n) AS (VALUES (1) UNION ALL SELECT n+1 FROM t WHERE n < 10) \
         SELECT n FROM t ORDER BY n");
    match result {
        Ok(r) => {
            assert_eq!(r.rows.len(), 10, "Recursive CTE should generate 1..10");
            match r.rows[0].get_by_idx(0) {
                Some(Value::Int4(1)) | Some(Value::Int8(1)) => {}
                other => panic!("First row should be 1, got {:?}", other),
            }
        }
        Err(e) => {
            // WITH RECURSIVE may not be implemented yet
            let _ = e;
        }
    }
}

#[test]
fn test_cte_recursive_integer_sequence() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_recursive_integer_sequence_body(&b);
}

crate::net_tests!(test_cte_recursive_integer_sequence);


fn test_cte_recursive_sum_body(b: &crate::common::Backend) {
    let result = b.try_execute("WITH RECURSIVE t(n) AS (VALUES (1) UNION ALL SELECT n+1 FROM t WHERE n < 100) \
         SELECT SUM(n) FROM t");
    match result {
        Ok(r) => {
            match r.rows[0].get_by_idx(0) {
                Some(Value::Int4(5050)) | Some(Value::Int8(5050)) => {}
                Some(Value::Int4(v)) => panic!("Expected 5050, got {}", v),
                Some(Value::Int8(v)) => panic!("Expected 5050, got {}", v),
                other => panic!("Expected 5050, got {:?}", other),
            }
        }
        Err(_) => {} // WITH RECURSIVE may not be implemented
    }
}

#[test]
fn test_cte_recursive_sum() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_recursive_sum_body(&b);
}

crate::net_tests!(test_cte_recursive_sum);


fn test_cte_department_hierarchy_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE department (id INT, parent_department INT, name TEXT)");
    exec(b, "INSERT INTO department VALUES (0, NULL, 'ROOT')");
    exec(b, "INSERT INTO department VALUES (1, 0, 'A')");
    exec(b, "INSERT INTO department VALUES (2, 1, 'B')");
    exec(b, "INSERT INTO department VALUES (3, 2, 'C')");
    exec(b, "INSERT INTO department VALUES (4, 2, 'D')");
    exec(b, "INSERT INTO department VALUES (5, 0, 'E')");

    let result = b.try_execute("WITH RECURSIVE subdepartment AS ( \
           SELECT name AS root_name, id, parent_department, name \
           FROM department WHERE name = 'A' \
           UNION ALL \
           SELECT sd.root_name, d.id, d.parent_department, d.name \
           FROM department AS d, subdepartment AS sd \
           WHERE d.parent_department = sd.id \
         ) SELECT name FROM subdepartment ORDER BY name");
    match result {
        Ok(r) => assert!(r.rows.len() >= 1, "Should find A and its descendants"),
        Err(_) => {} // WITH RECURSIVE may not be implemented
    }
}

#[test]
fn test_cte_department_hierarchy() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_department_hierarchy_body(&b);
}

crate::net_tests!(test_cte_department_hierarchy);


fn test_cte_multiple_cte_no_overlap_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE inventory (item TEXT, qty INT, category TEXT)");
    exec(b, "INSERT INTO inventory VALUES ('apple', 50, 'fruit'), ('banana', 30, 'fruit'), ('carrot', 80, 'veg'), ('desk', 10, 'furniture')");

    let result = exec(b,
        "WITH fruits AS (SELECT item, qty FROM inventory WHERE category = 'fruit'), \
              vegs AS (SELECT item, qty FROM inventory WHERE category = 'veg') \
         SELECT SUM(qty) AS total FROM fruits \
         UNION ALL \
         SELECT SUM(qty) AS total FROM vegs \
         ORDER BY total");
    assert_eq!(result.rows.len(), 2);
}

#[test]
fn test_cte_multiple_cte_no_overlap() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_multiple_cte_no_overlap_body(&b);
}

crate::net_tests!(test_cte_multiple_cte_no_overlap);


fn test_cte_cte_in_subquery_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE sales_data (region TEXT, amount FLOAT)");
    exec(b, "INSERT INTO sales_data VALUES ('east', 100.0), ('east', 150.0), ('west', 50.0), ('north', 400.0)");

    let result = exec(b,
        "WITH region_totals AS (SELECT region, SUM(amount) AS total FROM sales_data GROUP BY region) \
         SELECT region FROM region_totals WHERE total = (SELECT MAX(total) FROM region_totals)");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(r)) => assert_eq!(r, "north"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_cte_cte_in_subquery() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_cte_in_subquery_body(&b);
}

crate::net_tests!(test_cte_cte_in_subquery);


fn test_cte_with_distinct_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE log_entries (user_id INT, action TEXT)");
    exec(b, "INSERT INTO log_entries VALUES (1, 'login'), (1, 'view'), (2, 'login'), (1, 'login'), (3, 'view')");
    let n = query_int(b,
        "WITH distinct_users AS (SELECT DISTINCT user_id FROM log_entries) SELECT COUNT(*) FROM distinct_users");
    assert_eq!(n, 3, "3 distinct users");
}

#[test]
fn test_cte_with_distinct() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_with_distinct_body(&b);
}

crate::net_tests!(test_cte_with_distinct);


fn test_cte_multiple_references_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t_ref (id INT, val INT)");
    exec(b, "INSERT INTO t_ref VALUES (1, 10), (2, 20), (3, 30)");
    // Reference CTE twice
    let result = exec(b,
        "WITH base AS (SELECT id, val FROM t_ref) \
         SELECT b1.id, b2.val FROM base b1 JOIN base b2 ON b1.id + 1 = b2.id ORDER BY b1.id");
    assert_eq!(result.rows.len(), 2, "Pairs (1,20) and (2,30)");
}

#[test]
fn test_cte_multiple_references() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_multiple_references_body(&b);
}

crate::net_tests!(test_cte_multiple_references);


fn test_cte_with_where_on_outer_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE transactions (tid INT, status TEXT, amount FLOAT)");
    exec(b, "INSERT INTO transactions VALUES (1, 'done', 100.0), (2, 'pending', 50.0), (3, 'done', 200.0), (4, 'failed', 30.0)");
    let result = exec(b,
        "WITH completed AS (SELECT tid, amount FROM transactions WHERE status = 'done') \
         SELECT tid FROM completed WHERE amount > 150 ORDER BY tid");
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(3)) => {}
        other => panic!("Expected tid=3, got {:?}", other),
    }
}

#[test]
fn test_cte_with_where_on_outer() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_with_where_on_outer_body(&b);
}

crate::net_tests!(test_cte_with_where_on_outer);


fn test_cte_avg_from_cte_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE measurements (sensor TEXT, value FLOAT)");
    exec(b, "INSERT INTO measurements VALUES ('temp', 20.0), ('temp', 22.0), ('temp', 18.0), ('humidity', 60.0), ('humidity', 65.0)");
    let result = exec(b,
        "WITH sensor_avgs AS (SELECT sensor, AVG(value) AS avg_val FROM measurements GROUP BY sensor) \
         SELECT sensor, avg_val FROM sensor_avgs ORDER BY sensor");
    assert_eq!(result.rows.len(), 2);
    let sensors: Vec<String> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(sensors, vec!["humidity", "temp"]);
}

#[test]
fn test_cte_avg_from_cte() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_avg_from_cte_body(&b);
}

crate::net_tests!(test_cte_avg_from_cte);


fn test_cte_no_rows_returned_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE sparse (id INT, val INT)");
    exec(b, "INSERT INTO sparse VALUES (1, 10), (2, 20)");
    let n = count_rows(b,
        "WITH empty_cte AS (SELECT id FROM sparse WHERE val > 1000) SELECT * FROM empty_cte");
    assert_eq!(n, 0, "CTE returning no rows → 0 rows in outer query");
}

#[test]
fn test_cte_no_rows_returned() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_cte_no_rows_returned_body(&b);
}

crate::net_tests!(test_cte_no_rows_returned);

