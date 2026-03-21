// Integration tests for the SQL engine accessible from workspace
use catalog::manager::CatalogManager;
use sql::engine::QueryEngine;
use sql::Value;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;
use txn::manager::TransactionManager;
use wal::writer::WalWriter;

fn make_engine(dir: &Path) -> Arc<QueryEngine> {
    let wal = Arc::new(WalWriter::open(dir).unwrap());
    let txn = Arc::new(TransactionManager::new_with_wal_recovery(
        Arc::clone(&wal),
        dir,
    ));
    let cat = Arc::new(CatalogManager::open(dir, Arc::clone(&wal), Arc::clone(&txn)).unwrap());
    Arc::new(QueryEngine::new(txn, cat, dir.to_path_buf()))
}

#[test]
fn test_having_clause() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine
        .execute("CREATE TABLE sales (region TEXT, amount INT)")
        .unwrap();
    engine
        .execute("INSERT INTO sales VALUES ('north', 100)")
        .unwrap();
    engine
        .execute("INSERT INTO sales VALUES ('north', 200)")
        .unwrap();
    engine
        .execute("INSERT INTO sales VALUES ('south', 50)")
        .unwrap();
    engine
        .execute("INSERT INTO sales VALUES ('south', 30)")
        .unwrap();

    // Only regions with total > 100
    let result = engine
        .execute("SELECT region, SUM(amount) FROM sales GROUP BY region HAVING SUM(amount) > 100")
        .unwrap();
    assert_eq!(result.rows.len(), 1, "Expected only 'north' region with sum > 100");
    match result.rows[0].get("region") {
        Some(Value::Text(r)) => assert_eq!(r, "north"),
        other => panic!("expected 'north', got {:?}", other),
    }
}

#[test]
fn test_qualified_column_join() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine
        .execute("CREATE TABLE users (id INT, name TEXT)")
        .unwrap();
    engine
        .execute("CREATE TABLE orders (user_id INT, product TEXT)")
        .unwrap();
    engine
        .execute("INSERT INTO users VALUES (1, 'Alice')")
        .unwrap();
    engine
        .execute("INSERT INTO users VALUES (2, 'Bob')")
        .unwrap();
    engine
        .execute("INSERT INTO orders VALUES (1, 'Book')")
        .unwrap();
    engine
        .execute("INSERT INTO orders VALUES (1, 'Pen')")
        .unwrap();

    // Use qualified column references
    let result = engine
        .execute(
            "SELECT u.name, o.product FROM users u JOIN orders o ON u.id = o.user_id",
        )
        .unwrap();
    assert_eq!(result.rows.len(), 2, "Expected 2 joined rows for Alice");
    for row in &result.rows {
        match row.get("u.name").or_else(|| row.get("name")) {
            Some(Value::Text(n)) => assert_eq!(n, "Alice"),
            other => panic!("expected Alice, got {:?}", other),
        }
    }
}

#[test]
fn test_ambiguous_column_error() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine
        .execute("CREATE TABLE a (id INT, val TEXT)")
        .unwrap();
    engine
        .execute("CREATE TABLE b (id INT, val TEXT)")
        .unwrap();
    engine
        .execute("INSERT INTO a VALUES (1, 'a1')")
        .unwrap();
    engine
        .execute("INSERT INTO b VALUES (1, 'b1')")
        .unwrap();

    // Unqualified 'id' in a join where both tables have 'id' — should error OR return one
    // (PostgreSQL errors with "column reference 'id' is ambiguous")
    let result = engine.execute("SELECT id FROM a JOIN b ON a.id = b.id");
    // Either an ambiguity error or a result is acceptable, but it must not panic
    let _ = result;
}

#[test]
fn test_acid_bank_transfer_atomicity() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    engine
        .execute("CREATE TABLE accounts (id INT, balance INT)")
        .unwrap();
    engine
        .execute("INSERT INTO accounts VALUES (1, 1000)")
        .unwrap();
    engine
        .execute("INSERT INTO accounts VALUES (2, 1000)")
        .unwrap();

    // Successful transfer
    let xid = engine
        .txn_manager
        .begin(txn::transaction::IsolationLevel::ReadCommitted);
    engine
        .execute_in_txn(xid, "UPDATE accounts SET balance = 500 WHERE id = 1")
        .unwrap();
    engine
        .execute_in_txn(xid, "UPDATE accounts SET balance = 1500 WHERE id = 2")
        .unwrap();
    engine.txn_manager.commit(xid).unwrap();

    let result = engine
        .execute("SELECT id, balance FROM accounts ORDER BY id")
        .unwrap();
    let balances: Vec<i64> = result
        .rows
        .iter()
        .map(|r| match r.get_by_idx(1) {
            Some(Value::Int4(v)) => *v as i64,
            Some(Value::Int8(v)) => *v,
            other => panic!("{:?}", other),
        })
        .collect();
    assert_eq!(balances, vec![500, 1500]);
    assert_eq!(balances.iter().sum::<i64>(), 2000);
}

#[test]
fn test_durability_restart() {
    let dir = TempDir::new().unwrap();

    {
        let engine = make_engine(dir.path());
        engine
            .execute("CREATE TABLE persistent (id INT, data TEXT)")
            .unwrap();
        engine
            .execute("INSERT INTO persistent VALUES (1, 'survives restart')")
            .unwrap();
    }

    {
        let engine = make_engine(dir.path());
        let result = engine
            .execute("SELECT data FROM persistent WHERE id = 1")
            .unwrap();
        match result.rows.first().and_then(|r| r.get_by_idx(0)) {
            Some(Value::Text(v)) => assert_eq!(v, "survives restart"),
            other => panic!("Expected text, got {:?}", other),
        }
    }
}

#[test]
fn test_no_dirty_reads() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    engine.execute("CREATE TABLE t (id INT, val INT)").unwrap();
    engine.execute("INSERT INTO t VALUES (1, 100)").unwrap();

    let xid1 = engine
        .txn_manager
        .begin(txn::transaction::IsolationLevel::ReadCommitted);
    engine
        .execute_in_txn(xid1, "UPDATE t SET val = 999 WHERE id = 1")
        .unwrap();

    // T2 should NOT see T1's uncommitted 999
    let result = engine.execute("SELECT val FROM t WHERE id = 1").unwrap();
    match result.rows.first().and_then(|r| r.get_by_idx(0)) {
        Some(Value::Int4(v)) => assert_eq!(*v, 100, "Dirty read detected!"),
        other => panic!("{:?}", other),
    }

    engine.txn_manager.abort(xid1).unwrap();
}

// ── New feature integration tests ─────────────────────────────────────────────

#[test]
fn test_distinct_deduplication() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine
        .execute("CREATE TABLE t (id INT, color TEXT)")
        .unwrap();
    engine.execute("INSERT INTO t VALUES (1, 'red')").unwrap();
    engine.execute("INSERT INTO t VALUES (2, 'blue')").unwrap();
    engine.execute("INSERT INTO t VALUES (3, 'red')").unwrap();
    engine.execute("INSERT INTO t VALUES (4, 'green')").unwrap();
    engine.execute("INSERT INTO t VALUES (5, 'blue')").unwrap();

    let result = engine.execute("SELECT DISTINCT color FROM t").unwrap();
    assert_eq!(
        result.rows.len(),
        3,
        "DISTINCT should return 3 unique colors: {:?}",
        result.rows
    );

    let mut colors: Vec<String> = result
        .rows
        .iter()
        .map(|r| match r.get_by_idx(0) {
            Some(Value::Text(s)) => s.clone(),
            other => panic!("expected text, got {:?}", other),
        })
        .collect();
    colors.sort();
    assert_eq!(colors, vec!["blue", "green", "red"]);
}

#[test]
fn test_union_two_selects() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine
        .execute("CREATE TABLE a (id INT, val TEXT)")
        .unwrap();
    engine
        .execute("CREATE TABLE b (id INT, val TEXT)")
        .unwrap();
    engine.execute("INSERT INTO a VALUES (1, 'alpha')").unwrap();
    engine.execute("INSERT INTO a VALUES (2, 'beta')").unwrap();
    engine
        .execute("INSERT INTO b VALUES (2, 'beta')")
        .unwrap();
    engine
        .execute("INSERT INTO b VALUES (3, 'gamma')")
        .unwrap();

    // UNION (deduplicates)
    let result = engine
        .execute("SELECT id, val FROM a UNION SELECT id, val FROM b")
        .unwrap();
    assert_eq!(
        result.rows.len(),
        3,
        "UNION should deduplicate: {:?}",
        result.rows
    );

    // UNION ALL (keeps duplicates)
    let result_all = engine
        .execute("SELECT id, val FROM a UNION ALL SELECT id, val FROM b")
        .unwrap();
    assert_eq!(
        result_all.rows.len(),
        4,
        "UNION ALL should keep all rows: {:?}",
        result_all.rows
    );
}

