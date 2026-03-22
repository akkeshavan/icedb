/// Comprehensive tutorial validation test
///
/// Extracts and runs every SQL statement from:
///   - icedb-book/ch03-quickstart.md
///   - icedb-book/ch04-sql-reference.md
///
/// Run with:
///   cargo test -p icedb-tests tutorial_validation -- --nocapture
use std::sync::Arc;
use tempfile::TempDir;
use crate::common::{make_engine, exec_session};

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

fn ok(engine: &Arc<sql::engine::QueryEngine>, sql: &str) {
    engine
        .execute(sql)
        .unwrap_or_else(|e| panic!("Expected OK:\n  SQL: {sql}\n  Err: {e}"));
}

fn err(engine: &Arc<sql::engine::QueryEngine>, sql: &str) {
    engine
        .execute(sql)
        .expect_err(&format!("Expected error for: {sql}"));
}

fn rows_count(engine: &Arc<sql::engine::QueryEngine>, sql: &str, expected: usize) {
    let r = engine
        .execute(sql)
        .unwrap_or_else(|e| panic!("Query failed:\n  SQL: {sql}\n  Err: {e}"));
    assert_eq!(
        r.rows.len(),
        expected,
        "Wrong row count for: {sql}"
    );
}

// ---------------------------------------------------------------------------
// Shared bookstore schema setup
// ---------------------------------------------------------------------------

fn setup_bookstore(engine: &Arc<sql::engine::QueryEngine>) {
    // --- ch03: DDL ---
    ok(engine, "CREATE TABLE authors (
        id       INT NOT NULL,
        name     TEXT NOT NULL,
        country  TEXT
    )");
    ok(engine, "CREATE TABLE books (
        id          INT NOT NULL,
        title       TEXT NOT NULL,
        author_id   INT NOT NULL,
        price       FLOAT,
        published   INT
    )");
    ok(engine, "CREATE TABLE orders (
        id          INT NOT NULL,
        book_id     INT NOT NULL,
        quantity    INT NOT NULL,
        total_price FLOAT
    )");

    // --- ch03: Author inserts ---
    ok(engine, "INSERT INTO authors VALUES (1, 'J.R.R. Tolkien', 'United Kingdom')");
    ok(engine, "INSERT INTO authors VALUES (2, 'Frank Herbert', 'United States')");
    ok(engine, "INSERT INTO authors VALUES (3, 'Ursula K. Le Guin', 'United States')");
    ok(engine, "INSERT INTO authors VALUES (4, 'Cormac McCarthy', 'United States')");

    // --- ch03: Book inserts ---
    ok(engine, "INSERT INTO books VALUES (1, 'The Hobbit', 1, 12.99, 1937)");
    ok(engine, "INSERT INTO books VALUES (2, 'The Lord of the Rings', 1, 24.99, 1954)");
    ok(engine, "INSERT INTO books VALUES (3, 'Dune', 2, 15.99, 1965)");
    ok(engine, "INSERT INTO books VALUES (4, 'The Left Hand of Darkness', 3, 13.99, 1969)");
    ok(engine, "INSERT INTO books VALUES (5, 'The Dispossessed', 3, 11.99, 1974)");
    ok(engine, "INSERT INTO books VALUES (6, 'Blood Meridian', 4, 14.99, 1985)");
    ok(engine, "INSERT INTO books VALUES (7, 'Children of Dune', 2, 13.99, 1976)");

    // --- ch03: Order inserts ---
    ok(engine, "INSERT INTO orders VALUES (1, 1, 2, 25.98)");
    ok(engine, "INSERT INTO orders VALUES (2, 3, 1, 15.99)");
    ok(engine, "INSERT INTO orders VALUES (3, 2, 1, 24.99)");
    ok(engine, "INSERT INTO orders VALUES (4, 4, 3, 41.97)");
    ok(engine, "INSERT INTO orders VALUES (5, 3, 5, 79.95)");

    // --- ch03: Genre tables (needed for JOIN USING) ---
    ok(engine, "CREATE TABLE genres (
        id   INT NOT NULL,
        name TEXT NOT NULL
    )");
    ok(engine, "CREATE TABLE book_genres (
        book_id  INT NOT NULL,
        genre_id INT NOT NULL
    )");
    ok(engine, "INSERT INTO genres VALUES (1, 'Fantasy'), (2, 'Science Fiction'), (3, 'Western')");
    ok(engine, "INSERT INTO book_genres VALUES (1, 1), (2, 1), (3, 2), (4, 2), (5, 2), (6, 3), (7, 2)");
}

// ---------------------------------------------------------------------------
// Main tutorial validation test
// ---------------------------------------------------------------------------

