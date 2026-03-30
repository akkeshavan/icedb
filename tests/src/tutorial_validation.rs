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
use crate::common::{make_engine, exec_session, Backend};

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

fn ok(b: &crate::common::Backend, sql: &str) {
    b.execute(sql);
}

fn err(b: &crate::common::Backend, sql: &str) {
    b.execute_err(sql);
}

fn rows_count(b: &crate::common::Backend, sql: &str, expected: usize) {
    let r = b.execute(sql);
    assert_eq!(r.rows.len(), expected, "Wrong row count for: {sql}");
}

// ---------------------------------------------------------------------------
// Shared bookstore schema setup
// ---------------------------------------------------------------------------

fn setup_bookstore(b: &crate::common::Backend) {
    // --- ch03: DDL ---
    ok(b, "CREATE TABLE authors (
        id       INT NOT NULL,
        name     TEXT NOT NULL,
        country  TEXT
    )");
    ok(b, "CREATE TABLE books (
        id          INT NOT NULL,
        title       TEXT NOT NULL,
        author_id   INT NOT NULL,
        price       FLOAT,
        published   INT
    )");
    ok(b, "CREATE TABLE orders (
        id          INT NOT NULL,
        book_id     INT NOT NULL,
        quantity    INT NOT NULL,
        total_price FLOAT
    )");

    // --- ch03: Author inserts ---
    ok(b, "INSERT INTO authors VALUES (1, 'J.R.R. Tolkien', 'United Kingdom')");
    ok(b, "INSERT INTO authors VALUES (2, 'Frank Herbert', 'United States')");
    ok(b, "INSERT INTO authors VALUES (3, 'Ursula K. Le Guin', 'United States')");
    ok(b, "INSERT INTO authors VALUES (4, 'Cormac McCarthy', 'United States')");

    // --- ch03: Book inserts ---
    ok(b, "INSERT INTO books VALUES (1, 'The Hobbit', 1, 12.99, 1937)");
    ok(b, "INSERT INTO books VALUES (2, 'The Lord of the Rings', 1, 24.99, 1954)");
    ok(b, "INSERT INTO books VALUES (3, 'Dune', 2, 15.99, 1965)");
    ok(b, "INSERT INTO books VALUES (4, 'The Left Hand of Darkness', 3, 13.99, 1969)");
    ok(b, "INSERT INTO books VALUES (5, 'The Dispossessed', 3, 11.99, 1974)");
    ok(b, "INSERT INTO books VALUES (6, 'Blood Meridian', 4, 14.99, 1985)");
    ok(b, "INSERT INTO books VALUES (7, 'Children of Dune', 2, 13.99, 1976)");

    // --- ch03: Order inserts ---
    ok(b, "INSERT INTO orders VALUES (1, 1, 2, 25.98)");
    ok(b, "INSERT INTO orders VALUES (2, 3, 1, 15.99)");
    ok(b, "INSERT INTO orders VALUES (3, 2, 1, 24.99)");
    ok(b, "INSERT INTO orders VALUES (4, 4, 3, 41.97)");
    ok(b, "INSERT INTO orders VALUES (5, 3, 5, 79.95)");

    // --- ch03: Genre tables (needed for JOIN USING) ---
    ok(b, "CREATE TABLE genres (
        genre_id INT NOT NULL,
        name     TEXT NOT NULL
    )");
    ok(b, "CREATE TABLE book_genres (
        book_id  INT NOT NULL,
        genre_id INT NOT NULL
    )");
    ok(b, "INSERT INTO genres VALUES (1, 'Fantasy'), (2, 'Science Fiction'), (3, 'Western')");
    ok(b, "INSERT INTO book_genres VALUES (1, 1), (2, 1), (3, 2), (4, 2), (5, 2), (6, 3), (7, 2)");
}

// ---------------------------------------------------------------------------
// Main tutorial validation test
// ---------------------------------------------------------------------------