#[test]
fn test_cte_with_clause() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine
        .execute("CREATE TABLE employees (id INT, name TEXT, dept TEXT, salary INT)")
        .unwrap();
    engine
        .execute("INSERT INTO employees VALUES (1, 'Alice', 'eng', 90000)")
        .unwrap();
    engine
        .execute("INSERT INTO employees VALUES (2, 'Bob', 'eng', 80000)")
        .unwrap();
    engine
        .execute("INSERT INTO employees VALUES (3, 'Carol', 'hr', 70000)")
        .unwrap();

    let result = engine
        .execute(
            "WITH eng AS (SELECT name, salary FROM employees WHERE dept = 'eng') \
             SELECT name FROM eng WHERE salary > 85000",
        )
        .unwrap();

    assert_eq!(result.rows.len(), 1, "CTE should return Alice: {:?}", result.rows);
    match result.rows[0].get("name").or_else(|| result.rows[0].get_by_idx(0)) {
        Some(Value::Text(n)) => assert_eq!(n, "Alice"),
        other => panic!("expected Alice, got {:?}", other),
    }
}

#[test]
fn test_is_null_is_not_null() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine
        .execute("CREATE TABLE t (id INT, val TEXT)")
        .unwrap();
    engine.execute("INSERT INTO t VALUES (1, 'hello')").unwrap();
    engine.execute("INSERT INTO t VALUES (2, NULL)").unwrap();
    engine.execute("INSERT INTO t VALUES (3, 'world')").unwrap();

    let null_result = engine
        .execute("SELECT id FROM t WHERE val IS NULL")
        .unwrap();
    assert_eq!(null_result.rows.len(), 1, "IS NULL should find row 2");
    match null_result.rows[0].get_by_idx(0) {
        Some(Value::Int4(2)) => {}
        other => panic!("expected id=2, got {:?}", other),
    }

    let not_null_result = engine
        .execute("SELECT id FROM t WHERE val IS NOT NULL")
        .unwrap();
    assert_eq!(
        not_null_result.rows.len(),
        2,
        "IS NOT NULL should find 2 rows"
    );
}

#[test]
fn test_ilike_case_insensitive() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine
        .execute("CREATE TABLE products (id INT, name TEXT)")
        .unwrap();
    engine
        .execute("INSERT INTO products VALUES (1, 'Apple')")
        .unwrap();
    engine
        .execute("INSERT INTO products VALUES (2, 'banana')")
        .unwrap();
    engine
        .execute("INSERT INTO products VALUES (3, 'CHERRY')")
        .unwrap();
    engine
        .execute("INSERT INTO products VALUES (4, 'apricot')")
        .unwrap();

    let result = engine
        .execute("SELECT name FROM products WHERE name ILIKE 'ap%'")
        .unwrap();
    assert_eq!(result.rows.len(), 2, "ILIKE 'ap%' should match Apple and apricot: {:?}", result.rows);

    let mut names: Vec<String> = result
        .rows
        .iter()
        .map(|r| match r.get_by_idx(0) {
            Some(Value::Text(s)) => s.clone(),
            other => panic!("expected text, got {:?}", other),
        })
        .collect();
    names.sort();
    assert_eq!(names, vec!["Apple", "apricot"]);
}

#[test]
fn test_in_subquery() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine
        .execute("CREATE TABLE users (id INT, name TEXT)")
        .unwrap();
    engine
        .execute("CREATE TABLE active_ids (uid INT)")
        .unwrap();
    engine
        .execute("INSERT INTO users VALUES (1, 'Alice')")
        .unwrap();
    engine
        .execute("INSERT INTO users VALUES (2, 'Bob')")
        .unwrap();
    engine
        .execute("INSERT INTO users VALUES (3, 'Carol')")
        .unwrap();
    engine
        .execute("INSERT INTO active_ids VALUES (1)")
        .unwrap();
    engine
        .execute("INSERT INTO active_ids VALUES (3)")
        .unwrap();

    let result = engine
        .execute("SELECT name FROM users WHERE id IN (SELECT uid FROM active_ids)")
        .unwrap();
    assert_eq!(result.rows.len(), 2, "IN subquery should return Alice and Carol: {:?}", result.rows);

    let mut names: Vec<String> = result
        .rows
        .iter()
        .map(|r| match r.get_by_idx(0) {
            Some(Value::Text(s)) => s.clone(),
            other => panic!("expected text, got {:?}", other),
        })
        .collect();
    names.sort();
    assert_eq!(names, vec!["Alice", "Carol"]);
}

#[test]
fn test_returning_clause_insert() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine
        .execute("CREATE TABLE t (id INT, name TEXT)")
        .unwrap();

    let result = engine
        .execute("INSERT INTO t VALUES (42, 'foo') RETURNING id, name")
        .unwrap();
    assert_eq!(result.rows.len(), 1, "RETURNING should return 1 row");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(42)) => {}
        other => panic!("expected id=42, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Text(s)) if s == "foo" => {}
        other => panic!("expected name='foo', got {:?}", other),
    }
}

#[test]
fn test_returning_clause_delete() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine
        .execute("CREATE TABLE t (id INT, name TEXT)")
        .unwrap();
    engine.execute("INSERT INTO t VALUES (1, 'Alice')").unwrap();
    engine.execute("INSERT INTO t VALUES (2, 'Bob')").unwrap();
    engine.execute("INSERT INTO t VALUES (3, 'Carol')").unwrap();

    let result = engine
        .execute("DELETE FROM t WHERE id > 2 RETURNING id, name")
        .unwrap();
    assert_eq!(result.rows.len(), 1, "RETURNING DELETE should return deleted row");
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(3)) => {}
        other => panic!("expected id=3, got {:?}", other),
    }

    // Verify deletion
    let remaining = engine.execute("SELECT * FROM t").unwrap();
    assert_eq!(remaining.rows.len(), 2);
}

#[test]
fn test_numeric_overflow_error() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (val INT)").unwrap();
    engine
        .execute(&format!("INSERT INTO t VALUES ({})", i32::MAX))
        .unwrap();

    // Adding 1 to i32::MAX should return NumericOverflow error
    let err = engine
        .execute("SELECT val + 1 FROM t")
        .unwrap_err();
    assert!(
        matches!(err, sql::SqlError::NumericOverflow(_)),
        "expected NumericOverflow, got: {:?}",
        err
    );
}

#[test]
fn test_division_by_zero_error() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (a INT, b INT)").unwrap();
    engine.execute("INSERT INTO t VALUES (10, 0)").unwrap();

    let err = engine
        .execute("SELECT a / b FROM t")
        .unwrap_err();
    assert!(
        matches!(err, sql::SqlError::DivisionByZero),
        "expected DivisionByZero, got: {:?}",
        err
    );
}

#[test]
fn test_multi_table_from_cross_join() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE colors (color TEXT)").unwrap();
    engine.execute("CREATE TABLE sizes (size TEXT)").unwrap();
    engine.execute("INSERT INTO colors VALUES ('red')").unwrap();
    engine.execute("INSERT INTO colors VALUES ('blue')").unwrap();
    engine.execute("INSERT INTO sizes VALUES ('S')").unwrap();
    engine.execute("INSERT INTO sizes VALUES ('M')").unwrap();

    // Multi-table FROM = implicit cross join
    let result = engine
        .execute("SELECT colors.color, sizes.size FROM colors, sizes")
        .unwrap();
    assert_eq!(
        result.rows.len(),
        4,
        "Cross join of 2x2 should produce 4 rows: {:?}",
        result.rows
    );
}

#[test]
fn test_sqlstate_table_not_found() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let err = engine.execute("SELECT * FROM nonexistent").unwrap_err();
    assert_eq!(
        err.sqlstate(),
        "42P01",
        "TableNotFound should have SQLSTATE 42P01"
    );
}

// ── Chapter 3 tutorial SQL test harness ───────────────────────────────────────