#[test]
fn test_tutorial_all_sql_examples() {
    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());

    setup_bookstore(&engine);

    // -----------------------------------------------------------------------
    // ch03: Basic SELECT
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT * FROM books");
    ok(&engine, "SELECT title, price FROM books WHERE price < 14.00");
    ok(&engine, "SELECT title, published FROM books WHERE published < 1970");
    ok(&engine, "SELECT title, price FROM books WHERE price > 13.00 AND published > 1960");

    // -----------------------------------------------------------------------
    // ch03: ORDER BY
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT title, price FROM books ORDER BY price ASC");
    ok(&engine, "SELECT title, published, price FROM books ORDER BY published DESC, price DESC");

    // -----------------------------------------------------------------------
    // ch03: LIMIT / OFFSET
    // -----------------------------------------------------------------------
    rows_count(&engine, "SELECT title, price FROM books ORDER BY price DESC LIMIT 3", 3);
    rows_count(&engine, "SELECT title, price FROM books ORDER BY price DESC LIMIT 2 OFFSET 3", 2);

    // -----------------------------------------------------------------------
    // ch03: JOIN (INNER)
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT b.title, a.name AS author, b.price
        FROM books b
        JOIN authors a ON b.author_id = a.id
        ORDER BY a.name, b.title");

    ok(&engine, "SELECT o.id AS order_id, b.title, o.quantity, o.total_price
        FROM orders o
        JOIN books b ON o.book_id = b.id
        ORDER BY o.id");

    // -----------------------------------------------------------------------
    // ch03: LEFT JOIN
    // -----------------------------------------------------------------------
    ok(&engine, "INSERT INTO authors VALUES (5, 'Gene Wolfe', 'United States')");

    ok(&engine, "SELECT a.name, b.title
        FROM authors a
        LEFT JOIN books b ON b.author_id = a.id
        ORDER BY a.name, b.title");

    // -----------------------------------------------------------------------
    // ch03: JOIN USING
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT bg.book_id, g.name AS genre
        FROM book_genres bg
        JOIN genres g USING (genre_id)
        ORDER BY bg.book_id");

    // -----------------------------------------------------------------------
    // ch03: IS NULL / IS NOT NULL
    // -----------------------------------------------------------------------
    // No nulls yet — should return 0 rows
    rows_count(&engine, "SELECT name FROM authors WHERE country IS NULL", 0);

    ok(&engine, "INSERT INTO authors VALUES (6, 'Anonymous', NULL)");
    rows_count(&engine, "SELECT name FROM authors WHERE country IS NULL", 1);

    // -----------------------------------------------------------------------
    // ch03: ILIKE
    // -----------------------------------------------------------------------
    rows_count(&engine, "SELECT name FROM authors WHERE name ILIKE '%ursula%'", 1);

    // -----------------------------------------------------------------------
    // ch03: DISTINCT
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT DISTINCT country FROM authors WHERE country IS NOT NULL ORDER BY country");
    ok(&engine, "SELECT DISTINCT country, name FROM authors ORDER BY country, name");

    // -----------------------------------------------------------------------
    // ch03: RETURNING (INSERT, UPDATE, DELETE)
    // -----------------------------------------------------------------------
    rows_count(
        &engine,
        "INSERT INTO orders VALUES (6, 2, 3, 74.97) RETURNING id, total_price",
        1,
    );
    rows_count(
        &engine,
        "UPDATE books SET price = price * 0.90 WHERE author_id = 4 RETURNING title, price",
        1,
    );
    rows_count(
        &engine,
        "DELETE FROM authors WHERE id = 6 RETURNING name",
        1,
    );

    // -----------------------------------------------------------------------
    // ch03: UNION
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT name AS label, 'author' AS kind FROM authors
        UNION
        SELECT title, 'book' FROM books
        ORDER BY kind, label");

    // -----------------------------------------------------------------------
    // ch03: CTE (chained WITH)
    // -----------------------------------------------------------------------
    ok(&engine, "WITH book_revenue AS (
        SELECT b.author_id, b.title, SUM(o.total_price) AS revenue
        FROM books b
        JOIN orders o ON o.book_id = b.id
        GROUP BY b.author_id, b.title
    ),
    author_revenue AS (
        SELECT a.name, SUM(br.revenue) AS total_revenue
        FROM book_revenue br
        JOIN authors a ON a.id = br.author_id
        GROUP BY a.name
    )
    SELECT name, total_revenue
    FROM author_revenue
    WHERE total_revenue > 50.00
    ORDER BY total_revenue DESC");

    // -----------------------------------------------------------------------
    // ch03: Aggregates
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT a.name, COUNT(*) AS book_count
        FROM books b
        JOIN authors a ON b.author_id = a.id
        GROUP BY a.name
        ORDER BY book_count DESC");

    ok(&engine, "SELECT
        AVG(price) AS avg_price,
        MIN(price) AS min_price,
        MAX(price) AS max_price,
        SUM(price) AS total_catalog_value
    FROM books");

    ok(&engine, "SELECT b.title, SUM(o.quantity) AS units_sold, SUM(o.total_price) AS revenue
        FROM orders o
        JOIN books b ON o.book_id = b.id
        GROUP BY b.title
        ORDER BY revenue DESC");

    // -----------------------------------------------------------------------
    // ch03: UPDATE
    // -----------------------------------------------------------------------
    ok(&engine, "UPDATE books SET price = price * 1.10 WHERE author_id = 2");
    ok(&engine, "SELECT title, price FROM books WHERE author_id = 2");
    ok(&engine, "UPDATE authors SET name = 'Ursula K. Le Guin' WHERE id = 3");

    // -----------------------------------------------------------------------
    // ch03: DELETE
    // -----------------------------------------------------------------------
    ok(&engine, "DELETE FROM authors WHERE id = 5");
    ok(&engine, "DELETE FROM orders WHERE book_id = 1");

    // -----------------------------------------------------------------------
    // ch03: Advanced — CASE WHEN
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT title, price,
        CASE WHEN price < 13.00 THEN 'budget'
             WHEN price < 20.00 THEN 'mid-range'
             ELSE 'premium'
        END AS tier
    FROM books
    ORDER BY price");

    // -----------------------------------------------------------------------
    // ch03: Advanced — COALESCE
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT name, COALESCE(country, 'Unknown') AS country FROM authors ORDER BY name");

    // -----------------------------------------------------------------------
    // ch03: Advanced — ALTER TABLE ADD COLUMN, RENAME COLUMN, DROP COLUMN, RENAME TABLE
    // -----------------------------------------------------------------------
    ok(&engine, "ALTER TABLE authors ADD COLUMN bio TEXT");
    ok(&engine, "UPDATE authors SET bio = 'Author of Middle-earth.' WHERE id = 1");
    ok(&engine, "SELECT id, name, bio FROM authors ORDER BY id");
    ok(&engine, "ALTER TABLE authors RENAME COLUMN bio TO biography");
    ok(&engine, "ALTER TABLE authors RENAME COLUMN biography TO bio");
    ok(&engine, "ALTER TABLE authors DROP COLUMN bio");
    ok(&engine, "ALTER TABLE authors RENAME TO writers");
    ok(&engine, "ALTER TABLE writers RENAME TO authors"); // rename back

    // -----------------------------------------------------------------------
    // ch03: Advanced — PRIMARY KEY and UNIQUE constraints
    // -----------------------------------------------------------------------
    ok(&engine, "CREATE TABLE members (
        id      INTEGER PRIMARY KEY,
        email   TEXT    UNIQUE NOT NULL,
        handle  TEXT    NOT NULL
    )");
    ok(&engine, "INSERT INTO members VALUES (1, 'alice@example.com', 'alice')");
    ok(&engine, "INSERT INTO members VALUES (2, 'bob@example.com', 'bob')");

    // duplicate email — must fail
    err(&engine, "INSERT INTO members VALUES (3, 'alice@example.com', 'alice2')");

    // duplicate primary key — must fail
    err(&engine, "INSERT INTO members VALUES (1, 'carol@example.com', 'carol')");

    // -----------------------------------------------------------------------
    // ch03: Advanced — Window functions
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT a.name AS author, b.title, b.price,
        ROW_NUMBER() OVER (PARTITION BY b.author_id ORDER BY b.price DESC) AS price_rank
    FROM books b
    JOIN authors a ON a.id = b.author_id
    ORDER BY a.name, price_rank");

    ok(&engine, "SELECT a.name AS author, b.title, b.price,
        SUM(b.price) OVER (PARTITION BY b.author_id) AS author_total
    FROM books b
    JOIN authors a ON a.id = b.author_id
    ORDER BY a.name, b.price DESC");

    // -----------------------------------------------------------------------
    // ch03: Advanced — WITH RECURSIVE
    // -----------------------------------------------------------------------
    rows_count(
        &engine,
        "WITH RECURSIVE series(n) AS (
            SELECT 1
            UNION ALL
            SELECT n + 1 FROM series WHERE n < 5
        )
        SELECT n FROM series",
        5,
    );

    // -----------------------------------------------------------------------
    // ch04: Data type tables
    // -----------------------------------------------------------------------
    ok(&engine, "CREATE TABLE flags (enabled BOOLEAN)");
    ok(&engine, "INSERT INTO flags VALUES (true)");
    ok(&engine, "INSERT INTO flags VALUES (false)");
    ok(&engine, "SELECT * FROM flags WHERE enabled = true");

    ok(&engine, "CREATE TABLE counters (n INT NOT NULL)");
    ok(&engine, "INSERT INTO counters VALUES (0), (100), (-50)");

    ok(&engine, "CREATE TABLE large_ids (id BIGINT NOT NULL)");
    ok(&engine, "INSERT INTO large_ids VALUES (9000000000)");

    ok(&engine, "CREATE TABLE measurements (value FLOAT)");
    ok(&engine, "INSERT INTO measurements VALUES (3.14159265358979)");

    ok(&engine, "CREATE TABLE notes (body TEXT)");
    ok(&engine, "INSERT INTO notes VALUES ('Hello, world!')");
    ok(&engine, "INSERT INTO notes VALUES ('Multi-line strings work too')");

    ok(&engine, "CREATE TABLE codes (code VARCHAR(10) NOT NULL)");
    ok(&engine, "INSERT INTO codes VALUES ('ABC123')");

    // -----------------------------------------------------------------------
    // ch04: DDL — CREATE TABLE with PRIMARY KEY and UNIQUE
    // -----------------------------------------------------------------------
    ok(&engine, "CREATE TABLE products (
        id       INT NOT NULL,
        name     TEXT NOT NULL,
        price    FLOAT,
        in_stock BOOLEAN
    )");

    ok(&engine, "CREATE TABLE users (
        id        INTEGER PRIMARY KEY,
        email     TEXT    UNIQUE NOT NULL,
        username  TEXT    NOT NULL,
        bio       TEXT
    )");

    // CREATE TABLE IF NOT EXISTS
    ok(&engine, "CREATE TABLE IF NOT EXISTS products (
        id   INT NOT NULL,
        name TEXT NOT NULL
    )");

    // -----------------------------------------------------------------------
    // ch04: DDL — DROP TABLE IF EXISTS
    // -----------------------------------------------------------------------
    ok(&engine, "CREATE TABLE temp_drop (id INT)");
    ok(&engine, "DROP TABLE temp_drop");
    ok(&engine, "DROP TABLE IF EXISTS temp_drop");

    // -----------------------------------------------------------------------
    // ch04: DML — INSERT variants
    // -----------------------------------------------------------------------
    ok(&engine, "INSERT INTO products VALUES (1, 'Widget', 9.99, true)");
    ok(&engine, "INSERT INTO products VALUES (2, 'Gadget', NULL, NULL)");
    ok(&engine, "INSERT INTO products (id, name) VALUES (3, 'Doohickey')");
    ok(&engine, "INSERT INTO products VALUES
        (4, 'Alpha', 1.00, true),
        (5, 'Beta',  2.00, false),
        (6, 'Gamma', 3.00, true)");

    // RETURNING on INSERT
    rows_count(
        &engine,
        "INSERT INTO products (id, name, price, in_stock) VALUES (7, 'Sprocket', 4.50, true) RETURNING id, name",
        1,
    );
    rows_count(
        &engine,
        "INSERT INTO products VALUES (8, 'Cog', 2.25, false) RETURNING id, name, price * 1.2 AS price_with_tax",
        1,
    );

    // -----------------------------------------------------------------------
    // ch04: DML — UPDATE variants
    // -----------------------------------------------------------------------
    ok(&engine, "UPDATE products SET price = price * 1.05 WHERE in_stock = true");
    ok(&engine, "UPDATE products SET in_stock = false WHERE price > 100.0");
    ok(&engine, "UPDATE products SET name = 'Widget Pro', price = 19.99 WHERE id = 1");

    // RETURNING on UPDATE
    ok(&engine, "UPDATE products SET price = price * 1.10 WHERE in_stock = true RETURNING id, name, price");

    // -----------------------------------------------------------------------
    // ch04: DML — DELETE variants
    // -----------------------------------------------------------------------
    ok(&engine, "DELETE FROM products WHERE in_stock = false");
    ok(&engine, "DELETE FROM products WHERE in_stock = false RETURNING id, name");

    // -----------------------------------------------------------------------
    // ch04: SELECT — basic, DISTINCT, expressions
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT * FROM products");
    ok(&engine, "SELECT id, name, price * 1.2 AS price_with_tax FROM products");
    ok(&engine, "SELECT p.name, p.price FROM products p WHERE p.price < 10.0");

    // -----------------------------------------------------------------------
    // ch04: DISTINCT
    // -----------------------------------------------------------------------
    // products table may not have a 'category' column; use in_stock instead
    ok(&engine, "SELECT DISTINCT in_stock FROM products ORDER BY in_stock");

    // -----------------------------------------------------------------------
    // ch04: CTE (WITH)
    // -----------------------------------------------------------------------
    ok(&engine, "WITH expensive AS (
        SELECT id, name, price FROM products WHERE price > 5.0
    ),
    summary AS (
        SELECT COUNT(*) AS cnt, AVG(price) AS avg_price FROM expensive
    )
    SELECT * FROM summary");

    // -----------------------------------------------------------------------
    // ch04: WITH RECURSIVE (countdown from 10)
    // -----------------------------------------------------------------------
    rows_count(
        &engine,
        "WITH RECURSIVE countdown(n) AS (
            SELECT 10
            UNION ALL
            SELECT n - 1 FROM countdown WHERE n > 1
        )
        SELECT n FROM countdown ORDER BY n DESC",
        10,
    );

    // -----------------------------------------------------------------------
    // ch04: Set operations — UNION, INTERSECT, EXCEPT
    // -----------------------------------------------------------------------

    // UNION on same-schema tables (using books and authors which both have names)
    ok(&engine, "SELECT name AS label FROM authors
        UNION
        SELECT title AS label FROM books
        ORDER BY label");

    // INTERSECT: names in products that are also in users (likely empty but valid SQL)
    ok(&engine, "SELECT name FROM products WHERE in_stock = true
        INTERSECT
        SELECT name FROM products WHERE price < 20.0");

    // EXCEPT
    ok(&engine, "SELECT id FROM books
        EXCEPT
        SELECT id FROM orders");

    // UNION ALL
    ok(&engine, "SELECT id FROM books
        UNION ALL
        SELECT id FROM orders");

    // -----------------------------------------------------------------------
    // ch04: Multi-table FROM (implicit cross join)
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT o.id, b.title
        FROM orders o, books b
        WHERE o.book_id = b.id");

    // -----------------------------------------------------------------------
    // ch04: JOIN USING
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT book_id, g.name AS genre
        FROM book_genres
        JOIN genres g USING (genre_id)");

    // -----------------------------------------------------------------------
    // ch04: Qualified column references
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT b.title, a.name AS author
        FROM books b
        JOIN authors a ON b.author_id = a.id
        WHERE b.price > 15.0");

    // -----------------------------------------------------------------------
    // ch04: WHERE — comparisons
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT * FROM products WHERE price = 9.99");
    ok(&engine, "SELECT * FROM products WHERE name <> 'Widget Pro'");
    ok(&engine, "SELECT * FROM products WHERE price >= 5.00 AND price <= 20.00");
    ok(&engine, "SELECT * FROM products WHERE (price < 5.0 OR price > 50.0) AND in_stock = true");

    // -----------------------------------------------------------------------
    // ch04: NULL checks
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT * FROM products WHERE price IS NULL");
    ok(&engine, "SELECT * FROM products WHERE price IS NOT NULL");

    // -----------------------------------------------------------------------
    // ch04: IS DISTINCT FROM / IS NOT DISTINCT FROM
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT * FROM products WHERE price IS DISTINCT FROM 9.99");
    ok(&engine, "SELECT * FROM products WHERE price IS NOT DISTINCT FROM NULL");

    // -----------------------------------------------------------------------
    // ch04: ILIKE
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT * FROM products WHERE name ILIKE 'widget%'");
    ok(&engine, "SELECT * FROM products WHERE name ILIKE '%pro%'");

    // -----------------------------------------------------------------------
    // ch04: IN (subquery) and EXISTS / NOT EXISTS
    // -----------------------------------------------------------------------

    // We need a table with a product_id column for the ch04 subquery examples.
    // Use orders (book_id serves as product id) and books.
    ok(&engine, "SELECT title FROM books
        WHERE id IN (SELECT book_id FROM orders WHERE quantity > 1)");

    ok(&engine, "SELECT title FROM books
        WHERE price > (SELECT AVG(price) FROM books)");

    // EXISTS — authors with at least one book
    ok(&engine, "SELECT a.name
        FROM authors a
        WHERE EXISTS (
            SELECT 1 FROM books b WHERE b.author_id = a.id
        )");

    // NOT EXISTS — authors with no books
    ok(&engine, "SELECT a.name
        FROM authors a
        WHERE NOT EXISTS (
            SELECT 1 FROM books b WHERE b.author_id = a.id
        )");

    // -----------------------------------------------------------------------
    // ch04: HAVING
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT a.name, COUNT(*) AS cnt, AVG(b.price) AS avg_price
        FROM books b
        JOIN authors a ON b.author_id = a.id
        GROUP BY a.name
        HAVING COUNT(*) > 1 AND AVG(b.price) < 50.0
        ORDER BY cnt DESC");

    // -----------------------------------------------------------------------
    // ch04: Conditional expressions — CASE WHEN (searched and simple)
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT title,
        CASE WHEN price < 10 THEN 'budget'
             WHEN price < 30 THEN 'mid-range'
             ELSE 'premium'
        END AS tier
    FROM books");

    ok(&engine, "SELECT name,
        CASE country
            WHEN 'United States' THEN 'US'
            WHEN 'United Kingdom' THEN 'UK'
            ELSE 'Other'
        END AS region
    FROM authors");

    // -----------------------------------------------------------------------
    // ch04: COALESCE
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT name, COALESCE(country, 'Unknown') AS country_display FROM authors");

    // -----------------------------------------------------------------------
    // ch04: NULLIF (division-by-zero guard)
    // -----------------------------------------------------------------------
    // Create a summary table to test NULLIF
    ok(&engine, "CREATE TABLE summary (total_sales FLOAT, num_orders INT)");
    ok(&engine, "INSERT INTO summary VALUES (1000.0, 5)");
    ok(&engine, "INSERT INTO summary VALUES (500.0, 0)");
    ok(&engine, "SELECT total_sales / NULLIF(num_orders, 0) AS avg_order_value FROM summary");

    // -----------------------------------------------------------------------
    // ch04: String operator ||
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT name || ' (author)' AS label FROM authors ORDER BY name");

    // -----------------------------------------------------------------------
    // ch04: ORDER BY, LIMIT, OFFSET
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT name, price FROM products ORDER BY price ASC");
    ok(&engine, "SELECT name, price FROM products ORDER BY price DESC");
    ok(&engine, "SELECT name, price FROM products ORDER BY name ASC, price DESC");
    ok(&engine, "SELECT * FROM products ORDER BY id LIMIT 10");
    ok(&engine, "SELECT * FROM products ORDER BY id LIMIT 10 OFFSET 2");

    // -----------------------------------------------------------------------
    // ch04: GROUP BY and aggregates
    // -----------------------------------------------------------------------
    // Use in_stock as the group-by column (replaces 'category' which products doesn't have)
    // ORDER BY uses alias name rather than aggregate expression (PostgreSQL-compatible style)
    ok(&engine, "SELECT in_stock, COUNT(*) AS cnt, AVG(price) AS avg_p
        FROM products
        GROUP BY in_stock
        ORDER BY cnt DESC");

    // COUNT DISTINCT
    ok(&engine, "SELECT COUNT(DISTINCT in_stock) AS unique_in_stock FROM products");

    // -----------------------------------------------------------------------
    // ch04: JOINs — LEFT, RIGHT, FULL, CROSS
    // -----------------------------------------------------------------------

    // LEFT JOIN
    ok(&engine, "SELECT a.name, b.title
        FROM authors a
        LEFT JOIN books b ON b.author_id = a.id
        ORDER BY a.name, b.title");

    // RIGHT JOIN
    ok(&engine, "SELECT a.name, b.title
        FROM books b
        RIGHT JOIN authors a ON b.author_id = a.id
        ORDER BY a.name");

    // FULL JOIN
    ok(&engine, "SELECT a.name AS author, b.title
        FROM authors a
        FULL JOIN books b ON b.author_id = a.id
        ORDER BY a.name");

    // CROSS JOIN (explicit keyword)
    ok(&engine, "CREATE TABLE sizes (sz TEXT)");
    ok(&engine, "CREATE TABLE colors (col TEXT)");
    ok(&engine, "INSERT INTO sizes VALUES ('S'), ('M'), ('L')");
    ok(&engine, "INSERT INTO colors VALUES ('red'), ('blue')");
    rows_count(
        &engine,
        "SELECT s.sz AS size, c.col AS color FROM sizes s CROSS JOIN colors c",
        6,
    );

    // Multi-condition JOIN
    ok(&engine, "CREATE TABLE shipments (id INT, warehouse_id INT, region TEXT)");
    ok(&engine, "CREATE TABLE warehouses (id INT, region TEXT)");
    ok(&engine, "INSERT INTO shipments VALUES (1, 10, 'east')");
    ok(&engine, "INSERT INTO warehouses VALUES (10, 'east')");
    ok(&engine, "SELECT s.id, w.region
        FROM shipments s
        JOIN warehouses w ON s.warehouse_id = w.id AND s.region = w.region");

    // -----------------------------------------------------------------------
    // ch04: Window functions (employees table)
    // -----------------------------------------------------------------------
    ok(&engine, "CREATE TABLE employees (id INT, name TEXT, dept TEXT, salary INT)");
    ok(&engine, "INSERT INTO employees VALUES (1, 'Alice', 'eng', 90000)");
    ok(&engine, "INSERT INTO employees VALUES (2, 'Bob', 'eng', 80000)");
    ok(&engine, "INSERT INTO employees VALUES (3, 'Carol', 'hr', 70000)");
    ok(&engine, "INSERT INTO employees VALUES (4, 'Dave', 'hr', 75000)");

    ok(&engine, "SELECT name, dept, salary,
        ROW_NUMBER() OVER (PARTITION BY dept ORDER BY salary DESC) AS rank_in_dept
    FROM employees");

    ok(&engine, "SELECT name, dept, salary,
        SUM(salary) OVER (PARTITION BY dept) AS dept_total
    FROM employees");

    // -----------------------------------------------------------------------
    // ch04: COUNT(DISTINCT ...) per group
    // -----------------------------------------------------------------------
    ok(&engine, "CREATE TABLE emp_roles (dept TEXT, job_title TEXT)");
    ok(&engine, "INSERT INTO emp_roles VALUES ('eng', 'engineer')");
    ok(&engine, "INSERT INTO emp_roles VALUES ('eng', 'engineer')");
    ok(&engine, "INSERT INTO emp_roles VALUES ('eng', 'manager')");
    ok(&engine, "INSERT INTO emp_roles VALUES ('hr', 'recruiter')");
    ok(&engine, "SELECT dept, COUNT(DISTINCT job_title) AS unique_roles
        FROM emp_roles
        GROUP BY dept");

    // -----------------------------------------------------------------------
    // ch04: ALTER TABLE — ADD COLUMN, DROP COLUMN, RENAME COLUMN, RENAME TABLE
    // -----------------------------------------------------------------------
    ok(&engine, "ALTER TABLE employees ADD COLUMN department TEXT");
    ok(&engine, "ALTER TABLE employees DROP COLUMN department");
    ok(&engine, "ALTER TABLE employees RENAME COLUMN dept TO department");
    ok(&engine, "ALTER TABLE employees RENAME COLUMN department TO dept");
    ok(&engine, "CREATE TABLE old_name (id INT)");
    ok(&engine, "ALTER TABLE old_name RENAME TO new_name");
    ok(&engine, "DROP TABLE new_name");

    // -----------------------------------------------------------------------
    // ch04: Transaction control
    // -----------------------------------------------------------------------
    ok(&engine, "BEGIN");
    ok(&engine, "INSERT INTO notes VALUES ('in transaction')");
    ok(&engine, "COMMIT");

    ok(&engine, "BEGIN");
    ok(&engine, "INSERT INTO notes VALUES ('will be rolled back')");
    ok(&engine, "ROLLBACK");

    ok(&engine, "BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ");
    ok(&engine, "COMMIT");

    ok(&engine, "BEGIN TRANSACTION ISOLATION LEVEL SERIALIZABLE");
    ok(&engine, "COMMIT");

    // -----------------------------------------------------------------------
    // ch04: NULL semantics
    // -----------------------------------------------------------------------

    // NULL in aggregates
    ok(&engine, "SELECT AVG(price) FROM products");
    ok(&engine, "SELECT COUNT(*), COUNT(price) FROM products");

    // NULL-safe equality
    ok(&engine, "SELECT * FROM products WHERE price IS NOT DISTINCT FROM NULL");

    // -----------------------------------------------------------------------
    // ch04: Arithmetic safety — overflow and division by zero
    // -----------------------------------------------------------------------
    err(&engine, "SELECT 2147483647 + 1");
    err(&engine, "SELECT 2000000000 * 2");
    err(&engine, "SELECT 10 / 0");

    // -----------------------------------------------------------------------
    // ch03: NEW — FETCH FIRST (SQL-standard LIMIT)
    // -----------------------------------------------------------------------
    rows_count(&engine, "SELECT title, price FROM books ORDER BY price DESC FETCH FIRST 3 ROWS ONLY", 3);
    rows_count(&engine, "SELECT title, price FROM books ORDER BY price DESC FETCH FIRST 1 ROW ONLY", 1);

    // -----------------------------------------------------------------------
    // ch03: NEW — LATERAL join (most expensive book per author)
    // -----------------------------------------------------------------------
    let r = engine.execute(
        "SELECT a.name AS author, top_book.title, top_book.price
         FROM authors a
         JOIN LATERAL (
             SELECT title, price
             FROM books b
             WHERE b.author_id = a.id
             ORDER BY price DESC
             LIMIT 1
         ) AS top_book ON true
         ORDER BY a.name"
    ).expect("LATERAL join should succeed");
    // Authors 1-4 each have at least one book (author 5 Gene Wolfe was deleted earlier)
    assert!(r.rows.len() >= 3, "LATERAL join should return at least 3 rows, got {}", r.rows.len());

    // LEFT JOIN LATERAL - keeps authors with no books
    ok(&engine, "SELECT a.name, top_book.title
        FROM authors a
        LEFT JOIN LATERAL (
            SELECT title FROM books b WHERE b.author_id = a.id ORDER BY price DESC LIMIT 1
        ) AS top_book ON true
        ORDER BY a.name");

    // -----------------------------------------------------------------------
    // ch03: NEW — String functions (bookstore examples from ch03)
    // -----------------------------------------------------------------------

    // UPPER / LOWER on real table data
    ok(&engine, "SELECT UPPER(name) AS shout, LOWER(name) AS whisper FROM authors LIMIT 2");

    // LENGTH + TRIM combined, ORDER BY expression (not alias)
    ok(&engine, "SELECT name, LENGTH(TRIM(name)) AS chars FROM authors ORDER BY LENGTH(TRIM(name)) DESC");

    // SUBSTRING + POSITION combined — extract first word of each title
    let r2 = engine.execute(
        "SELECT title, SUBSTRING(title, 1, POSITION(' ' IN title) - 1) AS first_word
         FROM books
         WHERE POSITION(' ' IN title) > 0"
    ).expect("SUBSTRING + POSITION should succeed");
    assert!(!r2.rows.is_empty(), "SUBSTRING+POSITION query should return rows");

    // REPLACE on author names
    ok(&engine, "SELECT REPLACE(name, '.', '') AS simplified_name FROM authors");

    // || concatenation with nullable column
    ok(&engine, "SELECT name || ' (' || country || ')' AS label FROM authors WHERE country IS NOT NULL");

    // -----------------------------------------------------------------------
    // ch04: NEW — Modulo operator
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT id % 2 AS is_odd FROM products");
    ok(&engine, "SELECT id, id % 4 AS bucket FROM products ORDER BY id % 4, id");

    // -----------------------------------------------------------------------
    // ch04: NEW — String functions
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT UPPER('hello')");
    ok(&engine, "SELECT LOWER('WORLD')");
    ok(&engine, "SELECT LENGTH('abc')");
    ok(&engine, "SELECT TRIM('  hi  ')");
    ok(&engine, "SELECT LTRIM('  hi')");
    ok(&engine, "SELECT RTRIM('hi  ')");
    ok(&engine, "SELECT SUBSTRING('hello', 2, 3)");
    ok(&engine, "SELECT POSITION('ll' IN 'hello')");
    ok(&engine, "SELECT STRPOS('hello', 'll')");
    ok(&engine, "SELECT REPLACE('foo bar', 'bar', 'baz')");
    ok(&engine, "SELECT TRIM(LOWER(name)) AS clean_name FROM authors");
    ok(&engine, "SELECT SPLIT_PART('a,b,c', ',', 2)");
    ok(&engine, "SELECT LPAD('5', 3, '0')");
    ok(&engine, "SELECT RPAD('hi', 5, '.')");
    ok(&engine, "SELECT LEFT('hello', 3)");
    ok(&engine, "SELECT RIGHT('hello', 3)");
    ok(&engine, "SELECT REPEAT('ab', 3)");
    ok(&engine, "SELECT REVERSE('abc')");

    // -----------------------------------------------------------------------
    // ch04: NEW — Type casts (text → numeric / boolean)
    // -----------------------------------------------------------------------
    ok(&engine, "SELECT 'Infinity'::FLOAT8");
    ok(&engine, "SELECT '-Infinity'::FLOAT8");
    ok(&engine, "SELECT 'NaN'::FLOAT8");
    ok(&engine, "SELECT '42'::INT");
    ok(&engine, "SELECT '9000000000'::BIGINT");
    ok(&engine, "SELECT 'true'::BOOLEAN");
    ok(&engine, "SELECT 'yes'::BOOLEAN");
    ok(&engine, "SELECT 'on'::BOOLEAN");
    ok(&engine, "SELECT '1'::BOOLEAN");
    ok(&engine, "SELECT 'false'::BOOLEAN");
    ok(&engine, "SELECT 'no'::BOOLEAN");

    // -----------------------------------------------------------------------
    // ch04: NEW — IS UNKNOWN / IS NOT UNKNOWN
    // -----------------------------------------------------------------------
    ok(&engine, "CREATE TABLE maybe_flags (enabled BOOLEAN)");
    ok(&engine, "INSERT INTO maybe_flags VALUES (true), (false), (NULL)");
    rows_count(&engine, "SELECT * FROM maybe_flags WHERE enabled IS UNKNOWN", 1);
    rows_count(&engine, "SELECT * FROM maybe_flags WHERE enabled IS NOT UNKNOWN", 2);

    // -----------------------------------------------------------------------
    // ch04: NEW — FETCH FIRST N ROWS ONLY
    // -----------------------------------------------------------------------
    rows_count(&engine, "SELECT * FROM books ORDER BY id FETCH FIRST 3 ROWS ONLY", 3);
    rows_count(&engine, "SELECT * FROM books ORDER BY id FETCH FIRST 1 ROW ONLY", 1);

    // -----------------------------------------------------------------------
    // ch04: NEW — LATERAL join
    // -----------------------------------------------------------------------
    // For each author, get their most expensive book using LATERAL
    ok(&engine, "SELECT a.name, recent.title, recent.price
        FROM authors a
        JOIN LATERAL (
            SELECT title, price
            FROM books b
            WHERE b.author_id = a.id
            ORDER BY price DESC
            LIMIT 1
        ) AS recent ON true
        ORDER BY a.name");

    // -----------------------------------------------------------------------
    // ch03: NEW — SAVEPOINT (Production Patterns section)
    // The ch03 example uses an 'accounts' table; create it for this test.
    // -----------------------------------------------------------------------
    ok(&engine, "CREATE TABLE accounts (name TEXT NOT NULL, balance FLOAT NOT NULL)");
    ok(&engine, "INSERT INTO accounts VALUES ('Alice', 1000), ('Bob', 500)");

    let sid_sp = "tut_savepoint_accounts";
    exec_session(&engine, sid_sp, "BEGIN");
    exec_session(&engine, sid_sp, "UPDATE accounts SET balance = balance - 100 WHERE name = 'Alice'");
    exec_session(&engine, sid_sp, "SAVEPOINT after_debit");
    exec_session(&engine, sid_sp, "UPDATE accounts SET balance = balance + 100 WHERE name = 'Bob'");
    // ROLLBACK TO aborts entire txn in current implementation and starts fresh
    exec_session(&engine, sid_sp, "ROLLBACK TO SAVEPOINT after_debit");
    exec_session(&engine, sid_sp, "UPDATE accounts SET balance = balance + 100 WHERE name = 'Bob'");
    exec_session(&engine, sid_sp, "COMMIT");

    // RELEASE SAVEPOINT pattern
    let sid_rel = "tut_savepoint_release";
    exec_session(&engine, sid_rel, "BEGIN");
    exec_session(&engine, sid_rel, "SAVEPOINT sp1");
    exec_session(&engine, sid_rel, "RELEASE SAVEPOINT sp1");
    exec_session(&engine, sid_rel, "COMMIT");

    // -----------------------------------------------------------------------
    // ch04: NEW — SAVEPOINT (accepted; full partial-rollback not guaranteed)
    // -----------------------------------------------------------------------
    let sid = "tut_savepoint";
    exec_session(&engine, sid, "BEGIN");
    exec_session(&engine, sid, "INSERT INTO notes VALUES ('savepoint test')");
    exec_session(&engine, sid, "SAVEPOINT sp1");
    exec_session(&engine, sid, "RELEASE SAVEPOINT sp1");
    exec_session(&engine, sid, "COMMIT");

    // -----------------------------------------------------------------------
    // ch04: NEW — SET TRANSACTION (accepted as no-op)
    // -----------------------------------------------------------------------
    let sid2 = "tut_set_txn";
    exec_session(&engine, sid2, "BEGIN");
    exec_session(&engine, sid2, "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ");
    exec_session(&engine, sid2, "COMMIT");

    // -----------------------------------------------------------------------
    // ch04: CREATE DATABASE / DROP DATABASE / CREATE SCHEMA
    // -----------------------------------------------------------------------
    ok(&engine, "CREATE DATABASE testdb_tut");
    ok(&engine, "DROP DATABASE testdb_tut");
    ok(&engine, "DROP DATABASE IF EXISTS testdb_tut");       // idempotent (already dropped)
    ok(&engine, "CREATE DATABASE IF NOT EXISTS testdb_tut2");
    ok(&engine, "CREATE DATABASE IF NOT EXISTS testdb_tut2"); // idempotent
    ok(&engine, "DROP DATABASE IF EXISTS testdb_tut2");
    ok(&engine, "CREATE SCHEMA reporting");
    ok(&engine, "CREATE SCHEMA IF NOT EXISTS reporting");    // idempotent
}