fn test_tutorial_all_sql_examples_body(b: &crate::common::Backend) {
    setup_bookstore(b);

    // -----------------------------------------------------------------------
    // ch03: Basic SELECT
    // -----------------------------------------------------------------------
    ok(b, "SELECT * FROM books");
    ok(b, "SELECT title, price FROM books WHERE price < 14.00");
    ok(b, "SELECT title, published FROM books WHERE published < 1970");
    ok(b, "SELECT title, price FROM books WHERE price > 13.00 AND published > 1960");

    // -----------------------------------------------------------------------
    // ch03: ORDER BY
    // -----------------------------------------------------------------------
    ok(b, "SELECT title, price FROM books ORDER BY price ASC");
    ok(b, "SELECT title, published, price FROM books ORDER BY published DESC, price DESC");

    // -----------------------------------------------------------------------
    // ch03: LIMIT / OFFSET
    // -----------------------------------------------------------------------
    rows_count(b, "SELECT title, price FROM books ORDER BY price DESC LIMIT 3", 3);
    rows_count(b, "SELECT title, price FROM books ORDER BY price DESC LIMIT 2 OFFSET 3", 2);

    // -----------------------------------------------------------------------
    // ch03: JOIN (INNER)
    // -----------------------------------------------------------------------
    ok(b, "SELECT b.title, a.name AS author, b.price
        FROM books b
        JOIN authors a ON b.author_id = a.id
        ORDER BY a.name, b.title");

    ok(b, "SELECT o.id AS order_id, b.title, o.quantity, o.total_price
        FROM orders o
        JOIN books b ON o.book_id = b.id
        ORDER BY o.id");

    // -----------------------------------------------------------------------
    // ch03: LEFT JOIN
    // -----------------------------------------------------------------------
    ok(b, "INSERT INTO authors VALUES (5, 'Gene Wolfe', 'United States')");

    ok(b, "SELECT a.name, b.title
        FROM authors a
        LEFT JOIN books b ON b.author_id = a.id
        ORDER BY a.name, b.title");

    // -----------------------------------------------------------------------
    // ch03: JOIN USING
    // -----------------------------------------------------------------------
    rows_count(b, "SELECT bg.book_id, g.name AS genre
        FROM book_genres bg
        JOIN genres g USING (genre_id)
        ORDER BY bg.book_id", 7);

    // -----------------------------------------------------------------------
    // ch03: IS NULL / IS NOT NULL
    // -----------------------------------------------------------------------
    // No nulls yet — should return 0 rows
    rows_count(b, "SELECT name FROM authors WHERE country IS NULL", 0);

    ok(b, "INSERT INTO authors VALUES (6, 'Anonymous', NULL)");
    rows_count(b, "SELECT name FROM authors WHERE country IS NULL", 1);

    // -----------------------------------------------------------------------
    // ch03: ILIKE
    // -----------------------------------------------------------------------
    rows_count(b, "SELECT name FROM authors WHERE name ILIKE '%ursula%'", 1);

    // -----------------------------------------------------------------------
    // ch03: DISTINCT
    // -----------------------------------------------------------------------
    ok(b, "SELECT DISTINCT country FROM authors WHERE country IS NOT NULL ORDER BY country");
    ok(b, "SELECT DISTINCT country, name FROM authors ORDER BY country, name");

    // -----------------------------------------------------------------------
    // ch03: RETURNING (INSERT, UPDATE, DELETE)
    // -----------------------------------------------------------------------
    rows_count(
        b,
        "INSERT INTO orders VALUES (6, 2, 3, 74.97) RETURNING id, total_price",
        1,
    );
    rows_count(
        b,
        "UPDATE books SET price = ROUND(price * 0.90, 2) WHERE author_id = 4 RETURNING title, price",
        1,
    );
    rows_count(
        b,
        "DELETE FROM authors WHERE id = 6 RETURNING name",
        1,
    );

    // -----------------------------------------------------------------------
    // ch03: UNION
    // -----------------------------------------------------------------------
    // After DELETE FROM authors WHERE id=6 (Anonymous), 5 authors remain + 7 books = 12 rows
    rows_count(b, "SELECT name AS label, 'author' AS kind FROM authors
        UNION
        SELECT title, 'book' FROM books
        ORDER BY kind, label", 12);

    // -----------------------------------------------------------------------
    // ch03: CTE (chained WITH)
    // -----------------------------------------------------------------------
    // At this point orders 1-6 are present; Tolkien=125.94, Herbert=95.94 both > 50.00
    rows_count(b, "WITH book_revenue AS (
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
    ORDER BY total_revenue DESC", 2);

    // -----------------------------------------------------------------------
    // ch03: Aggregates
    // -----------------------------------------------------------------------
    rows_count(b, "SELECT a.name, COUNT(*) AS book_count
        FROM books b
        JOIN authors a ON b.author_id = a.id
        GROUP BY a.name
        ORDER BY book_count DESC, a.name", 4);

    ok(b, "SELECT
        AVG(price) AS avg_price,
        MIN(price) AS min_price,
        MAX(price) AS max_price,
        SUM(price) AS total_catalog_value
    FROM books");

    ok(b, "SELECT b.title, SUM(o.quantity) AS units_sold, SUM(o.total_price) AS revenue
        FROM orders o
        JOIN books b ON o.book_id = b.id
        GROUP BY b.title
        ORDER BY revenue DESC");

    // -----------------------------------------------------------------------
    // ch03: UPDATE
    // -----------------------------------------------------------------------
    ok(b, "UPDATE books SET price = ROUND(price * 1.10, 2) WHERE author_id = 2");
    ok(b, "SELECT title, price FROM books WHERE author_id = 2");
    ok(b, "UPDATE authors SET name = 'Ursula K. Le Guin' WHERE id = 3");

    // -----------------------------------------------------------------------
    // ch03: DELETE
    // -----------------------------------------------------------------------
    ok(b, "DELETE FROM authors WHERE id = 5");
    ok(b, "DELETE FROM orders WHERE book_id = 1");

    // -----------------------------------------------------------------------
    // ch03: Advanced — CASE WHEN
    // -----------------------------------------------------------------------
    ok(b, "SELECT title, price,
        CASE WHEN price < 13.00 THEN 'budget'
             WHEN price < 20.00 THEN 'mid-range'
             ELSE 'premium'
        END AS tier
    FROM books
    ORDER BY price");

    // -----------------------------------------------------------------------
    // ch03: Advanced — COALESCE
    // -----------------------------------------------------------------------
    ok(b, "INSERT INTO authors VALUES (6, 'Anonymous', NULL)");
    rows_count(b, "SELECT name, COALESCE(country, 'Unknown') AS country FROM authors ORDER BY name", 5);
    ok(b, "DELETE FROM authors WHERE id = 6");

    // -----------------------------------------------------------------------
    // ch03: Advanced — ALTER TABLE ADD COLUMN, RENAME COLUMN, DROP COLUMN, RENAME TABLE
    // -----------------------------------------------------------------------
    ok(b, "ALTER TABLE authors ADD COLUMN bio TEXT");
    ok(b, "UPDATE authors SET bio = 'Author of Middle-earth.' WHERE id = 1");
    ok(b, "SELECT id, name, bio FROM authors ORDER BY id");
    ok(b, "ALTER TABLE authors RENAME COLUMN bio TO biography");
    ok(b, "ALTER TABLE authors RENAME COLUMN biography TO bio");
    ok(b, "ALTER TABLE authors DROP COLUMN bio");
    ok(b, "ALTER TABLE authors RENAME TO writers");
    ok(b, "ALTER TABLE writers RENAME TO authors"); // rename back

    // -----------------------------------------------------------------------
    // ch03: Advanced — PRIMARY KEY and UNIQUE constraints
    // -----------------------------------------------------------------------
    ok(b, "CREATE TABLE members (
        id      INTEGER PRIMARY KEY,
        email   TEXT    UNIQUE NOT NULL,
        handle  TEXT    NOT NULL
    )");
    ok(b, "INSERT INTO members VALUES (1, 'alice@example.com', 'alice')");
    ok(b, "INSERT INTO members VALUES (2, 'bob@example.com', 'bob')");

    // duplicate email — must fail
    err(b, "INSERT INTO members VALUES (3, 'alice@example.com', 'alice2')");

    // duplicate primary key — must fail
    err(b, "INSERT INTO members VALUES (1, 'carol@example.com', 'carol')");

    // -----------------------------------------------------------------------
    // ch03: Advanced — Window functions
    // -----------------------------------------------------------------------
    ok(b, "SELECT a.name AS author, b.title, b.price,
        ROW_NUMBER() OVER (PARTITION BY b.author_id ORDER BY b.price DESC) AS price_rank
    FROM books b
    JOIN authors a ON a.id = b.author_id
    ORDER BY a.name, price_rank");

    ok(b, "SELECT a.name AS author, b.title, b.price,
        SUM(b.price) OVER (PARTITION BY b.author_id) AS author_total
    FROM books b
    JOIN authors a ON a.id = b.author_id
    ORDER BY a.name, b.price DESC");

    // -----------------------------------------------------------------------
    // ch03: Advanced — WITH RECURSIVE
    // -----------------------------------------------------------------------
    rows_count(
        b,
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
    ok(b, "CREATE TABLE flags (enabled BOOLEAN)");
    ok(b, "INSERT INTO flags VALUES (true)");
    ok(b, "INSERT INTO flags VALUES (false)");
    ok(b, "SELECT * FROM flags WHERE enabled = true");

    ok(b, "CREATE TABLE counters (n INT NOT NULL)");
    ok(b, "INSERT INTO counters VALUES (0), (100), (-50)");

    ok(b, "CREATE TABLE large_ids (id BIGINT NOT NULL)");
    ok(b, "INSERT INTO large_ids VALUES (9000000000)");

    ok(b, "CREATE TABLE measurements (value FLOAT)");
    ok(b, "INSERT INTO measurements VALUES (3.14159265358979)");

    ok(b, "CREATE TABLE notes (body TEXT)");
    ok(b, "INSERT INTO notes VALUES ('Hello, world!')");
    ok(b, "INSERT INTO notes VALUES ('Multi-line strings work too')");

    ok(b, "CREATE TABLE codes (code VARCHAR(10) NOT NULL)");
    ok(b, "INSERT INTO codes VALUES ('ABC123')");

    // -----------------------------------------------------------------------
    // ch04: DDL — CREATE TABLE with PRIMARY KEY and UNIQUE
    // -----------------------------------------------------------------------
    ok(b, "CREATE TABLE products (
        id       INT NOT NULL,
        name     TEXT NOT NULL,
        price    FLOAT,
        in_stock BOOLEAN
    )");

    ok(b, "CREATE TABLE users (
        id        INTEGER PRIMARY KEY,
        email     TEXT    UNIQUE NOT NULL,
        username  TEXT    NOT NULL,
        bio       TEXT
    )");

    // CREATE TABLE IF NOT EXISTS
    ok(b, "CREATE TABLE IF NOT EXISTS products (
        id   INT NOT NULL,
        name TEXT NOT NULL
    )");

    // -----------------------------------------------------------------------
    // ch04: DDL — DROP TABLE IF EXISTS
    // -----------------------------------------------------------------------
    ok(b, "CREATE TABLE temp_drop (id INT)");
    ok(b, "DROP TABLE temp_drop");
    ok(b, "DROP TABLE IF EXISTS temp_drop");

    // -----------------------------------------------------------------------
    // ch04: DML — INSERT variants
    // -----------------------------------------------------------------------
    ok(b, "INSERT INTO products VALUES (1, 'Widget', 9.99, true)");
    ok(b, "INSERT INTO products VALUES (2, 'Gadget', NULL, NULL)");
    ok(b, "INSERT INTO products (id, name) VALUES (3, 'Doohickey')");
    ok(b, "INSERT INTO products VALUES
        (4, 'Alpha', 1.00, true),
        (5, 'Beta',  2.00, false),
        (6, 'Gamma', 3.00, true)");

    // RETURNING on INSERT
    rows_count(
        b,
        "INSERT INTO products (id, name, price, in_stock) VALUES (7, 'Sprocket', 4.50, true) RETURNING id, name",
        1,
    );
    rows_count(
        b,
        "INSERT INTO products VALUES (8, 'Cog', 2.25, false) RETURNING id, name, price * 1.2 AS price_with_tax",
        1,
    );

    // -----------------------------------------------------------------------
    // ch04: DML — UPDATE variants
    // -----------------------------------------------------------------------
    ok(b, "UPDATE products SET price = price * 1.05 WHERE in_stock = true");
    ok(b, "UPDATE products SET in_stock = false WHERE price > 100.0");
    ok(b, "UPDATE products SET name = 'Widget Pro', price = 19.99 WHERE id = 1");

    // RETURNING on UPDATE
    ok(b, "UPDATE products SET price = price * 1.10 WHERE in_stock = true RETURNING id, name, price");

    // -----------------------------------------------------------------------
    // ch04: DML — DELETE variants
    // -----------------------------------------------------------------------
    ok(b, "DELETE FROM products WHERE in_stock = false");
    ok(b, "DELETE FROM products WHERE in_stock = false RETURNING id, name");

    // -----------------------------------------------------------------------
    // ch04: SELECT — basic, DISTINCT, expressions
    // -----------------------------------------------------------------------
    ok(b, "SELECT * FROM products");
    ok(b, "SELECT id, name, price * 1.2 AS price_with_tax FROM products");
    ok(b, "SELECT p.name, p.price FROM products p WHERE p.price < 10.0");

    // -----------------------------------------------------------------------
    // ch04: DISTINCT
    // -----------------------------------------------------------------------
    // products table may not have a 'category' column; use in_stock instead
    ok(b, "SELECT DISTINCT in_stock FROM products ORDER BY in_stock");

    // -----------------------------------------------------------------------
    // ch04: CTE (WITH)
    // -----------------------------------------------------------------------
    ok(b, "WITH expensive AS (
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
        b,
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
    ok(b, "SELECT name AS label FROM authors
        UNION
        SELECT title AS label FROM books
        ORDER BY label");

    // INTERSECT: names in products that are also in users (likely empty but valid SQL)
    ok(b, "SELECT name FROM products WHERE in_stock = true
        INTERSECT
        SELECT name FROM products WHERE price < 20.0");

    // EXCEPT
    ok(b, "SELECT id FROM books
        EXCEPT
        SELECT id FROM orders");

    // UNION ALL
    ok(b, "SELECT id FROM books
        UNION ALL
        SELECT id FROM orders");

    // -----------------------------------------------------------------------
    // ch04: Multi-table FROM (implicit cross join)
    // -----------------------------------------------------------------------
    ok(b, "SELECT o.id, b.title
        FROM orders o, books b
        WHERE o.book_id = b.id");

    // -----------------------------------------------------------------------
    // ch04: JOIN USING
    // -----------------------------------------------------------------------
    rows_count(b, "SELECT book_id, g.name AS genre
        FROM book_genres
        JOIN genres g USING (genre_id)", 7);

    // -----------------------------------------------------------------------
    // ch04: Qualified column references
    // -----------------------------------------------------------------------
    ok(b, "SELECT b.title, a.name AS author
        FROM books b
        JOIN authors a ON b.author_id = a.id
        WHERE b.price > 15.0");

    // -----------------------------------------------------------------------
    // ch04: WHERE — comparisons
    // -----------------------------------------------------------------------
    ok(b, "SELECT * FROM products WHERE price = 9.99");
    ok(b, "SELECT * FROM products WHERE name <> 'Widget Pro'");
    ok(b, "SELECT * FROM products WHERE price >= 5.00 AND price <= 20.00");
    ok(b, "SELECT * FROM products WHERE (price < 5.0 OR price > 50.0) AND in_stock = true");

    // -----------------------------------------------------------------------
    // ch04: NULL checks
    // -----------------------------------------------------------------------
    ok(b, "SELECT * FROM products WHERE price IS NULL");
    ok(b, "SELECT * FROM products WHERE price IS NOT NULL");

    // -----------------------------------------------------------------------
    // ch04: IS DISTINCT FROM / IS NOT DISTINCT FROM
    // -----------------------------------------------------------------------
    ok(b, "SELECT * FROM products WHERE price IS DISTINCT FROM 9.99");
    ok(b, "SELECT * FROM products WHERE price IS NOT DISTINCT FROM NULL");

    // -----------------------------------------------------------------------
    // ch04: ILIKE
    // -----------------------------------------------------------------------
    ok(b, "SELECT * FROM products WHERE name ILIKE 'widget%'");
    ok(b, "SELECT * FROM products WHERE name ILIKE '%pro%'");

    // -----------------------------------------------------------------------
    // ch04: IN (subquery) and EXISTS / NOT EXISTS
    // -----------------------------------------------------------------------

    // We need a table with a product_id column for the ch04 subquery examples.
    // Use orders (book_id serves as product id) and books.
    ok(b, "SELECT title FROM books
        WHERE id IN (SELECT book_id FROM orders WHERE quantity > 1)");

    ok(b, "SELECT title FROM books
        WHERE price > (SELECT AVG(price) FROM books)");

    // EXISTS — authors with at least one book
    ok(b, "SELECT a.name
        FROM authors a
        WHERE EXISTS (
            SELECT 1 FROM books b WHERE b.author_id = a.id
        )");

    // NOT EXISTS — authors with no books
    ok(b, "SELECT a.name
        FROM authors a
        WHERE NOT EXISTS (
            SELECT 1 FROM books b WHERE b.author_id = a.id
        )");

    // -----------------------------------------------------------------------
    // ch04: HAVING
    // -----------------------------------------------------------------------
    ok(b, "SELECT a.name, COUNT(*) AS cnt, AVG(b.price) AS avg_price
        FROM books b
        JOIN authors a ON b.author_id = a.id
        GROUP BY a.name
        HAVING COUNT(*) > 1 AND AVG(b.price) < 50.0
        ORDER BY cnt DESC");

    // -----------------------------------------------------------------------
    // ch04: Conditional expressions — CASE WHEN (searched and simple)
    // -----------------------------------------------------------------------
    ok(b, "SELECT title,
        CASE WHEN price < 10 THEN 'budget'
             WHEN price < 30 THEN 'mid-range'
             ELSE 'premium'
        END AS tier
    FROM books");

    ok(b, "SELECT name,
        CASE country
            WHEN 'United States' THEN 'US'
            WHEN 'United Kingdom' THEN 'UK'
            ELSE 'Other'
        END AS region
    FROM authors");

    // -----------------------------------------------------------------------
    // ch04: COALESCE
    // -----------------------------------------------------------------------
    ok(b, "SELECT name, COALESCE(country, 'Unknown') AS country_display FROM authors");

    // -----------------------------------------------------------------------
    // ch04: NULLIF (division-by-zero guard)
    // -----------------------------------------------------------------------
    // Create a summary table to test NULLIF
    ok(b, "CREATE TABLE summary (total_sales FLOAT, num_orders INT)");
    ok(b, "INSERT INTO summary VALUES (1000.0, 5)");
    ok(b, "INSERT INTO summary VALUES (500.0, 0)");
    ok(b, "SELECT total_sales / NULLIF(num_orders, 0) AS avg_order_value FROM summary");

    // -----------------------------------------------------------------------
    // ch04: String operator ||
    // -----------------------------------------------------------------------
    ok(b, "SELECT name || ' (author)' AS label FROM authors ORDER BY name");

    // -----------------------------------------------------------------------
    // ch04: ORDER BY, LIMIT, OFFSET
    // -----------------------------------------------------------------------
    ok(b, "SELECT name, price FROM products ORDER BY price ASC");
    ok(b, "SELECT name, price FROM products ORDER BY price DESC");
    ok(b, "SELECT name, price FROM products ORDER BY name ASC, price DESC");
    ok(b, "SELECT * FROM products ORDER BY id LIMIT 10");
    ok(b, "SELECT * FROM products ORDER BY id LIMIT 10 OFFSET 2");

    // -----------------------------------------------------------------------
    // ch04: GROUP BY and aggregates
    // -----------------------------------------------------------------------
    // Use in_stock as the group-by column (replaces 'category' which products doesn't have)
    // ORDER BY uses alias name rather than aggregate expression (PostgreSQL-compatible style)
    ok(b, "SELECT in_stock, COUNT(*) AS cnt, AVG(price) AS avg_p
        FROM products
        GROUP BY in_stock
        ORDER BY cnt DESC");

    // COUNT DISTINCT
    ok(b, "SELECT COUNT(DISTINCT in_stock) AS unique_in_stock FROM products");

    // -----------------------------------------------------------------------
    // ch04: JOINs — LEFT, RIGHT, FULL, CROSS
    // -----------------------------------------------------------------------

    // LEFT JOIN
    ok(b, "SELECT a.name, b.title
        FROM authors a
        LEFT JOIN books b ON b.author_id = a.id
        ORDER BY a.name, b.title");

    // RIGHT JOIN
    ok(b, "SELECT a.name, b.title
        FROM books b
        RIGHT JOIN authors a ON b.author_id = a.id
        ORDER BY a.name");

    // FULL JOIN
    ok(b, "SELECT a.name AS author, b.title
        FROM authors a
        FULL JOIN books b ON b.author_id = a.id
        ORDER BY a.name");

    // CROSS JOIN (explicit keyword)
    ok(b, "CREATE TABLE sizes (sz TEXT)");
    ok(b, "CREATE TABLE colors (col TEXT)");
    ok(b, "INSERT INTO sizes VALUES ('S'), ('M'), ('L')");
    ok(b, "INSERT INTO colors VALUES ('red'), ('blue')");
    rows_count(
        b,
        "SELECT s.sz AS size, c.col AS color FROM sizes s CROSS JOIN colors c",
        6,
    );

    // Multi-condition JOIN
    ok(b, "CREATE TABLE shipments (id INT, warehouse_id INT, region TEXT)");
    ok(b, "CREATE TABLE warehouses (id INT, region TEXT)");
    ok(b, "INSERT INTO shipments VALUES (1, 10, 'east')");
    ok(b, "INSERT INTO warehouses VALUES (10, 'east')");
    ok(b, "SELECT s.id, w.region
        FROM shipments s
        JOIN warehouses w ON s.warehouse_id = w.id AND s.region = w.region");

    // -----------------------------------------------------------------------
    // ch04: Window functions (employees table)
    // -----------------------------------------------------------------------
    ok(b, "CREATE TABLE employees (id INT, name TEXT, dept TEXT, salary INT)");
    ok(b, "INSERT INTO employees VALUES (1, 'Alice', 'eng', 90000)");
    ok(b, "INSERT INTO employees VALUES (2, 'Bob', 'eng', 80000)");
    ok(b, "INSERT INTO employees VALUES (3, 'Carol', 'hr', 70000)");
    ok(b, "INSERT INTO employees VALUES (4, 'Dave', 'hr', 75000)");

    ok(b, "SELECT name, dept, salary,
        ROW_NUMBER() OVER (PARTITION BY dept ORDER BY salary DESC) AS rank_in_dept
    FROM employees");

    ok(b, "SELECT name, dept, salary,
        SUM(salary) OVER (PARTITION BY dept) AS dept_total
    FROM employees");

    // -----------------------------------------------------------------------
    // ch04: COUNT(DISTINCT ...) per group
    // -----------------------------------------------------------------------
    ok(b, "CREATE TABLE emp_roles (dept TEXT, job_title TEXT)");
    ok(b, "INSERT INTO emp_roles VALUES ('eng', 'engineer')");
    ok(b, "INSERT INTO emp_roles VALUES ('eng', 'engineer')");
    ok(b, "INSERT INTO emp_roles VALUES ('eng', 'manager')");
    ok(b, "INSERT INTO emp_roles VALUES ('hr', 'recruiter')");
    ok(b, "SELECT dept, COUNT(DISTINCT job_title) AS unique_roles
        FROM emp_roles
        GROUP BY dept");

    // -----------------------------------------------------------------------
    // ch04: ALTER TABLE — ADD COLUMN, DROP COLUMN, RENAME COLUMN, RENAME TABLE
    // -----------------------------------------------------------------------
    ok(b, "ALTER TABLE employees ADD COLUMN department TEXT");
    ok(b, "ALTER TABLE employees DROP COLUMN department");
    ok(b, "ALTER TABLE employees RENAME COLUMN dept TO department");
    ok(b, "ALTER TABLE employees RENAME COLUMN department TO dept");
    ok(b, "CREATE TABLE old_name (id INT)");
    ok(b, "ALTER TABLE old_name RENAME TO new_name");
    ok(b, "DROP TABLE new_name");

    // -----------------------------------------------------------------------
    // ch04: Transaction control
    // -----------------------------------------------------------------------
    ok(b, "BEGIN");
    ok(b, "INSERT INTO notes VALUES ('in transaction')");
    ok(b, "COMMIT");

    ok(b, "BEGIN");
    ok(b, "INSERT INTO notes VALUES ('will be rolled back')");
    ok(b, "ROLLBACK");

    ok(b, "BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ");
    ok(b, "COMMIT");

    ok(b, "BEGIN TRANSACTION ISOLATION LEVEL SERIALIZABLE");
    ok(b, "COMMIT");

    // -----------------------------------------------------------------------
    // ch04: NULL semantics
    // -----------------------------------------------------------------------

    // NULL in aggregates
    ok(b, "SELECT AVG(price) FROM products");
    ok(b, "SELECT COUNT(*), COUNT(price) FROM products");

    // NULL-safe equality
    ok(b, "SELECT * FROM products WHERE price IS NOT DISTINCT FROM NULL");

    // -----------------------------------------------------------------------
    // ch04: Arithmetic safety — overflow and division by zero
    // -----------------------------------------------------------------------
    err(b, "SELECT 2147483647 + 1");
    err(b, "SELECT 2000000000 * 2");
    err(b, "SELECT 10 / 0");

    // -----------------------------------------------------------------------
    // ch03: NEW — FETCH FIRST (SQL-standard LIMIT)
    // -----------------------------------------------------------------------
    rows_count(b, "SELECT title, price FROM books ORDER BY price DESC FETCH FIRST 3 ROWS ONLY", 3);
    rows_count(b, "SELECT title, price FROM books ORDER BY price DESC FETCH FIRST 1 ROW ONLY", 1);

    // -----------------------------------------------------------------------
    // ch03: NEW — LATERAL join (most expensive book per author)
    // -----------------------------------------------------------------------
    let r = b.try_execute("SELECT a.name AS author, top_book.title, top_book.price
         FROM authors a
         JOIN LATERAL (
             SELECT title, price
             FROM books b
             WHERE b.author_id = a.id
             ORDER BY price DESC
             LIMIT 1
         ) AS top_book ON true
         ORDER BY a.name").expect("LATERAL join should succeed");
    // Authors 1-4 each have at least one book (author 5 Gene Wolfe was deleted earlier)
    assert!(r.rows.len() >= 3, "LATERAL join should return at least 3 rows, got {}", r.rows.len());

    // LEFT JOIN LATERAL - keeps authors with no books
    ok(b, "SELECT a.name, top_book.title
        FROM authors a
        LEFT JOIN LATERAL (
            SELECT title FROM books b WHERE b.author_id = a.id ORDER BY price DESC LIMIT 1
        ) AS top_book ON true
        ORDER BY a.name");

    // -----------------------------------------------------------------------
    // ch03: NEW — String functions (bookstore examples from ch03)
    // -----------------------------------------------------------------------

    // UPPER / LOWER on real table data
    ok(b, "SELECT UPPER(name) AS shout, LOWER(name) AS whisper FROM authors LIMIT 2");

    // LENGTH + TRIM combined, ORDER BY expression (not alias)
    ok(b, "SELECT name, LENGTH(TRIM(name)) AS chars FROM authors ORDER BY LENGTH(TRIM(name)) DESC");

    // SUBSTRING + POSITION combined — extract first word of each title
    let r2 = b.try_execute("SELECT title, SUBSTRING(title, 1, POSITION(' ' IN title) - 1) AS first_word
         FROM books
         WHERE POSITION(' ' IN title) > 0").expect("SUBSTRING + POSITION should succeed");
    assert!(!r2.rows.is_empty(), "SUBSTRING+POSITION query should return rows");

    // REPLACE on author names
    ok(b, "SELECT REPLACE(name, '.', '') AS simplified_name FROM authors");

    // || concatenation with nullable column
    ok(b, "SELECT name || ' (' || country || ')' AS label FROM authors WHERE country IS NOT NULL");

    // -----------------------------------------------------------------------
    // ch04: NEW — Modulo operator
    // -----------------------------------------------------------------------
    ok(b, "SELECT id % 2 AS is_odd FROM products");
    ok(b, "SELECT id, id % 4 AS bucket FROM products ORDER BY id % 4, id");

    // -----------------------------------------------------------------------
    // ch04: NEW — String functions
    // -----------------------------------------------------------------------
    ok(b, "SELECT UPPER('hello')");
    ok(b, "SELECT LOWER('WORLD')");
    ok(b, "SELECT LENGTH('abc')");
    ok(b, "SELECT TRIM('  hi  ')");
    ok(b, "SELECT LTRIM('  hi')");
    ok(b, "SELECT RTRIM('hi  ')");
    ok(b, "SELECT SUBSTRING('hello', 2, 3)");
    ok(b, "SELECT POSITION('ll' IN 'hello')");
    ok(b, "SELECT STRPOS('hello', 'll')");
    ok(b, "SELECT REPLACE('foo bar', 'bar', 'baz')");
    ok(b, "SELECT TRIM(LOWER(name)) AS clean_name FROM authors");
    ok(b, "SELECT SPLIT_PART('a,b,c', ',', 2)");
    ok(b, "SELECT LPAD('5', 3, '0')");
    ok(b, "SELECT RPAD('hi', 5, '.')");
    ok(b, "SELECT LEFT('hello', 3)");
    ok(b, "SELECT RIGHT('hello', 3)");
    ok(b, "SELECT REPEAT('ab', 3)");
    ok(b, "SELECT REVERSE('abc')");

    // -----------------------------------------------------------------------
    // ch04: NEW — Type casts (text → numeric / boolean)
    // -----------------------------------------------------------------------
    ok(b, "SELECT 'Infinity'::FLOAT8");
    ok(b, "SELECT '-Infinity'::FLOAT8");
    ok(b, "SELECT 'NaN'::FLOAT8");
    ok(b, "SELECT '42'::INT");
    ok(b, "SELECT '9000000000'::BIGINT");
    ok(b, "SELECT 'true'::BOOLEAN");
    ok(b, "SELECT 'yes'::BOOLEAN");
    ok(b, "SELECT 'on'::BOOLEAN");
    ok(b, "SELECT '1'::BOOLEAN");
    ok(b, "SELECT 'false'::BOOLEAN");
    ok(b, "SELECT 'no'::BOOLEAN");

    // -----------------------------------------------------------------------
    // ch04: NEW — IS UNKNOWN / IS NOT UNKNOWN
    // -----------------------------------------------------------------------
    ok(b, "CREATE TABLE maybe_flags (enabled BOOLEAN)");
    ok(b, "INSERT INTO maybe_flags VALUES (true), (false), (NULL)");
    rows_count(b, "SELECT * FROM maybe_flags WHERE enabled IS UNKNOWN", 1);
    rows_count(b, "SELECT * FROM maybe_flags WHERE enabled IS NOT UNKNOWN", 2);

    // -----------------------------------------------------------------------
    // ch04: NEW — FETCH FIRST N ROWS ONLY
    // -----------------------------------------------------------------------
    rows_count(b, "SELECT * FROM books ORDER BY id FETCH FIRST 3 ROWS ONLY", 3);
    rows_count(b, "SELECT * FROM books ORDER BY id FETCH FIRST 1 ROW ONLY", 1);

    // -----------------------------------------------------------------------
    // ch04: NEW — LATERAL join
    // -----------------------------------------------------------------------
    // For each author, get their most expensive book using LATERAL
    ok(b, "SELECT a.name, recent.title, recent.price
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
    ok(b, "CREATE TABLE accounts (name TEXT NOT NULL, balance FLOAT NOT NULL)");
    ok(b, "INSERT INTO accounts VALUES ('Alice', 1000), ('Bob', 500)");

    let sid_sp = "tut_savepoint_accounts";
    exec_session(b, sid_sp, "BEGIN");
    exec_session(b, sid_sp, "UPDATE accounts SET balance = balance - 100 WHERE name = 'Alice'");
    exec_session(b, sid_sp, "SAVEPOINT after_debit");
    exec_session(b, sid_sp, "UPDATE accounts SET balance = balance + 100 WHERE name = 'Bob'");
    // ROLLBACK TO aborts entire txn in current implementation and starts fresh
    exec_session(b, sid_sp, "ROLLBACK TO SAVEPOINT after_debit");
    exec_session(b, sid_sp, "UPDATE accounts SET balance = balance + 100 WHERE name = 'Bob'");
    exec_session(b, sid_sp, "COMMIT");

    // RELEASE SAVEPOINT pattern
    let sid_rel = "tut_savepoint_release";
    exec_session(b, sid_rel, "BEGIN");
    exec_session(b, sid_rel, "SAVEPOINT sp1");
    exec_session(b, sid_rel, "RELEASE SAVEPOINT sp1");
    exec_session(b, sid_rel, "COMMIT");

    // -----------------------------------------------------------------------
    // ch04: NEW — SAVEPOINT (accepted; full partial-rollback not guaranteed)
    // -----------------------------------------------------------------------
    let sid = "tut_savepoint";
    exec_session(b, sid, "BEGIN");
    exec_session(b, sid, "INSERT INTO notes VALUES ('savepoint test')");
    exec_session(b, sid, "SAVEPOINT sp1");
    exec_session(b, sid, "RELEASE SAVEPOINT sp1");
    exec_session(b, sid, "COMMIT");

    // -----------------------------------------------------------------------
    // ch04: NEW — SET TRANSACTION (accepted as no-op)
    // -----------------------------------------------------------------------
    let sid2 = "tut_set_txn";
    exec_session(b, sid2, "BEGIN");
    exec_session(b, sid2, "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ");
    exec_session(b, sid2, "COMMIT");

    // -----------------------------------------------------------------------
    // ch04: CREATE DATABASE / DROP DATABASE / CREATE SCHEMA
    // -----------------------------------------------------------------------
    ok(b, "CREATE DATABASE testdb_tut");
    ok(b, "DROP DATABASE testdb_tut");
    ok(b, "DROP DATABASE IF EXISTS testdb_tut");       // idempotent (already dropped)
    ok(b, "CREATE DATABASE IF NOT EXISTS testdb_tut2");
    ok(b, "CREATE DATABASE IF NOT EXISTS testdb_tut2"); // idempotent
    ok(b, "DROP DATABASE IF EXISTS testdb_tut2");
    ok(b, "CREATE SCHEMA reporting");
    ok(b, "CREATE SCHEMA IF NOT EXISTS reporting");    // idempotent
}

#[test]
fn test_tutorial_all_sql_examples() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_tutorial_all_sql_examples_body(&b);
}

crate::net_tests!(test_tutorial_all_sql_examples);


// ---------------------------------------------------------------------------
// Chapter 3 value-level validation
// ---------------------------------------------------------------------------

/// Format a Value for comparison against the chapter's expected text output.
/// Matches the Display impl in sql::value::Value.
fn fmt_val(v: &sql::Value) -> String {
    format!("{v}")
}

/// Run `sql`, assert row count matches expected_rows.len(), and assert each
/// column value (as a formatted string) matches the expected matrix.
///
/// expected_rows: slice of row slices; each inner slice holds one string per column.
fn val(
    engine: &Arc<sql::engine::QueryEngine>,
    sql: &str,
    expected_rows: &[&[&str]],
) {
    let result = engine
        .execute(sql)
        .unwrap_or_else(|e| panic!("Query failed:\n  SQL: {sql}\n  Err: {e}"));

    assert_eq!(
        result.rows.len(),
        expected_rows.len(),
        "Row count mismatch for:\n  SQL: {sql}\n  Got {} rows, expected {}",
        result.rows.len(),
        expected_rows.len(),
    );

    for (row_idx, (row, expected_cols)) in result.rows.iter().zip(expected_rows.iter()).enumerate() {
        for (col_idx, expected) in expected_cols.iter().enumerate() {
            let got = row
                .get_by_idx(col_idx)
                .unwrap_or_else(|| panic!(
                    "Column {col_idx} missing at row {row_idx} for SQL: {sql}"
                ));
            let got_str = fmt_val(got);
            assert_eq!(
                got_str.as_str(),
                *expected,
                "Value mismatch at row {row_idx}, col {col_idx} for:\n  SQL: {sql}\n  Got: {got_str}\n  Expected: {expected}",
            );
        }
    }
}

fn test_tutorial_chapter3_values_body(b: &crate::common::Backend) {
    if b.is_network() { return; } // Uses val() helper that takes &QueryEngine directly
    let engine = b.as_engine().clone();
    // -----------------------------------------------------------------------
    // Step 1: DDL + initial data (mirrors setup_bookstore but inline so the
    //         state progression is explicit and matches the chapter exactly)
    // -----------------------------------------------------------------------
    ok(b, "CREATE TABLE authors (id INT NOT NULL, name TEXT NOT NULL, country TEXT)");
    ok(b, "CREATE TABLE books (id INT NOT NULL, title TEXT NOT NULL, author_id INT NOT NULL, price FLOAT, published INT)");
    ok(b, "CREATE TABLE orders (id INT NOT NULL, book_id INT NOT NULL, quantity INT NOT NULL, total_price FLOAT)");

    ok(b, "INSERT INTO authors VALUES (1, 'J.R.R. Tolkien', 'United Kingdom')");
    ok(b, "INSERT INTO authors VALUES (2, 'Frank Herbert', 'United States')");
    ok(b, "INSERT INTO authors VALUES (3, 'Ursula K. Le Guin', 'United States')");
    ok(b, "INSERT INTO authors VALUES (4, 'Cormac McCarthy', 'United States')");

    ok(b, "INSERT INTO books VALUES (1, 'The Hobbit', 1, 12.99, 1937)");
    ok(b, "INSERT INTO books VALUES (2, 'The Lord of the Rings', 1, 24.99, 1954)");
    ok(b, "INSERT INTO books VALUES (3, 'Dune', 2, 15.99, 1965)");
    ok(b, "INSERT INTO books VALUES (4, 'The Left Hand of Darkness', 3, 13.99, 1969)");
    ok(b, "INSERT INTO books VALUES (5, 'The Dispossessed', 3, 11.99, 1974)");
    ok(b, "INSERT INTO books VALUES (6, 'Blood Meridian', 4, 14.99, 1985)");
    ok(b, "INSERT INTO books VALUES (7, 'Children of Dune', 2, 13.99, 1976)");

    ok(b, "INSERT INTO orders VALUES (1, 1, 2, 25.98)");
    ok(b, "INSERT INTO orders VALUES (2, 3, 1, 15.99)");
    ok(b, "INSERT INTO orders VALUES (3, 2, 1, 24.99)");
    ok(b, "INSERT INTO orders VALUES (4, 4, 3, 41.97)");
    ok(b, "INSERT INTO orders VALUES (5, 3, 5, 79.95)");

    // -----------------------------------------------------------------------
    // SELECT * FROM books  (chapter p. "Retrieve all books")
    // -----------------------------------------------------------------------
    val(
        &engine,
        "SELECT * FROM books",
        &[
            &["1", "The Hobbit",                 "1", "12.99", "1937"],
            &["2", "The Lord of the Rings",       "1", "24.99", "1954"],
            &["3", "Dune",                        "2", "15.99", "1965"],
            &["4", "The Left Hand of Darkness",   "3", "13.99", "1969"],
            &["5", "The Dispossessed",             "3", "11.99", "1974"],
            &["6", "Blood Meridian",               "4", "14.99", "1985"],
            &["7", "Children of Dune",             "2", "13.99", "1976"],
        ],
    );

    // -----------------------------------------------------------------------
    // SELECT title, price FROM books WHERE price < 14.00
    // -----------------------------------------------------------------------
    val(
        &engine,
        "SELECT title, price FROM books WHERE price < 14.00",
        &[
            &["The Hobbit",                 "12.99"],
            &["The Left Hand of Darkness",  "13.99"],
            &["The Dispossessed",           "11.99"],
            &["Children of Dune",           "13.99"],
        ],
    );

    // -----------------------------------------------------------------------
    // WHERE published < 1970
    // -----------------------------------------------------------------------
    val(
        &engine,
        "SELECT title, published FROM books WHERE published < 1970",
        &[
            &["The Hobbit",                "1937"],
            &["The Lord of the Rings",     "1954"],
            &["Dune",                      "1965"],
            &["The Left Hand of Darkness", "1969"],
        ],
    );

    // -----------------------------------------------------------------------
    // WHERE price > 13.00 AND published > 1960
    // -----------------------------------------------------------------------
    val(
        &engine,
        "SELECT title, price FROM books WHERE price > 13.00 AND published > 1960",
        &[
            &["Dune",                      "15.99"],
            &["The Left Hand of Darkness", "13.99"],
            &["Blood Meridian",            "14.99"],
            &["Children of Dune",          "13.99"],
        ],
    );

    // -----------------------------------------------------------------------
    // ORDER BY price ASC
    // -----------------------------------------------------------------------
    val(
        &engine,
        "SELECT title, price FROM books ORDER BY price ASC",
        &[
            &["The Dispossessed",           "11.99"],
            &["The Hobbit",                 "12.99"],
            &["The Left Hand of Darkness",  "13.99"],
            &["Children of Dune",           "13.99"],
            &["Blood Meridian",             "14.99"],
            &["Dune",                       "15.99"],
            &["The Lord of the Rings",      "24.99"],
        ],
    );

    // -----------------------------------------------------------------------
    // LIMIT 3 most expensive
    // -----------------------------------------------------------------------
    val(
        &engine,
        "SELECT title, price FROM books ORDER BY price DESC LIMIT 3",
        &[
            &["The Lord of the Rings", "24.99"],
            &["Dune",                  "15.99"],
            &["Blood Meridian",        "14.99"],
        ],
    );

    // -----------------------------------------------------------------------
    // INNER JOIN books -> authors
    // -----------------------------------------------------------------------
    val(
        &engine,
        "SELECT b.title, a.name AS author, b.price
         FROM books b
         JOIN authors a ON b.author_id = a.id
         ORDER BY a.name, b.title",
        &[
            &["Blood Meridian",              "Cormac McCarthy",   "14.99"],
            &["Children of Dune",            "Frank Herbert",     "13.99"],
            &["Dune",                        "Frank Herbert",     "15.99"],
            &["The Hobbit",                  "J.R.R. Tolkien",    "12.99"],
            &["The Lord of the Rings",       "J.R.R. Tolkien",    "24.99"],
            &["The Dispossessed",            "Ursula K. Le Guin", "11.99"],
            &["The Left Hand of Darkness",   "Ursula K. Le Guin", "13.99"],
        ],
    );

    // -----------------------------------------------------------------------
    // INNER JOIN orders -> books
    // -----------------------------------------------------------------------
    val(
        &engine,
        "SELECT o.id AS order_id, b.title, o.quantity, o.total_price
         FROM orders o
         JOIN books b ON o.book_id = b.id
         ORDER BY o.id",
        &[
            &["1", "The Hobbit",                "2", "25.98"],
            &["2", "Dune",                      "1", "15.99"],
            &["3", "The Lord of the Rings",     "1", "24.99"],
            &["4", "The Left Hand of Darkness", "3", "41.97"],
            &["5", "Dune",                      "5", "79.95"],
        ],
    );

    // -----------------------------------------------------------------------
    // LEFT JOIN — after inserting Gene Wolfe (id=5)
    // -----------------------------------------------------------------------
    ok(b, "INSERT INTO authors VALUES (5, 'Gene Wolfe', 'United States')");

    val(
        &engine,
        "SELECT a.name, b.title
         FROM authors a
         LEFT JOIN books b ON b.author_id = a.id
         ORDER BY a.name, b.title",
        &[
            &["Cormac McCarthy",   "Blood Meridian"],
            &["Frank Herbert",     "Children of Dune"],
            &["Frank Herbert",     "Dune"],
            &["Gene Wolfe",        "NULL"],
            &["J.R.R. Tolkien",    "The Hobbit"],
            &["J.R.R. Tolkien",    "The Lord of the Rings"],
            &["Ursula K. Le Guin", "The Dispossessed"],
            &["Ursula K. Le Guin", "The Left Hand of Darkness"],
        ],
    );

    // -----------------------------------------------------------------------
    // JOIN USING — genre tables
    // -----------------------------------------------------------------------
    ok(b, "CREATE TABLE genres (genre_id INT NOT NULL, name TEXT NOT NULL)");
    ok(b, "CREATE TABLE book_genres (book_id INT NOT NULL, genre_id INT NOT NULL)");
    ok(b, "INSERT INTO genres VALUES (1, 'Fantasy'), (2, 'Science Fiction'), (3, 'Western')");
    ok(b, "INSERT INTO book_genres VALUES (1, 1), (2, 1), (3, 2), (4, 2), (5, 2), (6, 3), (7, 2)");

    val(
        &engine,
        "SELECT bg.book_id, g.name AS genre
         FROM book_genres bg
         JOIN genres g USING (genre_id)
         ORDER BY bg.book_id",
        &[
            &["1", "Fantasy"],
            &["2", "Fantasy"],
            &["3", "Science Fiction"],
            &["4", "Science Fiction"],
            &["5", "Science Fiction"],
            &["6", "Western"],
            &["7", "Science Fiction"],
        ],
    );

    // -----------------------------------------------------------------------
    // IS NULL — 0 rows before, then 1 row after inserting Anonymous
    // -----------------------------------------------------------------------
    val(&engine, "SELECT name FROM authors WHERE country IS NULL", &[]);

    ok(b, "INSERT INTO authors VALUES (6, 'Anonymous', NULL)");

    val(
        &engine,
        "SELECT name FROM authors WHERE country IS NULL",
        &[&["Anonymous"]],
    );

    // -----------------------------------------------------------------------
    // ILIKE
    // -----------------------------------------------------------------------
    val(
        &engine,
        "SELECT name FROM authors WHERE name ILIKE '%ursula%'",
        &[&["Ursula K. Le Guin"]],
    );

    // -----------------------------------------------------------------------
    // DISTINCT country
    // -----------------------------------------------------------------------
    val(
        &engine,
        "SELECT DISTINCT country FROM authors WHERE country IS NOT NULL ORDER BY country",
        &[
            &["United Kingdom"],
            &["United States"],
        ],
    );

    // -----------------------------------------------------------------------
    // RETURNING — INSERT order 6
    // -----------------------------------------------------------------------
    val(
        &engine,
        "INSERT INTO orders VALUES (6, 2, 3, 74.97) RETURNING id, total_price",
        &[&["6", "74.97"]],
    );

    // -----------------------------------------------------------------------
    // RETURNING — UPDATE Blood Meridian price (10% discount)
    //   ROUND(14.99 * 0.90, 2) = ROUND(13.491, 2) = 13.49
    // -----------------------------------------------------------------------
    val(
        &engine,
        "UPDATE books SET price = ROUND(price * 0.90, 2) WHERE author_id = 4 RETURNING title, price",
        &[&["Blood Meridian", "13.49"]],
    );

    // -----------------------------------------------------------------------
    // RETURNING — DELETE Anonymous (id=6)
    // -----------------------------------------------------------------------
    val(
        &engine,
        "DELETE FROM authors WHERE id = 6 RETURNING name",
        &[&["Anonymous"]],
    );

    // -----------------------------------------------------------------------
    // UNION — 5 authors (Gene Wolfe still present) + 7 books = 12 rows
    // -----------------------------------------------------------------------
    val(
        &engine,
        "SELECT name AS label, 'author' AS kind FROM authors
         UNION
         SELECT title, 'book' FROM books
         ORDER BY kind, label",
        &[
            &["Cormac McCarthy",            "author"],
            &["Frank Herbert",              "author"],
            &["Gene Wolfe",                 "author"],
            &["J.R.R. Tolkien",             "author"],
            &["Ursula K. Le Guin",          "author"],
            &["Blood Meridian",             "book"],
            &["Children of Dune",           "book"],
            &["Dune",                       "book"],
            &["The Dispossessed",           "book"],
            &["The Hobbit",                 "book"],
            &["The Left Hand of Darkness",  "book"],
            &["The Lord of the Rings",      "book"],
        ],
    );

    // -----------------------------------------------------------------------
    // CTE — author revenue > $50
    //   Tolkien:  orders 1 (25.98) + 3 (24.99) + 6 (74.97) = 125.94
    //   Herbert:  orders 2 (15.99) + 5 (79.95) = 95.94
    // -----------------------------------------------------------------------
    val(
        &engine,
        "WITH book_revenue AS (
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
        ORDER BY total_revenue DESC",
        &[
            &["J.R.R. Tolkien", "125.94"],
            &["Frank Herbert",  "95.94"],
        ],
    );

    // -----------------------------------------------------------------------
    // Aggregates — book count per author
    // -----------------------------------------------------------------------
    val(
        &engine,
        "SELECT a.name, COUNT(*) AS book_count
         FROM books b
         JOIN authors a ON b.author_id = a.id
         GROUP BY a.name
         ORDER BY book_count DESC, a.name",
        &[
            &["Frank Herbert",      "2"],
            &["J.R.R. Tolkien",     "2"],
            &["Ursula K. Le Guin",  "2"],
            &["Cormac McCarthy",    "1"],
        ],
    );

    // -----------------------------------------------------------------------
    // Aggregates — AVG/MIN/MAX/SUM of prices
    //   At this point Blood Meridian = 13.49 (discounted in RETURNING section).
    //   Prices: 12.99, 24.99, 15.99, 13.99, 11.99, 13.49, 13.99
    //   Sum  = 107.43
    //   Min  = 11.99
    //   Max  = 24.99
    //   Avg  = 107.43 / 7 = 15.347142857142857...
    // -----------------------------------------------------------------------
    {
        let r = b.try_execute("SELECT AVG(price) AS avg_price, MIN(price) AS min_price,
                    MAX(price) AS max_price, SUM(price) AS total_catalog_value
             FROM books").expect("AVG query should succeed");
        assert_eq!(r.rows.len(), 1, "AVG query should return 1 row");
        let avg_val = r.rows[0].get_by_idx(0).map(|v| format!("{v}")).unwrap_or_default();
        let min_val = r.rows[0].get_by_idx(1).map(|v| format!("{v}")).unwrap_or_default();
        let max_val = r.rows[0].get_by_idx(2).map(|v| format!("{v}")).unwrap_or_default();
        let sum_val = r.rows[0].get_by_idx(3).map(|v| format!("{v}")).unwrap_or_default();
        assert_eq!(min_val, "11.99", "MIN(price) should be 11.99");
        assert_eq!(max_val, "24.99", "MAX(price) should be 24.99");
        // Float arithmetic: 107.43 stored as 107.42999999999998 due to f64 precision
        assert_eq!(sum_val, "107.42999999999998", "SUM(price) float repr");
        // Avg: 107.42999999999998 / 7 = 15.347142857142854
        assert_eq!(avg_val, "15.347142857142854", "AVG(price) float repr");
    }

    // -----------------------------------------------------------------------
    // Aggregates — revenue per book (ORDER BY revenue DESC)
    //   At this point order 6 (book_id=2, qty=3, 74.97) has been inserted.
    //   The Lord of the Rings (book_id=2): orders 3+6 qty=1+3=4  rev=24.99+74.97=99.96
    //   Dune (book_id=3):                  orders 2+5 qty=1+5=6  rev=15.99+79.95=95.94
    //   The Left Hand... (book_id=4):      order  4   qty=3       rev=41.97
    //   The Hobbit (book_id=1):            order  1   qty=2       rev=25.98
    //
    // ORDER BY revenue DESC: 99.96 > 95.94 > 41.97 > 25.98
    // -----------------------------------------------------------------------
    val(
        &engine,
        "SELECT b.title, SUM(o.quantity) AS units_sold, SUM(o.total_price) AS revenue
         FROM orders o
         JOIN books b ON o.book_id = b.id
         GROUP BY b.title
         ORDER BY revenue DESC",
        &[
            &["The Lord of the Rings",     "4", "99.96"],
            &["Dune",                      "6", "95.94"],
            &["The Left Hand of Darkness", "3", "41.97"],
            &["The Hobbit",                "2", "25.98"],
        ],
    );

    // -----------------------------------------------------------------------
    // UPDATE Herbert books +10%
    //   Dune:             ROUND(15.99 * 1.10, 2) = ROUND(17.589, 2) = 17.59
    //   Children of Dune: ROUND(13.99 * 1.10, 2) = ROUND(15.389, 2) = 15.39
    // -----------------------------------------------------------------------
    ok(b, "UPDATE books SET price = ROUND(price * 1.10, 2) WHERE author_id = 2");

    val(
        &engine,
        "SELECT title, price FROM books WHERE author_id = 2",
        &[
            &["Dune",             "17.59"],
            &["Children of Dune", "15.39"],
        ],
    );

    // -----------------------------------------------------------------------
    // DELETE Gene Wolfe (id=5) and orders for book_id=1
    // -----------------------------------------------------------------------
    ok(b, "DELETE FROM authors WHERE id = 5");
    ok(b, "DELETE FROM orders WHERE book_id = 1");

    // -----------------------------------------------------------------------
    // CASE WHEN — all 7 books with updated prices
    //   After updates: Blood Meridian=13.49, Dune=17.59, Children=15.39;
    //   others unchanged.
    //   ORDER BY price:
    //     The Dispossessed   11.99  budget
    //     The Hobbit         12.99  budget
    //     Blood Meridian     13.49  mid-range
    //     The Left Hand...   13.99  mid-range
    //     Children of Dune   15.39  mid-range
    //     Dune               17.59  mid-range
    //     The Lord...        24.99  premium
    // -----------------------------------------------------------------------
    val(
        &engine,
        "SELECT title, price,
               CASE WHEN price < 13.00 THEN 'budget'
                    WHEN price < 20.00 THEN 'mid-range'
                    ELSE 'premium'
               END AS tier
         FROM books
         ORDER BY price",
        &[
            &["The Dispossessed",           "11.99", "budget"],
            &["The Hobbit",                 "12.99", "budget"],
            &["Blood Meridian",             "13.49", "mid-range"],
            &["The Left Hand of Darkness",  "13.99", "mid-range"],
            &["Children of Dune",           "15.39", "mid-range"],
            &["Dune",                       "17.59", "mid-range"],
            &["The Lord of the Rings",      "24.99", "premium"],
        ],
    );

    // -----------------------------------------------------------------------
    // COALESCE — insert Anonymous (id=6), query 5 rows, delete
    //   (4 remaining authors after Gene Wolfe deleted + Anonymous = 5)
    // -----------------------------------------------------------------------
    ok(b, "INSERT INTO authors VALUES (6, 'Anonymous', NULL)");

    val(
        &engine,
        "SELECT name, COALESCE(country, 'Unknown') AS country FROM authors ORDER BY name",
        &[
            &["Anonymous",          "Unknown"],
            &["Cormac McCarthy",    "United States"],
            &["Frank Herbert",      "United States"],
            &["J.R.R. Tolkien",     "United Kingdom"],
            &["Ursula K. Le Guin",  "United States"],
        ],
    );

    ok(b, "DELETE FROM authors WHERE id = 6");

    // -----------------------------------------------------------------------
    // ALTER TABLE ADD COLUMN + UPDATE bio
    // -----------------------------------------------------------------------
    ok(b, "ALTER TABLE authors ADD COLUMN bio TEXT");
    ok(b, "UPDATE authors SET bio = 'Author of Middle-earth.' WHERE id = 1");

    val(
        &engine,
        "SELECT id, name, bio FROM authors ORDER BY id",
        &[
            &["1", "J.R.R. Tolkien",    "Author of Middle-earth."],
            &["2", "Frank Herbert",      "NULL"],
            &["3", "Ursula K. Le Guin",  "NULL"],
            &["4", "Cormac McCarthy",    "NULL"],
        ],
    );

    // -----------------------------------------------------------------------
    // String function — SUBSTRING + POSITION
    //   Books where title contains a space (all except "Dune") → 6 rows
    //   ORDER: heap/insertion order; WHERE filters Dune out
    //   Expected titles & first words in insertion order:
    //     The Hobbit                → The
    //     The Lord of the Rings     → The
    //     The Left Hand of Darkness → The
    //     The Dispossessed          → The
    //     Blood Meridian            → Blood
    //     Children of Dune          → Children
    // -----------------------------------------------------------------------
    val(
        &engine,
        "SELECT title, SUBSTRING(title, 1, POSITION(' ' IN title) - 1) AS first_word
         FROM books
         WHERE POSITION(' ' IN title) > 0",
        &[
            &["The Hobbit",                 "The"],
            &["The Lord of the Rings",      "The"],
            &["The Left Hand of Darkness",  "The"],
            &["The Dispossessed",           "The"],
            &["Blood Meridian",             "Blood"],
            &["Children of Dune",           "Children"],
        ],
    );

    // -----------------------------------------------------------------------
    // Window functions — ROW_NUMBER OVER (PARTITION BY author_id ORDER BY price DESC)
    //   At this point prices: Blood Meridian=13.49, Dune=17.59, Children=15.39
    //   (Gene Wolfe deleted; 4 authors × their books)
    //   ORDER BY a.name, price_rank
    //   Cormac McCarthy:   Blood Meridian 13.49 rank=1
    //   Frank Herbert:     Dune 17.59 rank=1; Children of Dune 15.39 rank=2
    //   J.R.R. Tolkien:    The Lord of the Rings 24.99 rank=1; The Hobbit 12.99 rank=2
    //   Ursula K. Le Guin: The Left Hand of Darkness 13.99 rank=1; The Dispossessed 11.99 rank=2
    // -----------------------------------------------------------------------
    val(
        &engine,
        "SELECT a.name AS author, b.title, b.price,
               ROW_NUMBER() OVER (PARTITION BY b.author_id ORDER BY b.price DESC) AS price_rank
         FROM books b
         JOIN authors a ON a.id = b.author_id
         ORDER BY a.name, price_rank",
        &[
            &["Cormac McCarthy",   "Blood Meridian",              "13.49", "1"],
            &["Frank Herbert",     "Dune",                        "17.59", "1"],
            &["Frank Herbert",     "Children of Dune",            "15.39", "2"],
            &["J.R.R. Tolkien",    "The Lord of the Rings",       "24.99", "1"],
            &["J.R.R. Tolkien",    "The Hobbit",                  "12.99", "2"],
            &["Ursula K. Le Guin", "The Left Hand of Darkness",   "13.99", "1"],
            &["Ursula K. Le Guin", "The Dispossessed",            "11.99", "2"],
        ],
    );

    // -----------------------------------------------------------------------
    // WITH RECURSIVE series 1..5
    // -----------------------------------------------------------------------
    val(
        &engine,
        "WITH RECURSIVE series(n) AS (
            SELECT 1
            UNION ALL
            SELECT n + 1 FROM series WHERE n < 5
        )
        SELECT n FROM series",
        &[&["1"], &["2"], &["3"], &["4"], &["5"]],
    );
}

#[test]
fn test_tutorial_chapter3_values() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_tutorial_chapter3_values_body(&b);
}

crate::net_tests!(test_tutorial_chapter3_values);