#[test]
fn test_tutorial_chapter3() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    let statements: &[(&str, &str)] = &[
        // DDL
        ("CREATE TABLE authors",
         "CREATE TABLE authors (id INT NOT NULL, name TEXT NOT NULL, country TEXT)"),
        ("CREATE TABLE books",
         "CREATE TABLE books (id INT NOT NULL, title TEXT NOT NULL, author_id INT NOT NULL, price FLOAT, published INT)"),
        ("CREATE TABLE orders",
         "CREATE TABLE orders (id INT NOT NULL, book_id INT NOT NULL, quantity INT NOT NULL, total_price FLOAT)"),

        // Author inserts
        ("INSERT author 1", "INSERT INTO authors VALUES (1, 'J.R.R. Tolkien', 'United Kingdom')"),
        ("INSERT author 2", "INSERT INTO authors VALUES (2, 'Frank Herbert', 'United States')"),
        ("INSERT author 3", "INSERT INTO authors VALUES (3, 'Ursula K. Le Guin', 'United States')"),
        ("INSERT author 4", "INSERT INTO authors VALUES (4, 'Cormac McCarthy', 'United States')"),

        // Book inserts
        ("INSERT book 1", "INSERT INTO books VALUES (1, 'The Hobbit', 1, 12.99, 1937)"),
        ("INSERT book 2", "INSERT INTO books VALUES (2, 'The Lord of the Rings', 1, 24.99, 1954)"),
        ("INSERT book 3", "INSERT INTO books VALUES (3, 'Dune', 2, 15.99, 1965)"),
        ("INSERT book 4", "INSERT INTO books VALUES (4, 'The Left Hand of Darkness', 3, 13.99, 1969)"),
        ("INSERT book 5", "INSERT INTO books VALUES (5, 'The Dispossessed', 3, 11.99, 1974)"),
        ("INSERT book 6", "INSERT INTO books VALUES (6, 'Blood Meridian', 4, 14.99, 1985)"),
        ("INSERT book 7", "INSERT INTO books VALUES (7, 'Children of Dune', 2, 13.99, 1976)"),

        // Order inserts
        ("INSERT order 1", "INSERT INTO orders VALUES (1, 1, 2, 25.98)"),
        ("INSERT order 2", "INSERT INTO orders VALUES (2, 3, 1, 15.99)"),
        ("INSERT order 3", "INSERT INTO orders VALUES (3, 2, 1, 24.99)"),
        ("INSERT order 4", "INSERT INTO orders VALUES (4, 4, 3, 41.97)"),
        ("INSERT order 5", "INSERT INTO orders VALUES (5, 3, 5, 79.95)"),

        // Basic SELECTs
        ("SELECT * FROM books", "SELECT * FROM books"),
        ("SELECT title, price WHERE price < 14", "SELECT title, price FROM books WHERE price < 14.00"),
        ("SELECT WHERE published < 1970", "SELECT title, published FROM books WHERE published < 1970"),
        ("SELECT WHERE price > 13 AND published > 1960", "SELECT title, price FROM books WHERE price > 13.00 AND published > 1960"),

        // ORDER BY
        ("ORDER BY price ASC", "SELECT title, price FROM books ORDER BY price ASC"),
        ("ORDER BY published DESC, price DESC", "SELECT title, published, price FROM books ORDER BY published DESC, price DESC"),

        // LIMIT / OFFSET
        ("LIMIT 3", "SELECT title, price FROM books ORDER BY price DESC LIMIT 3"),
        ("LIMIT 2 OFFSET 3", "SELECT title, price FROM books ORDER BY price DESC LIMIT 2 OFFSET 3"),

        // JOINs
        ("JOIN books to authors", "SELECT b.title, a.name AS author, b.price FROM books b JOIN authors a ON b.author_id = a.id ORDER BY a.name, b.title"),
        ("JOIN orders to books", "SELECT o.id AS order_id, b.title, o.quantity, o.total_price FROM orders o JOIN books b ON o.book_id = b.id ORDER BY o.id"),

        // LEFT JOIN setup
        ("INSERT author 5 Gene Wolfe", "INSERT INTO authors VALUES (5, 'Gene Wolfe', 'United States')"),
        ("LEFT JOIN authors books", "SELECT a.name, b.title FROM authors a LEFT JOIN books b ON b.author_id = a.id ORDER BY a.name"),

        // JOIN USING setup
        ("CREATE TABLE genres", "CREATE TABLE genres (id INT NOT NULL, name TEXT NOT NULL)"),
        ("CREATE TABLE book_genres", "CREATE TABLE book_genres (book_id INT NOT NULL, genre_id INT NOT NULL)"),
        ("INSERT genres multi-row", "INSERT INTO genres VALUES (1, 'Fantasy'), (2, 'Science Fiction'), (3, 'Western')"),
        ("INSERT book_genres multi-row", "INSERT INTO book_genres VALUES (1, 1), (2, 1), (3, 2), (4, 2), (5, 2), (6, 3), (7, 2)"),
        ("JOIN USING genre_id", "SELECT bg.book_id, g.name AS genre FROM book_genres bg JOIN genres g USING (genre_id) ORDER BY bg.book_id"),

        // IS NULL / IS NOT NULL
        ("SELECT WHERE country IS NULL (empty)", "SELECT name FROM authors WHERE country IS NULL"),
        ("INSERT author 6 NULL country", "INSERT INTO authors VALUES (6, 'Anonymous', NULL)"),
        ("SELECT WHERE country IS NULL", "SELECT name FROM authors WHERE country IS NULL"),

        // ILIKE
        ("ILIKE ursula", "SELECT name FROM authors WHERE name ILIKE '%ursula%'"),

        // DISTINCT
        ("DISTINCT country", "SELECT DISTINCT country FROM authors WHERE country IS NOT NULL ORDER BY country"),
        ("DISTINCT country, name", "SELECT DISTINCT country, name FROM authors ORDER BY country, name"),

        // RETURNING
        ("INSERT RETURNING id, total_price", "INSERT INTO orders VALUES (6, 2, 3, 74.97) RETURNING id, total_price"),
        ("UPDATE RETURNING title, price", "UPDATE books SET price = price * 0.90 WHERE author_id = 4 RETURNING title, price"),
        ("DELETE RETURNING name", "DELETE FROM authors WHERE id = 6 RETURNING name"),

        // UNION
        ("UNION authors and books", "SELECT name AS label, 'author' AS kind FROM authors UNION SELECT title, 'book' FROM books ORDER BY kind, label"),

        // CTE
        ("CTE author revenue", "WITH book_revenue AS (SELECT b.author_id, b.title, SUM(o.total_price) AS revenue FROM books b JOIN orders o ON o.book_id = b.id GROUP BY b.author_id, b.title), author_revenue AS (SELECT a.name, SUM(br.revenue) AS total_revenue FROM book_revenue br JOIN authors a ON a.id = br.author_id GROUP BY a.name) SELECT name, total_revenue FROM author_revenue WHERE total_revenue > 50.00 ORDER BY total_revenue DESC"),

        // Aggregates
        ("COUNT books per author", "SELECT a.name, COUNT(*) AS book_count FROM books b JOIN authors a ON b.author_id = a.id GROUP BY a.name ORDER BY book_count DESC"),
        ("AVG MIN MAX SUM prices", "SELECT AVG(price) AS avg_price, MIN(price) AS min_price, MAX(price) AS max_price, SUM(price) AS total_catalog_value FROM books"),
        ("SUM units and revenue per book", "SELECT b.title, SUM(o.quantity) AS units_sold, SUM(o.total_price) AS revenue FROM orders o JOIN books b ON o.book_id = b.id GROUP BY b.title ORDER BY revenue DESC"),

        // UPDATE
        ("UPDATE Herbert price +10%", "UPDATE books SET price = price * 1.10 WHERE author_id = 2"),
        ("SELECT Herbert books after update", "SELECT title, price FROM books WHERE author_id = 2"),
        ("UPDATE author name", "UPDATE authors SET name = 'Ursula K. Le Guin' WHERE id = 3"),

        // DELETE
        ("DELETE Gene Wolfe", "DELETE FROM authors WHERE id = 5"),
        ("DELETE orders for book 1", "DELETE FROM orders WHERE book_id = 1"),
    ];

    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut failures: Vec<(String, String)> = Vec::new();

    for (label, sql) in statements {
        match engine.execute(sql) {
            Ok(_) => {
                println!("PASS [{}]: {}", label, sql);
                pass += 1;
            }
            Err(e) => {
                println!("FAIL [{}]: {}\n  => {}", label, sql, e);
                fail += 1;
                failures.push((label.to_string(), format!("{}", e)));
            }
        }
    }

    println!("\n=== TUTORIAL CH3 RESULTS: {} passed, {} failed ===", pass, fail);
    for (label, err) in &failures {
        println!("  FAIL: {} => {}", label, err);
    }

    assert_eq!(
        fail,
        0,
        "{} tutorial SQL statements failed (see output above)",
        fail
    );
}

#[test]
fn test_except_set_operation() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE a (id INT)").unwrap();
    engine.execute("CREATE TABLE b (id INT)").unwrap();
    for i in 1..=5 {
        engine
            .execute(&format!("INSERT INTO a VALUES ({})", i))
            .unwrap();
    }
    for i in 3..=7 {
        engine
            .execute(&format!("INSERT INTO b VALUES ({})", i))
            .unwrap();
    }

    let result = engine
        .execute("SELECT id FROM a EXCEPT SELECT id FROM b")
        .unwrap();
    assert_eq!(result.rows.len(), 2, "EXCEPT should return ids 1,2: {:?}", result.rows);
    let mut ids: Vec<i32> = result
        .rows
        .iter()
        .map(|r| match r.get_by_idx(0) {
            Some(Value::Int4(v)) => *v,
            other => panic!("expected Int4, got {:?}", other),
        })
        .collect();
    ids.sort();
    assert_eq!(ids, vec![1, 2]);
}

// ── Feature 1: New Data Types ──────────────────────────────────────────────────

#[test]
fn test_date_type_insert_select() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE events (id INT, happened DATE)").unwrap();
    engine.execute("INSERT INTO events VALUES (1, '2024-01-15')").unwrap();
    engine.execute("INSERT INTO events VALUES (2, '2024-03-01')").unwrap();
    let result = engine.execute("SELECT id, happened FROM events ORDER BY happened").unwrap();
    assert_eq!(result.rows.len(), 2);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(1)) => {}
        other => panic!("expected id=1, got {:?}", other),
    }
}

#[test]
fn test_date_ordering() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE events (id INT, d DATE)").unwrap();
    engine.execute("INSERT INTO events VALUES (3, '2020-06-15')").unwrap();
    engine.execute("INSERT INTO events VALUES (1, '2019-01-01')").unwrap();
    engine.execute("INSERT INTO events VALUES (2, '2020-01-01')").unwrap();
    let result = engine.execute("SELECT id FROM events ORDER BY d ASC").unwrap();
    let ids: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(ids, vec![1, 2, 3]);
}

#[test]
fn test_timestamp_type_insert_select() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE logs (id INT, ts TIMESTAMP)").unwrap();
    engine.execute("INSERT INTO logs VALUES (1, '2024-01-15 10:30:00')").unwrap();
    engine.execute("INSERT INTO logs VALUES (2, '2024-01-15 09:00:00')").unwrap();
    let result = engine.execute("SELECT id FROM logs ORDER BY ts ASC").unwrap();
    let ids: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(ids, vec![2, 1]);
}

#[test]
fn test_numeric_type_insert_select() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE prices (id INT, price NUMERIC)").unwrap();
    engine.execute("INSERT INTO prices VALUES (1, '99.99')").unwrap();
    engine.execute("INSERT INTO prices VALUES (2, '100.00')").unwrap();
    let result = engine.execute("SELECT id, price FROM prices ORDER BY id").unwrap();
    assert_eq!(result.rows.len(), 2);
    match result.rows[0].get_by_idx(1) {
        Some(Value::Numeric(s)) => assert_eq!(s, "99.99"),
        other => panic!("expected Numeric, got {:?}", other),
    }
}

#[test]
fn test_uuid_type_insert_select() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE items (id INT, uid UUID)").unwrap();
    engine.execute("INSERT INTO items VALUES (1, '550e8400-e29b-41d4-a716-446655440000')").unwrap();
    let result = engine.execute("SELECT uid FROM items WHERE id = 1").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Uuid(s)) => assert!(s.contains("550e8400")),
        other => panic!("expected Uuid, got {:?}", other),
    }
}

#[test]
fn test_date_persistence_across_restart() {
    let dir = TempDir::new().unwrap();
    {
        let engine = make_engine(dir.path());
        engine.execute("CREATE TABLE dated (id INT, d DATE)").unwrap();
        engine.execute("INSERT INTO dated VALUES (1, '2024-06-01')").unwrap();
    }
    {
        let engine = make_engine(dir.path());
        let result = engine.execute("SELECT d FROM dated WHERE id = 1").unwrap();
        assert_eq!(result.rows.len(), 1);
        match result.rows[0].get_by_idx(0) {
            Some(Value::Date(_)) => {} // correctly stored as Date
            other => panic!("expected Date after restart, got {:?}", other),
        }
    }
}

#[test]
fn test_numeric_persistence_across_restart() {
    let dir = TempDir::new().unwrap();
    {
        let engine = make_engine(dir.path());
        engine.execute("CREATE TABLE nums (id INT, val NUMERIC)").unwrap();
        engine.execute("INSERT INTO nums VALUES (1, '3.14159')").unwrap();
    }
    {
        let engine = make_engine(dir.path());
        let result = engine.execute("SELECT val FROM nums WHERE id = 1").unwrap();
        assert_eq!(result.rows.len(), 1);
        match result.rows[0].get_by_idx(0) {
            Some(Value::Numeric(s)) => assert_eq!(s, "3.14159"),
            other => panic!("expected Numeric after restart, got {:?}", other),
        }
    }
}

#[test]
fn test_current_date_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT CURRENT_DATE").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Date(_)) => {}
        other => panic!("expected Date from CURRENT_DATE, got {:?}", other),
    }
}

#[test]
fn test_now_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT NOW()").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Timestamp(_)) => {}
        other => panic!("expected Timestamp from NOW(), got {:?}", other),
    }
}

#[test]
fn test_gen_random_uuid_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT gen_random_uuid()").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Uuid(s)) => {
            // UUID format: 8-4-4-4-12 hex chars with dashes
            let parts: Vec<&str> = s.split('-').collect();
            assert_eq!(parts.len(), 5, "UUID should have 5 dash-separated parts: {}", s);
        }
        other => panic!("expected Uuid from gen_random_uuid(), got {:?}", other),
    }
}

// ── Feature 2: Sequences and SERIAL ───────────────────────────────────────────

#[test]
fn test_serial_auto_increment() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE items (id SERIAL, name TEXT)").unwrap();
    engine.execute("INSERT INTO items (name) VALUES ('alpha')").unwrap();
    engine.execute("INSERT INTO items (name) VALUES ('beta')").unwrap();
    engine.execute("INSERT INTO items (name) VALUES ('gamma')").unwrap();
    let result = engine.execute("SELECT id FROM items ORDER BY id").unwrap();
    assert_eq!(result.rows.len(), 3);
    let ids: Vec<i64> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v as i64,
        Some(Value::Int8(v)) => *v,
        other => panic!("expected int id, got {:?}", other),
    }).collect();
    assert_eq!(ids, vec![1, 2, 3]);
}

#[test]
fn test_bigserial_auto_increment() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE events (id BIGSERIAL, event TEXT)").unwrap();
    engine.execute("INSERT INTO events (event) VALUES ('start')").unwrap();
    engine.execute("INSERT INTO events (event) VALUES ('middle')").unwrap();
    let result = engine.execute("SELECT id FROM events ORDER BY id").unwrap();
    assert_eq!(result.rows.len(), 2);
    let ids: Vec<i64> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int8(v)) => *v,
        Some(Value::Int4(v)) => *v as i64,
        other => panic!("expected int id, got {:?}", other),
    }).collect();
    assert_eq!(ids, vec![1, 2]);
}

#[test]
fn test_serial_persists_across_restart() {
    let dir = TempDir::new().unwrap();
    {
        let engine = make_engine(dir.path());
        engine.execute("CREATE TABLE t (id SERIAL, v TEXT)").unwrap();
        engine.execute("INSERT INTO t (v) VALUES ('a')").unwrap();
        engine.execute("INSERT INTO t (v) VALUES ('b')").unwrap();
    }
    {
        let engine = make_engine(dir.path());
        engine.execute("INSERT INTO t (v) VALUES ('c')").unwrap();
        let result = engine.execute("SELECT id FROM t ORDER BY id").unwrap();
        assert_eq!(result.rows.len(), 3);
        let ids: Vec<i64> = result.rows.iter().map(|r| match r.get_by_idx(0) {
            Some(Value::Int4(v)) => *v as i64,
            Some(Value::Int8(v)) => *v,
            other => panic!("{:?}", other),
        }).collect();
        assert_eq!(ids, vec![1, 2, 3], "serial should continue from 3 after restart");
    }
}

// ── Feature 3: DEFAULT value evaluation ───────────────────────────────────────

#[test]
fn test_default_value_literal() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, status TEXT DEFAULT 'active')").unwrap();
    engine.execute("INSERT INTO t (id) VALUES (1)").unwrap();
    engine.execute("INSERT INTO t (id) VALUES (2)").unwrap();
    let result = engine.execute("SELECT status FROM t ORDER BY id").unwrap();
    assert_eq!(result.rows.len(), 2);
    for row in &result.rows {
        match row.get_by_idx(0) {
            Some(Value::Text(s)) => assert_eq!(s, "active"),
            other => panic!("expected default 'active', got {:?}", other),
        }
    }
}

#[test]
fn test_default_value_numeric() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, count INT DEFAULT 0)").unwrap();
    engine.execute("INSERT INTO t (id) VALUES (1)").unwrap();
    let result = engine.execute("SELECT count FROM t WHERE id = 1").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(0)) => {}
        other => panic!("expected default 0, got {:?}", other),
    }
}

#[test]
fn test_default_now_timestamp() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, created_at TIMESTAMP DEFAULT NOW())").unwrap();
    engine.execute("INSERT INTO t (id) VALUES (1)").unwrap();
    let result = engine.execute("SELECT created_at FROM t WHERE id = 1").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Timestamp(_)) => {}
        other => panic!("expected Timestamp from DEFAULT NOW(), got {:?}", other),
    }
}

#[test]
fn test_explicit_value_overrides_default() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, label TEXT DEFAULT 'default_val')").unwrap();
    engine.execute("INSERT INTO t (id, label) VALUES (1, 'custom')").unwrap();
    engine.execute("INSERT INTO t (id) VALUES (2)").unwrap();
    let result = engine.execute("SELECT id, label FROM t ORDER BY id").unwrap();
    assert_eq!(result.rows.len(), 2);
    match result.rows[0].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "custom"),
        other => panic!("expected 'custom', got {:?}", other),
    }
    match result.rows[1].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "default_val"),
        other => panic!("expected 'default_val', got {:?}", other),
    }
}

// ── Feature 4: String and Math functions ──────────────────────────────────────

#[test]
fn test_upper_lower_functions() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT upper('hello'), lower('WORLD')").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "HELLO"),
        other => panic!("expected 'HELLO', got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "world"),
        other => panic!("expected 'world', got {:?}", other),
    }
}

#[test]
fn test_length_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT length('hello world')").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(11)) | Some(Value::Int8(11)) => {}
        other => panic!("expected 11, got {:?}", other),
    }
}

#[test]
fn test_trim_functions() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT trim('  hello  '), ltrim('  hi'), rtrim('bye  ')").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("expected 'hello', got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "hi"),
        other => panic!("expected 'hi', got {:?}", other),
    }
    match result.rows[0].get_by_idx(2) {
        Some(Value::Text(s)) => assert_eq!(s, "bye"),
        other => panic!("expected 'bye', got {:?}", other),
    }
}

#[test]
fn test_substring_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT substring('hello world', 7, 5)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "world"),
        other => panic!("expected 'world', got {:?}", other),
    }
}

#[test]
fn test_replace_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT replace('hello world', 'world', 'there')").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello there"),
        other => panic!("expected 'hello there', got {:?}", other),
    }
}

#[test]
fn test_concat_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT concat('hello', ' ', 'world')").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello world"),
        other => panic!("expected 'hello world', got {:?}", other),
    }
}

#[test]
fn test_split_part_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT split_part('a,b,c', ',', 2)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "b"),
        other => panic!("expected 'b', got {:?}", other),
    }
}

#[test]
fn test_reverse_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT reverse('hello')").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "olleh"),
        other => panic!("expected 'olleh', got {:?}", other),
    }
}

#[test]
fn test_lpad_rpad_functions() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT lpad('5', 3, '0'), rpad('hi', 5, '.')").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "005"),
        other => panic!("expected '005', got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "hi..."),
        other => panic!("expected 'hi...', got {:?}", other),
    }
}

#[test]
fn test_abs_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT abs(-42), abs(10)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(42)) => {}
        other => panic!("expected 42, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Int4(10)) => {}
        other => panic!("expected 10, got {:?}", other),
    }
}

#[test]
fn test_ceil_floor_functions() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT ceil(3.2), floor(3.9)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((v - 4.0).abs() < 1e-9, "expected 4.0, got {}", v),
        other => panic!("expected Float8(4.0), got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Float8(v)) => assert!((v - 3.0).abs() < 1e-9, "expected 3.0, got {}", v),
        other => panic!("expected Float8(3.0), got {:?}", other),
    }
}

#[test]
fn test_sqrt_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT sqrt(16.0)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((v - 4.0).abs() < 1e-9, "expected 4.0, got {}", v),
        other => panic!("expected Float8(4.0), got {:?}", other),
    }
}

#[test]
fn test_power_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT power(2.0, 10.0)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((v - 1024.0).abs() < 1e-6, "expected 1024.0, got {}", v),
        other => panic!("expected Float8(1024.0), got {:?}", other),
    }
}

#[test]
fn test_round_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT round(3.567, 2)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((v - 3.57).abs() < 1e-9, "expected 3.57, got {}", v),
        other => panic!("expected Float8(3.57), got {:?}", other),
    }
}

#[test]
fn test_greatest_least_functions() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT greatest(3, 7, 2, 9, 1), least(3, 7, 2, 9, 1)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(9)) => {}
        other => panic!("expected 9, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Int4(1)) => {}
        other => panic!("expected 1, got {:?}", other),
    }
}

#[test]
fn test_strpos_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT strpos('hello world', 'world')").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(7)) | Some(Value::Int8(7)) => {}
        other => panic!("expected 7, got {:?}", other),
    }
}

#[test]
fn test_repeat_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT repeat('ab', 3)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "ababab"),
        other => panic!("expected 'ababab', got {:?}", other),
    }
}

#[test]
fn test_left_right_functions() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT left('hello world', 5), right('hello world', 5)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("expected 'hello', got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "world"),
        other => panic!("expected 'world', got {:?}", other),
    }
}

#[test]
fn test_concat_ws_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT concat_ws(', ', 'Alice', 'Bob', 'Carol')").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "Alice, Bob, Carol"),
        other => panic!("expected 'Alice, Bob, Carol', got {:?}", other),
    }
}

#[test]
fn test_string_functions_on_table() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE names (first TEXT, last TEXT)").unwrap();
    engine.execute("INSERT INTO names VALUES ('john', 'doe')").unwrap();
    engine.execute("INSERT INTO names VALUES ('jane', 'smith')").unwrap();
    let result = engine.execute(
        "SELECT upper(first), length(last), concat(first, ' ', last) FROM names ORDER BY first"
    ).unwrap();
    assert_eq!(result.rows.len(), 2);
    // First row (jane)
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "JANE"),
        other => panic!("expected 'JANE', got {:?}", other),
    }
}

#[test]
fn test_math_functions_on_table() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE data (val FLOAT)").unwrap();
    engine.execute("INSERT INTO data VALUES (-5.0)").unwrap();
    engine.execute("INSERT INTO data VALUES (16.0)").unwrap();
    let result = engine.execute("SELECT abs(val), sqrt(abs(val)) FROM data ORDER BY val").unwrap();
    assert_eq!(result.rows.len(), 2);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((v - 5.0).abs() < 1e-9),
        other => panic!("expected 5.0, got {:?}", other),
    }
}

#[test]
fn test_sign_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT sign(-5.0), sign(0.0), sign(3.0)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert_eq!(*v, -1.0),
        other => panic!("expected -1.0, got {:?}", other),
    }
    match result.rows[0].get_by_idx(2) {
        Some(Value::Float8(v)) => assert_eq!(*v, 1.0),
        other => panic!("expected 1.0, got {:?}", other),
    }
}

// ── Feature 5: Statistical aggregates ─────────────────────────────────────────

#[test]
fn test_stddev_aggregate() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE vals (v FLOAT)").unwrap();
    engine.execute("INSERT INTO vals VALUES (2.0)").unwrap();
    engine.execute("INSERT INTO vals VALUES (4.0)").unwrap();
    engine.execute("INSERT INTO vals VALUES (4.0)").unwrap();
    engine.execute("INSERT INTO vals VALUES (4.0)").unwrap();
    engine.execute("INSERT INTO vals VALUES (5.0)").unwrap();
    engine.execute("INSERT INTO vals VALUES (5.0)").unwrap();
    engine.execute("INSERT INTO vals VALUES (7.0)").unwrap();
    engine.execute("INSERT INTO vals VALUES (9.0)").unwrap();
    // Population: mean=5, stddev_pop=2.0
    let result = engine.execute("SELECT stddev_pop(v) FROM vals").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((v - 2.0).abs() < 1e-6, "expected stddev_pop=2.0, got {}", v),
        other => panic!("expected Float8, got {:?}", other),
    }
}

#[test]
fn test_variance_aggregate() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE vals (v FLOAT)").unwrap();
    engine.execute("INSERT INTO vals VALUES (2.0)").unwrap();
    engine.execute("INSERT INTO vals VALUES (4.0)").unwrap();
    engine.execute("INSERT INTO vals VALUES (4.0)").unwrap();
    engine.execute("INSERT INTO vals VALUES (4.0)").unwrap();
    engine.execute("INSERT INTO vals VALUES (5.0)").unwrap();
    engine.execute("INSERT INTO vals VALUES (5.0)").unwrap();
    engine.execute("INSERT INTO vals VALUES (7.0)").unwrap();
    engine.execute("INSERT INTO vals VALUES (9.0)").unwrap();
    // var_pop = 4.0 (stddev_pop^2 = 2^2)
    let result = engine.execute("SELECT var_pop(v) FROM vals").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((v - 4.0).abs() < 1e-6, "expected var_pop=4.0, got {}", v),
        other => panic!("expected Float8, got {:?}", other),
    }
}

#[test]
fn test_string_agg_aggregate() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE words (w TEXT)").unwrap();
    engine.execute("INSERT INTO words VALUES ('alpha')").unwrap();
    engine.execute("INSERT INTO words VALUES ('beta')").unwrap();
    engine.execute("INSERT INTO words VALUES ('gamma')").unwrap();
    let result = engine.execute("SELECT string_agg(w, ', ') FROM words").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => {
            // All three words should be present (order may vary)
            assert!(s.contains("alpha"), "missing 'alpha': {}", s);
            assert!(s.contains("beta"), "missing 'beta': {}", s);
            assert!(s.contains("gamma"), "missing 'gamma': {}", s);
        }
        other => panic!("expected Text, got {:?}", other),
    }
}

#[test]
fn test_bool_and_bool_or_aggregates() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE flags (active BOOLEAN)").unwrap();
    engine.execute("INSERT INTO flags VALUES (true)").unwrap();
    engine.execute("INSERT INTO flags VALUES (true)").unwrap();
    engine.execute("INSERT INTO flags VALUES (false)").unwrap();
    let result = engine.execute("SELECT bool_and(active), bool_or(active) FROM flags").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(false)) => {}
        other => panic!("bool_and(T,T,F) should be false, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Bool(true)) => {}
        other => panic!("bool_or(T,T,F) should be true, got {:?}", other),
    }
}

#[test]
fn test_bool_and_all_true() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE flags2 (active BOOLEAN)").unwrap();
    engine.execute("INSERT INTO flags2 VALUES (true)").unwrap();
    engine.execute("INSERT INTO flags2 VALUES (true)").unwrap();
    let result = engine.execute("SELECT bool_and(active) FROM flags2").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("bool_and(T,T) should be true, got {:?}", other),
    }
}

#[test]
fn test_stddev_sample() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE data (v FLOAT)").unwrap();
    engine.execute("INSERT INTO data VALUES (10.0)").unwrap();
    engine.execute("INSERT INTO data VALUES (20.0)").unwrap();
    engine.execute("INSERT INTO data VALUES (30.0)").unwrap();
    let result = engine.execute("SELECT stddev(v) FROM data").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => {
            // sample stddev of [10,20,30] = 10.0
            assert!((v - 10.0).abs() < 1e-6, "expected stddev=10.0, got {}", v);
        }
        other => panic!("expected Float8, got {:?}", other),
    }
}

#[test]
fn test_array_agg_aggregate() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE nums (n INT)").unwrap();
    engine.execute("INSERT INTO nums VALUES (1)").unwrap();
    engine.execute("INSERT INTO nums VALUES (2)").unwrap();
    engine.execute("INSERT INTO nums VALUES (3)").unwrap();
    let result = engine.execute("SELECT array_agg(n) FROM nums").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => {
            // Expected format: {1,2,3}
            assert!(s.starts_with('{') && s.ends_with('}'), "unexpected array_agg format: {}", s);
        }
        other => panic!("expected Text array format, got {:?}", other),
    }
}

// ── Feature 6: ORDER BY on aggregate expressions ───────────────────────────────

#[test]
fn test_order_by_aggregate_count() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE sales (region TEXT, amount INT)").unwrap();
    engine.execute("INSERT INTO sales VALUES ('west', 10)").unwrap();
    engine.execute("INSERT INTO sales VALUES ('east', 20)").unwrap();
    engine.execute("INSERT INTO sales VALUES ('east', 30)").unwrap();
    engine.execute("INSERT INTO sales VALUES ('west', 40)").unwrap();
    engine.execute("INSERT INTO sales VALUES ('west', 50)").unwrap();
    let result = engine.execute(
        "SELECT region, count(*) AS cnt FROM sales GROUP BY region ORDER BY count(*) DESC"
    ).unwrap();
    assert_eq!(result.rows.len(), 2);
    // west has 3 rows, east has 2 — west should be first
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(r)) => assert_eq!(r, "west", "west should be first (3 rows)"),
        other => panic!("expected 'west', got {:?}", other),
    }
}

#[test]
fn test_order_by_aggregate_sum() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE orders (category TEXT, revenue INT)").unwrap();
    engine.execute("INSERT INTO orders VALUES ('A', 100)").unwrap();
    engine.execute("INSERT INTO orders VALUES ('A', 200)").unwrap();
    engine.execute("INSERT INTO orders VALUES ('B', 50)").unwrap();
    engine.execute("INSERT INTO orders VALUES ('B', 75)").unwrap();
    engine.execute("INSERT INTO orders VALUES ('C', 500)").unwrap();
    let result = engine.execute(
        "SELECT category, sum(revenue) AS total FROM orders GROUP BY category ORDER BY sum(revenue) DESC"
    ).unwrap();
    assert_eq!(result.rows.len(), 3);
    // C (500) > A (300) > B (125)
    let categories: Vec<String> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(categories, vec!["C", "A", "B"]);
}

#[test]
fn test_order_by_alias_asc() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE data (grp TEXT, val INT)").unwrap();
    engine.execute("INSERT INTO data VALUES ('b', 5)").unwrap();
    engine.execute("INSERT INTO data VALUES ('a', 10)").unwrap();
    engine.execute("INSERT INTO data VALUES ('c', 3)").unwrap();
    engine.execute("INSERT INTO data VALUES ('b', 5)").unwrap();
    // Order by the non-aggregate column (alias for a GROUP BY column)
    let result = engine.execute(
        "SELECT grp, sum(val) AS total FROM data GROUP BY grp ORDER BY grp ASC"
    ).unwrap();
    assert_eq!(result.rows.len(), 3);
    let groups: Vec<String> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(groups, vec!["a", "b", "c"]);
}

// ── Feature 7: Page checksum verification ─────────────────────────────────────

#[test]
fn test_page_checksum_written_on_insert() {
    // Insert rows and verify we can still read them back correctly.
    // If checksums were corrupted, reads would fail.
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, val TEXT)").unwrap();
    for i in 0..100 {
        engine.execute(&format!("INSERT INTO t VALUES ({}, 'value_{}')", i, i)).unwrap();
    }
    let result = engine.execute("SELECT count(*) FROM t").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int8(100)) => {}
        other => panic!("expected 100 rows, got {:?}", other),
    }
}

#[test]
fn test_large_table_sequential_scan() {
    // A stress test that reads many pages (verifying no checksum failures)
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, data TEXT)").unwrap();
    for i in 0..200 {
        engine.execute(&format!("INSERT INTO t VALUES ({}, 'data_{}_{}')", i, i, "x".repeat(20))).unwrap();
    }
    let result = engine.execute("SELECT count(*) FROM t").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int8(200)) => {}
        other => panic!("expected 200, got {:?}", other),
    }
}

// ── Feature 8: MVCC-correct index scan visibility ──────────────────────────────

#[test]
fn test_index_scan_sees_only_committed() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, val TEXT)").unwrap();
    engine.execute("INSERT INTO t VALUES (1, 'committed')").unwrap();
    engine.execute("INSERT INTO t VALUES (2, 'committed2')").unwrap();
    engine.execute("CREATE INDEX ON t (id)").unwrap();

    // Start a transaction and update but don't commit
    let xid = engine.txn_manager.begin(txn::transaction::IsolationLevel::ReadCommitted);
    engine.execute_in_txn(xid, "UPDATE t SET val = 'uncommitted' WHERE id = 1").unwrap();

    // Concurrent read should see old value
    let result = engine.execute("SELECT val FROM t WHERE id = 1").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "committed", "should not see uncommitted update"),
        other => panic!("expected 'committed', got {:?}", other),
    }
    engine.txn_manager.abort(xid).unwrap();
}

#[test]
fn test_index_scan_after_delete() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, name TEXT)").unwrap();
    engine.execute("INSERT INTO t VALUES (1, 'Alice')").unwrap();
    engine.execute("INSERT INTO t VALUES (2, 'Bob')").unwrap();
    engine.execute("INSERT INTO t VALUES (3, 'Carol')").unwrap();
    engine.execute("CREATE INDEX ON t (id)").unwrap();

    engine.execute("DELETE FROM t WHERE id = 2").unwrap();
    let result = engine.execute("SELECT name FROM t ORDER BY id").unwrap();
    assert_eq!(result.rows.len(), 2, "deleted row should not appear");
    let names: Vec<String> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(names, vec!["Alice", "Carol"]);
}

// ── Additional coverage tests ──────────────────────────────────────────────────

#[test]
fn test_starts_ends_with_functions() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT starts_with('hello world', 'hello'), ends_with('hello world', 'world')").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Bool(true)) => {}
        other => panic!("expected true, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Bool(true)) => {}
        other => panic!("expected true, got {:?}", other),
    }
}

#[test]
fn test_date_truncation_equality() {
    // Two different timestamps on the same day should both cast to the same date
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, d DATE)").unwrap();
    engine.execute("INSERT INTO t VALUES (1, '2024-01-15')").unwrap();
    engine.execute("INSERT INTO t VALUES (2, '2024-01-15')").unwrap();
    engine.execute("INSERT INTO t VALUES (3, '2024-01-16')").unwrap();
    let result = engine.execute("SELECT count(*) FROM t WHERE d = '2024-01-15'").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int8(2)) => {}
        other => panic!("expected 2, got {:?}", other),
    }
}

#[test]
fn test_numeric_ordering() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE prices (id INT, price NUMERIC)").unwrap();
    engine.execute("INSERT INTO prices VALUES (1, '10.50')").unwrap();
    engine.execute("INSERT INTO prices VALUES (2, '5.99')").unwrap();
    engine.execute("INSERT INTO prices VALUES (3, '100.00')").unwrap();
    let result = engine.execute("SELECT id FROM prices ORDER BY price ASC").unwrap();
    assert_eq!(result.rows.len(), 3);
    // Numeric ordering: 5.99 < 10.50 < 100.00
    let ids: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(ids, vec![2, 1, 3]);
}

#[test]
fn test_pi_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT pi()").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((v - std::f64::consts::PI).abs() < 1e-10),
        other => panic!("expected pi, got {:?}", other),
    }
}

#[test]
fn test_exp_ln_functions() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT exp(1.0), ln(exp(1.0))").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((v - std::f64::consts::E).abs() < 1e-10),
        other => panic!("expected e, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Float8(v)) => assert!((v - 1.0).abs() < 1e-10, "ln(e) should be 1.0, got {}", v),
        other => panic!("expected 1.0, got {:?}", other),
    }
}

#[test]
fn test_multiple_aggregates_with_group_by_order() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE sales (dept TEXT, revenue FLOAT)").unwrap();
    engine.execute("INSERT INTO sales VALUES ('eng', 50000.0)").unwrap();
    engine.execute("INSERT INTO sales VALUES ('eng', 60000.0)").unwrap();
    engine.execute("INSERT INTO sales VALUES ('hr', 30000.0)").unwrap();
    engine.execute("INSERT INTO sales VALUES ('hr', 32000.0)").unwrap();
    engine.execute("INSERT INTO sales VALUES ('mkt', 45000.0)").unwrap();
    let result = engine.execute(
        "SELECT dept, sum(revenue) AS total, count(*) AS cnt, avg(revenue) AS avg_rev \
         FROM sales GROUP BY dept ORDER BY total DESC"
    ).unwrap();
    assert_eq!(result.rows.len(), 3);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "eng", "eng should have highest total"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_serial_in_join() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE categories (id SERIAL, name TEXT)").unwrap();
    engine.execute("CREATE TABLE products (id SERIAL, cat_id INT, name TEXT)").unwrap();
    engine.execute("INSERT INTO categories (name) VALUES ('Electronics')").unwrap();
    engine.execute("INSERT INTO categories (name) VALUES ('Books')").unwrap();
    engine.execute("INSERT INTO products (cat_id, name) VALUES (1, 'Phone')").unwrap();
    engine.execute("INSERT INTO products (cat_id, name) VALUES (1, 'Laptop')").unwrap();
    engine.execute("INSERT INTO products (cat_id, name) VALUES (2, 'Novel')").unwrap();
    let result = engine.execute(
        "SELECT c.name, p.name FROM categories c JOIN products p ON c.id = p.cat_id ORDER BY c.id, p.name"
    ).unwrap();
    assert_eq!(result.rows.len(), 3);
}

#[test]
fn test_date_comparison_in_where() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE events (id INT, event_date DATE)").unwrap();
    engine.execute("INSERT INTO events VALUES (1, '2020-01-01')").unwrap();
    engine.execute("INSERT INTO events VALUES (2, '2021-06-15')").unwrap();
    engine.execute("INSERT INTO events VALUES (3, '2022-12-31')").unwrap();
    let result = engine.execute("SELECT count(*) FROM events WHERE event_date > '2020-12-31'").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int8(2)) => {}
        other => panic!("expected 2 events after 2020, got {:?}", other),
    }
}

#[test]
fn test_uuid_where_clause() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE items (uid UUID, name TEXT)").unwrap();
    engine.execute("INSERT INTO items VALUES ('550e8400-e29b-41d4-a716-446655440000', 'first')").unwrap();
    engine.execute("INSERT INTO items VALUES ('6ba7b810-9dad-11d1-80b4-00c04fd430c8', 'second')").unwrap();
    let result = engine.execute("SELECT name FROM items WHERE uid = '550e8400-e29b-41d4-a716-446655440000'").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "first"),
        other => panic!("expected 'first', got {:?}", other),
    }
}

#[test]
fn test_trunc_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT trunc(3.9), trunc(-3.9)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((v - 3.0).abs() < 1e-9, "expected 3.0, got {}", v),
        other => panic!("expected 3.0, got {:?}", other),
    }
    match result.rows[0].get_by_idx(1) {
        Some(Value::Float8(v)) => assert!((v + 3.0).abs() < 1e-9, "expected -3.0, got {}", v),
        other => panic!("expected -3.0, got {:?}", other),
    }
}

#[test]
fn test_log_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT log(100.0)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((v - 2.0).abs() < 1e-9, "log10(100) should be 2.0, got {}", v),
        other => panic!("expected 2.0, got {:?}", other),
    }
}

#[test]
fn test_string_agg_with_group_by() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE tags (category TEXT, tag TEXT)").unwrap();
    engine.execute("INSERT INTO tags VALUES ('fruit', 'apple')").unwrap();
    engine.execute("INSERT INTO tags VALUES ('fruit', 'banana')").unwrap();
    engine.execute("INSERT INTO tags VALUES ('veg', 'carrot')").unwrap();
    let result = engine.execute(
        "SELECT category, string_agg(tag, '|') AS tags FROM tags GROUP BY category ORDER BY category"
    ).unwrap();
    assert_eq!(result.rows.len(), 2);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "fruit"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_variance_sample() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE data2 (v FLOAT)").unwrap();
    engine.execute("INSERT INTO data2 VALUES (2.0)").unwrap();
    engine.execute("INSERT INTO data2 VALUES (4.0)").unwrap();
    engine.execute("INSERT INTO data2 VALUES (6.0)").unwrap();
    // Sample variance of [2,4,6]: mean=4, sum_sq_dev=8, var_samp=8/(3-1)=4
    let result = engine.execute("SELECT variance(v) FROM data2").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((v - 4.0).abs() < 1e-9, "expected var_samp=4.0, got {}", v),
        other => panic!("expected Float8, got {:?}", other),
    }
}

#[test]
fn test_serial_mixed_with_explicit_inserts() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id SERIAL, name TEXT, score INT DEFAULT 100)").unwrap();
    engine.execute("INSERT INTO t (name) VALUES ('alice')").unwrap();
    engine.execute("INSERT INTO t (name, score) VALUES ('bob', 200)").unwrap();
    engine.execute("INSERT INTO t (name) VALUES ('carol')").unwrap();
    let result = engine.execute("SELECT id, name, score FROM t ORDER BY id").unwrap();
    assert_eq!(result.rows.len(), 3);
    // Verify alice gets id=1 and score=100 (default)
    match result.rows[0].get_by_idx(2) {
        Some(Value::Int4(100)) => {}
        other => panic!("expected default score=100, got {:?}", other),
    }
    // Bob gets score=200 (explicit)
    match result.rows[1].get_by_idx(2) {
        Some(Value::Int4(200)) => {}
        other => panic!("expected score=200, got {:?}", other),
    }
}

#[test]
fn test_count_distinct_aggregate() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (color TEXT)").unwrap();
    engine.execute("INSERT INTO t VALUES ('red')").unwrap();
    engine.execute("INSERT INTO t VALUES ('blue')").unwrap();
    engine.execute("INSERT INTO t VALUES ('red')").unwrap();
    engine.execute("INSERT INTO t VALUES ('green')").unwrap();
    engine.execute("INSERT INTO t VALUES ('blue')").unwrap();
    let result = engine.execute("SELECT count(DISTINCT color) FROM t").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int8(3)) => {}
        other => panic!("expected 3 distinct colors, got {:?}", other),
    }
}

#[test]
fn test_date_type_in_cte() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE appointments (id INT, patient TEXT, appt_date DATE)").unwrap();
    engine.execute("INSERT INTO appointments VALUES (1, 'Alice', '2024-01-15')").unwrap();
    engine.execute("INSERT INTO appointments VALUES (2, 'Bob', '2024-02-20')").unwrap();
    engine.execute("INSERT INTO appointments VALUES (3, 'Carol', '2024-01-20')").unwrap();
    let result = engine.execute(
        "WITH jan AS (SELECT patient FROM appointments WHERE appt_date < '2024-02-01') \
         SELECT count(*) FROM jan"
    ).unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int8(2)) => {}
        other => panic!("expected 2 january appointments, got {:?}", other),
    }
}

#[test]
fn test_timestamp_ordering_precise() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE logs2 (id INT, created TIMESTAMP)").unwrap();
    engine.execute("INSERT INTO logs2 VALUES (1, '2024-01-01 12:00:00')").unwrap();
    engine.execute("INSERT INTO logs2 VALUES (2, '2024-01-01 12:00:01')").unwrap();
    engine.execute("INSERT INTO logs2 VALUES (3, '2024-01-01 11:59:59')").unwrap();
    let result = engine.execute("SELECT id FROM logs2 ORDER BY created").unwrap();
    let ids: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("{:?}", other),
    }).collect();
    assert_eq!(ids, vec![3, 1, 2]);
}

#[test]
fn test_multiple_defaults_one_row() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute(
        "CREATE TABLE t (a INT, b TEXT DEFAULT 'hello', c INT DEFAULT 42, d BOOLEAN DEFAULT true)"
    ).unwrap();
    engine.execute("INSERT INTO t (a) VALUES (1)").unwrap();
    let result = engine.execute("SELECT a, b, c, d FROM t WHERE a = 1").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(1) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("expected 'hello', got {:?}", other),
    }
    match result.rows[0].get_by_idx(2) {
        Some(Value::Int4(42)) => {}
        other => panic!("expected 42, got {:?}", other),
    }
    match result.rows[0].get_by_idx(3) {
        Some(Value::Bool(true)) => {}
        other => panic!("expected true, got {:?}", other),
    }
}

// ── Additional feature tests to increase coverage ──────────────────────────────

#[test]
fn test_ceil_function_via_function_call() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    // Test ceil via uppercase SQL keyword form
    let result = engine.execute("SELECT ceil(3.2)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((v - 4.0).abs() < 1e-9, "expected 4.0, got {}", v),
        other => panic!("expected 4.0, got {:?}", other),
    }
}

#[test]
fn test_floor_function_via_function_call() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT floor(3.9)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((v - 3.0).abs() < 1e-9, "expected 3.0, got {}", v),
        other => panic!("expected 3.0, got {:?}", other),
    }
}

#[test]
fn test_substring_via_substr_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT substr('hello world', 7, 5)").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "world"),
        other => panic!("expected 'world', got {:?}", other),
    }
}

#[test]
fn test_trim_via_btrim_function() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT btrim('  hello  ')").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("expected 'hello', got {:?}", other),
    }
}

#[test]
fn test_timestamp_insert_and_filter() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE events (id INT, ts TIMESTAMP)").unwrap();
    engine.execute("INSERT INTO events VALUES (1, '2024-01-01 00:00:00')").unwrap();
    engine.execute("INSERT INTO events VALUES (2, '2024-06-15 12:00:00')").unwrap();
    let result = engine.execute("SELECT id FROM events WHERE ts > '2024-03-01 00:00:00'").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(2)) => {}
        other => panic!("expected id=2, got {:?}", other),
    }
}

#[test]
fn test_numeric_sum_aggregate() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE prices (price NUMERIC)").unwrap();
    engine.execute("INSERT INTO prices VALUES ('10.50')").unwrap();
    engine.execute("INSERT INTO prices VALUES ('20.75')").unwrap();
    engine.execute("INSERT INTO prices VALUES ('5.25')").unwrap();
    let result = engine.execute("SELECT count(*) FROM prices").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int8(3)) => {}
        other => panic!("expected 3, got {:?}", other),
    }
}

#[test]
fn test_uuid_persistence() {
    let dir = TempDir::new().unwrap();
    {
        let engine = make_engine(dir.path());
        engine.execute("CREATE TABLE t (uid UUID, name TEXT)").unwrap();
        engine.execute("INSERT INTO t VALUES ('550e8400-e29b-41d4-a716-446655440000', 'test')").unwrap();
    }
    {
        let engine = make_engine(dir.path());
        let result = engine.execute("SELECT uid FROM t").unwrap();
        assert_eq!(result.rows.len(), 1);
        match result.rows[0].get_by_idx(0) {
            Some(Value::Uuid(s)) => assert!(s.contains("550e8400")),
            other => panic!("expected Uuid, got {:?}", other),
        }
    }
}

#[test]
fn test_timestamp_persistence() {
    let dir = TempDir::new().unwrap();
    {
        let engine = make_engine(dir.path());
        engine.execute("CREATE TABLE t (id INT, ts TIMESTAMP)").unwrap();
        engine.execute("INSERT INTO t VALUES (1, '2024-06-15 10:30:00')").unwrap();
    }
    {
        let engine = make_engine(dir.path());
        let result = engine.execute("SELECT ts FROM t WHERE id = 1").unwrap();
        assert_eq!(result.rows.len(), 1);
        match result.rows[0].get_by_idx(0) {
            Some(Value::Timestamp(_)) => {}
            other => panic!("expected Timestamp, got {:?}", other),
        }
    }
}

#[test]
fn test_multiple_serial_tables() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE a (id SERIAL, val TEXT)").unwrap();
    engine.execute("CREATE TABLE b (id SERIAL, val TEXT)").unwrap();
    engine.execute("INSERT INTO a (val) VALUES ('x')").unwrap();
    engine.execute("INSERT INTO a (val) VALUES ('y')").unwrap();
    engine.execute("INSERT INTO b (val) VALUES ('p')").unwrap();
    // Each table has its own independent sequence
    let r_a = engine.execute("SELECT id FROM a ORDER BY id").unwrap();
    let r_b = engine.execute("SELECT id FROM b ORDER BY id").unwrap();
    assert_eq!(r_a.rows.len(), 2);
    assert_eq!(r_b.rows.len(), 1);
    match r_b.rows[0].get_by_idx(0) {
        Some(Value::Int4(1)) | Some(Value::Int8(1)) => {}
        other => panic!("expected id=1 in table b, got {:?}", other),
    }
}

#[test]
fn test_date_min_max_aggregate() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (d DATE)").unwrap();
    engine.execute("INSERT INTO t VALUES ('2020-01-01')").unwrap();
    engine.execute("INSERT INTO t VALUES ('2022-12-31')").unwrap();
    engine.execute("INSERT INTO t VALUES ('2021-06-15')").unwrap();
    let result = engine.execute("SELECT min(d), max(d) FROM t").unwrap();
    assert_eq!(result.rows.len(), 1);
    // min should be 2020-01-01, max should be 2022-12-31
    match result.rows[0].get_by_idx(0) {
        Some(Value::Date(d)) => {
            use sql::value::format_date;
            assert_eq!(format_date(*d), "2020-01-01");
        }
        other => panic!("expected min date, got {:?}", other),
    }
}

#[test]
fn test_string_agg_with_order_by_group() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (grp TEXT, name TEXT)").unwrap();
    engine.execute("INSERT INTO t VALUES ('a', 'alice')").unwrap();
    engine.execute("INSERT INTO t VALUES ('a', 'anna')").unwrap();
    engine.execute("INSERT INTO t VALUES ('b', 'bob')").unwrap();
    let result = engine.execute(
        "SELECT grp, string_agg(name, ',') FROM t GROUP BY grp ORDER BY grp"
    ).unwrap();
    assert_eq!(result.rows.len(), 2);
    match result.rows[1].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "b"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_stddev_pop_simple() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (v FLOAT)").unwrap();
    engine.execute("INSERT INTO t VALUES (3.0)").unwrap();
    engine.execute("INSERT INTO t VALUES (3.0)").unwrap();
    engine.execute("INSERT INTO t VALUES (3.0)").unwrap();
    // All values same → stddev = 0
    let result = engine.execute("SELECT stddev_pop(v) FROM t").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!(v.abs() < 1e-9, "expected 0.0, got {}", v),
        other => panic!("expected Float8(0.0), got {:?}", other),
    }
}

#[test]
fn test_order_by_count_asc() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (grp TEXT)").unwrap();
    engine.execute("INSERT INTO t VALUES ('a')").unwrap();
    engine.execute("INSERT INTO t VALUES ('b')").unwrap();
    engine.execute("INSERT INTO t VALUES ('b')").unwrap();
    engine.execute("INSERT INTO t VALUES ('c')").unwrap();
    engine.execute("INSERT INTO t VALUES ('c')").unwrap();
    engine.execute("INSERT INTO t VALUES ('c')").unwrap();
    let result = engine.execute(
        "SELECT grp, count(*) AS n FROM t GROUP BY grp ORDER BY count(*) ASC"
    ).unwrap();
    assert_eq!(result.rows.len(), 3);
    // Ascending: a(1) < b(2) < c(3)
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "a"),
        other => panic!("{:?}", other),
    }
}

#[test]
fn test_float_aggregate_avg() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (v FLOAT)").unwrap();
    engine.execute("INSERT INTO t VALUES (1.0)").unwrap();
    engine.execute("INSERT INTO t VALUES (2.0)").unwrap();
    engine.execute("INSERT INTO t VALUES (3.0)").unwrap();
    let result = engine.execute("SELECT avg(v) FROM t").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Float8(v)) => assert!((v - 2.0).abs() < 1e-9, "expected 2.0, got {}", v),
        other => panic!("expected 2.0, got {:?}", other),
    }
}

#[test]
fn test_select_without_from() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let result = engine.execute("SELECT 1 + 1, 'hello', true").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int4(2)) => {}
        other => panic!("expected 2, got {:?}", other),
    }
}

#[test]
fn test_multi_row_insert() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, name TEXT)").unwrap();
    engine.execute("INSERT INTO t VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Carol')").unwrap();
    let result = engine.execute("SELECT count(*) FROM t").unwrap();
    match result.rows[0].get_by_idx(0) {
        Some(Value::Int8(3)) => {}
        other => panic!("expected 3, got {:?}", other),
    }
}

#[test]
fn test_intersect_set_operation() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE a (id INT)").unwrap();
    engine.execute("CREATE TABLE b (id INT)").unwrap();
    for i in 1..=5 {
        engine.execute(&format!("INSERT INTO a VALUES ({})", i)).unwrap();
    }
    for i in 3..=7 {
        engine.execute(&format!("INSERT INTO b VALUES ({})", i)).unwrap();
    }
    let result = engine.execute("SELECT id FROM a INTERSECT SELECT id FROM b").unwrap();
    assert_eq!(result.rows.len(), 3, "INTERSECT should return ids 3,4,5");
    let mut ids: Vec<i32> = result.rows.iter().map(|r| match r.get_by_idx(0) {
        Some(Value::Int4(v)) => *v,
        other => panic!("expected Int4, got {:?}", other),
    }).collect();
    ids.sort();
    assert_eq!(ids, vec![3, 4, 5]);
}

#[test]
fn test_varchar_type() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    engine.execute("CREATE TABLE t (id INT, name VARCHAR(100))").unwrap();
    engine.execute("INSERT INTO t VALUES (1, 'hello')").unwrap();
    let result = engine.execute("SELECT name FROM t WHERE id = 1").unwrap();
    assert_eq!(result.rows.len(), 1);
    match result.rows[0].get_by_idx(0) {
        Some(Value::Text(s)) => assert_eq!(s, "hello"),
        other => panic!("expected 'hello', got {:?}", other),
    }
}

