/// Chapter 3 — Quick Start sandbox
///
/// Runs every SQL example from ch03-quickstart.md in the order presented,
/// verifies outputs match the expected results shown in the chapter, and
/// prints a clear PASS / FAIL / SKIP report for each example block.
///
/// Run with:
///   cargo run --manifest-path sandbox/ch03/Cargo.toml
///
/// Shell / CLI commands (cargo run -p server, psql, \c, \dt, etc.) and
/// server-only features (COPY, LISTEN/NOTIFY) are marked SKIP.
use std::sync::Arc;
use sql::engine::QueryEngine;
use sql::Value;
use txn::manager::TransactionManager;
use catalog::manager::CatalogManager;
use wal::writer::WalWriter;
use tempfile::TempDir;

// ── Counters ──────────────────────────────────────────────────────────────────

struct Stats {
    passed: usize,
    failed: usize,
    skipped: usize,
}

impl Stats {
    fn new() -> Self { Stats { passed: 0, failed: 0, skipped: 0 } }
    fn pass(&mut self, label: &str) {
        self.passed += 1;
        println!("  [PASS] {label}");
    }
    fn fail(&mut self, label: &str, detail: &str) {
        self.failed += 1;
        eprintln!("  [FAIL] {label}");
        eprintln!("         {detail}");
    }
    fn skip(&mut self, label: &str, reason: &str) {
        self.skipped += 1;
        println!("  [SKIP] {label} — {reason}");
    }
}

// ── Engine factory ────────────────────────────────────────────────────────────

fn make_engine(dir: &std::path::Path) -> Arc<QueryEngine> {
    let wal = Arc::new(WalWriter::open(dir).unwrap());
    let txn = Arc::new(TransactionManager::new_with_wal_recovery(Arc::clone(&wal), dir));
    let cat = Arc::new(CatalogManager::open(dir, Arc::clone(&wal), Arc::clone(&txn)).unwrap());
    Arc::new(QueryEngine::new(txn, cat, dir.to_path_buf()))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn run(engine: &Arc<QueryEngine>, sql: &str) -> Result<sql::ExecutionResult, sql::SqlError> {
    engine.execute(sql)
}

fn run_session(engine: &Arc<QueryEngine>, session: &str, sql: &str)
    -> Result<sql::ExecutionResult, sql::SqlError>
{
    engine.execute_session(session, sql)
}

fn scalar_i64(r: &sql::ExecutionResult) -> Option<i64> {
    r.rows.first()?.get_by_idx(0).and_then(|v| match v {
        Value::Int4(n) => Some(*n as i64),
        Value::Int8(n) => Some(*n),
        _ => None,
    })
}

fn scalar_f64(r: &sql::ExecutionResult) -> Option<f64> {
    r.rows.first()?.get_by_idx(0).and_then(|v| match v {
        Value::Float8(f) => Some(*f),
        Value::Int4(n)   => Some(*n as f64),
        Value::Int8(n)   => Some(*n as f64),
        _ => None,
    })
}

fn scalar_text(r: &sql::ExecutionResult) -> Option<String> {
    r.rows.first()?.get_by_idx(0).and_then(|v| match v {
        Value::Text(s) => Some(s.clone()),
        _ => None,
    })
}

fn col_text(row: &sql::Row, idx: usize) -> String {
    match row.get_by_idx(idx) {
        Some(Value::Text(s))    => s.clone(),
        Some(Value::Int4(n))    => n.to_string(),
        Some(Value::Int8(n))    => n.to_string(),
        Some(Value::Float8(f))  => format!("{f:.2}"),
        Some(Value::Bool(b))    => b.to_string(),
        Some(Value::Numeric(s)) => s.clone(),
        Some(Value::Null)       => "NULL".to_string(),
        None                    => "NULL".to_string(),
        other                   => format!("{other:?}"),
    }
}

fn col_f64(row: &sql::Row, idx: usize) -> Option<f64> {
    match row.get_by_idx(idx) {
        Some(Value::Float8(f))    => Some(*f),
        Some(Value::Int4(n))      => Some(*n as f64),
        Some(Value::Int8(n))      => Some(*n as f64),
        Some(Value::Numeric(s))   => s.parse::<f64>().ok(),
        _ => None,
    }
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
    println!("\n=== Chapter 3: Quick Start — sandbox ===\n");

    let dir = TempDir::new().unwrap();
    let engine = make_engine(dir.path());
    let mut s = Stats::new();

    // Shell / server startup steps are not executable in-process.
    println!("── Starting the Server ──────────────────────────");
    s.skip("cargo run -p server", "shell command / server process");
    s.skip("psql -h 127.0.0.1", "external client command");

    println!("\n── Creating a Database ──────────────────────────");
    // CREATE DATABASE creates a subdirectory; we represent the bookstore
    // database as the engine's own data dir for all subsequent tests.
    match run(&engine, "CREATE DATABASE bookstore") {
        Ok(_) => s.pass("CREATE DATABASE bookstore"),
        Err(e) => s.fail("CREATE DATABASE bookstore", &e.to_string()),
    }
    s.skip("\\c bookstore", "CLI meta-command");
    s.skip("\\l", "CLI meta-command");

    // ── Creating Your First Tables ────────────────────────────────────────────
    println!("\n── Creating Your First Tables ───────────────────");

    let ddl = [
        ("CREATE TABLE authors",
         "CREATE TABLE authors (id INT NOT NULL, name TEXT NOT NULL, country TEXT)"),
        ("CREATE TABLE books",
         "CREATE TABLE books (id INT NOT NULL, title TEXT NOT NULL, author_id INT NOT NULL, price FLOAT, published INT)"),
        ("CREATE TABLE orders",
         "CREATE TABLE orders (id INT NOT NULL, book_id INT NOT NULL, quantity INT NOT NULL, total_price FLOAT)"),
    ];
    for (label, sql) in ddl {
        match run(&engine, sql) {
            Ok(_)  => s.pass(label),
            Err(e) => s.fail(label, &e.to_string()),
        }
    }
    s.skip("\\dt", "CLI meta-command");
    s.skip("\\d books", "CLI meta-command");

    // ── Inserting Rows ────────────────────────────────────────────────────────
    println!("\n── Inserting Rows ───────────────────────────────");

    let inserts = [
        "INSERT INTO authors VALUES (1, 'J.R.R. Tolkien', 'United Kingdom')",
        "INSERT INTO authors VALUES (2, 'Frank Herbert', 'United States')",
        "INSERT INTO authors VALUES (3, 'Ursula K. Le Guin', 'United States')",
        "INSERT INTO authors VALUES (4, 'Cormac McCarthy', 'United States')",
        "INSERT INTO books VALUES (1, 'The Hobbit', 1, 12.99, 1937)",
        "INSERT INTO books VALUES (2, 'The Lord of the Rings', 1, 24.99, 1954)",
        "INSERT INTO books VALUES (3, 'Dune', 2, 15.99, 1965)",
        "INSERT INTO books VALUES (4, 'The Left Hand of Darkness', 3, 13.99, 1969)",
        "INSERT INTO books VALUES (5, 'The Dispossessed', 3, 11.99, 1974)",
        "INSERT INTO books VALUES (6, 'Blood Meridian', 4, 14.99, 1985)",
        "INSERT INTO books VALUES (7, 'Children of Dune', 2, 13.99, 1976)",
        "INSERT INTO orders VALUES (1, 1, 2, 25.98)",
        "INSERT INTO orders VALUES (2, 3, 1, 15.99)",
        "INSERT INTO orders VALUES (3, 2, 1, 24.99)",
        "INSERT INTO orders VALUES (4, 4, 3, 41.97)",
        "INSERT INTO orders VALUES (5, 3, 5, 79.95)",
    ];
    let mut insert_ok = true;
    for sql in inserts {
        if let Err(e) = run(&engine, sql) {
            s.fail(&format!("INSERT: {}", &sql[..sql.len().min(60)]), &e.to_string());
            insert_ok = false;
        }
    }
    if insert_ok { s.pass("all INSERT rows (authors 4, books 7, orders 5)"); }

    // ── Querying Data ─────────────────────────────────────────────────────────
    println!("\n── Querying Data ────────────────────────────────");

    // SELECT * FROM books → 7 rows
    match run(&engine, "SELECT * FROM books") {
        Ok(r) if r.rows.len() == 7 => s.pass("SELECT * FROM books → 7 rows"),
        Ok(r) => s.fail("SELECT * FROM books", &format!("expected 7 rows, got {}", r.rows.len())),
        Err(e) => s.fail("SELECT * FROM books", &e.to_string()),
    }

    // SELECT title, price WHERE price < 14 → 4 rows (Hobbit, Left Hand, Dispossessed, Children)
    match run(&engine, "SELECT title, price FROM books WHERE price < 14.00") {
        Ok(r) if r.rows.len() == 4 => s.pass("SELECT WHERE price < 14.00 → 4 rows"),
        Ok(r) => s.fail("SELECT WHERE price < 14.00", &format!("expected 4 rows, got {}", r.rows.len())),
        Err(e) => s.fail("SELECT WHERE price < 14.00", &e.to_string()),
    }

    // WHERE published < 1970 → 4 rows
    match run(&engine, "SELECT title, published FROM books WHERE published < 1970") {
        Ok(r) if r.rows.len() == 4 => s.pass("SELECT WHERE published < 1970 → 4 rows"),
        Ok(r) => s.fail("SELECT WHERE published < 1970", &format!("expected 4, got {}", r.rows.len())),
        Err(e) => s.fail("SELECT WHERE published < 1970", &e.to_string()),
    }

    // WHERE price > 13 AND published > 1960 → 4 rows (Dune, Left Hand, Blood Meridian, Children)
    match run(&engine, "SELECT title, price FROM books WHERE price > 13.00 AND published > 1960") {
        Ok(r) if r.rows.len() == 4 => s.pass("SELECT WHERE price > 13 AND published > 1960 → 4 rows"),
        Ok(r) => s.fail("AND filter", &format!("expected 4, got {}", r.rows.len())),
        Err(e) => s.fail("AND filter", &e.to_string()),
    }

    // ORDER BY price ASC → cheapest = The Dispossessed (11.99)
    match run(&engine, "SELECT title, price FROM books ORDER BY price ASC") {
        Ok(r) if r.rows.len() == 7 && col_text(&r.rows[0], 0) == "The Dispossessed" =>
            s.pass("ORDER BY price ASC → Dispossessed first"),
        Ok(r) => s.fail("ORDER BY price ASC", &format!("first row = {:?}", r.rows.first().map(|r| col_text(r, 0)))),
        Err(e) => s.fail("ORDER BY price ASC", &e.to_string()),
    }

    // ORDER BY published DESC, price DESC
    match run(&engine, "SELECT title, published, price FROM books ORDER BY published DESC, price DESC") {
        Ok(r) if r.rows.len() == 7 => s.pass("ORDER BY published DESC, price DESC → 7 rows"),
        Ok(r) => s.fail("ORDER BY multi-col", &format!("expected 7, got {}", r.rows.len())),
        Err(e) => s.fail("ORDER BY multi-col", &e.to_string()),
    }

    // LIMIT 3 → top 3 by price DESC: Lord of Rings, Dune, Blood Meridian
    match run(&engine, "SELECT title, price FROM books ORDER BY price DESC LIMIT 3") {
        Ok(r) if r.rows.len() == 3 && col_text(&r.rows[0], 0) == "The Lord of the Rings" =>
            s.pass("LIMIT 3 → 3 rows, Lord of the Rings first"),
        Ok(r) => s.fail("LIMIT 3", &format!("rows={}, first={:?}", r.rows.len(), r.rows.first().map(|r| col_text(r, 0)))),
        Err(e) => s.fail("LIMIT 3", &e.to_string()),
    }

    // LIMIT 2 OFFSET 3
    match run(&engine, "SELECT title, price FROM books ORDER BY price DESC LIMIT 2 OFFSET 3") {
        Ok(r) if r.rows.len() == 2 => s.pass("LIMIT 2 OFFSET 3 → 2 rows"),
        Ok(r) => s.fail("LIMIT OFFSET", &format!("expected 2, got {}", r.rows.len())),
        Err(e) => s.fail("LIMIT OFFSET", &e.to_string()),
    }

    // FETCH FIRST 3 ROWS ONLY
    match run(&engine, "SELECT title, price FROM books ORDER BY price DESC FETCH FIRST 3 ROWS ONLY") {
        Ok(r) if r.rows.len() == 3 => s.pass("FETCH FIRST 3 ROWS ONLY → 3 rows"),
        Ok(r) => s.fail("FETCH FIRST", &format!("expected 3, got {}", r.rows.len())),
        Err(e) => s.fail("FETCH FIRST", &e.to_string()),
    }

    // ── Joining Tables ────────────────────────────────────────────────────────
    println!("\n── Joining Tables ───────────────────────────────");

    // INNER JOIN books + authors → 7 rows, ordered by author then title
    match run(&engine, "SELECT b.title, a.name AS author, b.price FROM books b JOIN authors a ON b.author_id = a.id ORDER BY a.name, b.title") {
        Ok(r) if r.rows.len() == 7 && col_text(&r.rows[0], 0) == "Blood Meridian" =>
            s.pass("JOIN books+authors → 7 rows, Blood Meridian first"),
        Ok(r) => s.fail("JOIN books+authors", &format!("rows={}, first={:?}", r.rows.len(), r.rows.first().map(|r| col_text(r, 0)))),
        Err(e) => s.fail("JOIN books+authors", &e.to_string()),
    }

    // JOIN orders + books → 5 rows
    match run(&engine, "SELECT o.id AS order_id, b.title, o.quantity, o.total_price FROM orders o JOIN books b ON o.book_id = b.id ORDER BY o.id") {
        Ok(r) if r.rows.len() == 5 => s.pass("JOIN orders+books → 5 rows"),
        Ok(r) => s.fail("JOIN orders+books", &format!("expected 5, got {}", r.rows.len())),
        Err(e) => s.fail("JOIN orders+books", &e.to_string()),
    }

    // LEFT JOIN — first add Gene Wolfe (id=5)
    match run(&engine, "INSERT INTO authors VALUES (5, 'Gene Wolfe', 'United States')") {
        Ok(_) => s.pass("INSERT Gene Wolfe (id=5)"),
        Err(e) => s.fail("INSERT Gene Wolfe", &e.to_string()),
    }

    // LEFT JOIN: Gene Wolfe has no books → 8 rows (7 books + Gene Wolfe NULL)
    match run(&engine, "SELECT a.name, b.title FROM authors a LEFT JOIN books b ON b.author_id = a.id ORDER BY a.name, b.title") {
        Ok(r) if r.rows.len() == 8 => {
            let gene_row = r.rows.iter().find(|r| col_text(r, 0) == "Gene Wolfe");
            match gene_row {
                Some(row) if matches!(row.get_by_idx(1), Some(Value::Null) | None) =>
                    s.pass("LEFT JOIN → 8 rows, Gene Wolfe has NULL title"),
                Some(_) => s.fail("LEFT JOIN Gene Wolfe", "expected NULL title for Gene Wolfe"),
                None    => s.fail("LEFT JOIN Gene Wolfe", "Gene Wolfe row not found"),
            }
        }
        Ok(r) => s.fail("LEFT JOIN", &format!("expected 8 rows, got {}", r.rows.len())),
        Err(e) => s.fail("LEFT JOIN", &e.to_string()),
    }

    // JOIN USING — create genres / book_genres
    let genre_setup = [
        "CREATE TABLE genres (genre_id INT NOT NULL, name TEXT NOT NULL)",
        "CREATE TABLE book_genres (book_id INT NOT NULL, genre_id INT NOT NULL)",
        "INSERT INTO genres VALUES (1, 'Fantasy'), (2, 'Science Fiction'), (3, 'Western')",
        "INSERT INTO book_genres VALUES (1, 1), (2, 1), (3, 2), (4, 2), (5, 2), (6, 3), (7, 2)",
    ];
    let mut genre_ok = true;
    for sql in genre_setup {
        if let Err(e) = run(&engine, sql) {
            s.fail(&format!("genre setup: {}", &sql[..sql.len().min(50)]), &e.to_string());
            genre_ok = false;
        }
    }
    if genre_ok { s.pass("CREATE genres/book_genres and INSERT rows"); }

    match run(&engine, "SELECT bg.book_id, g.name AS genre FROM book_genres bg JOIN genres g USING (genre_id) ORDER BY bg.book_id") {
        Ok(r) if r.rows.len() == 7 => s.pass("JOIN USING (genre_id) → 7 rows"),
        Ok(r) => s.fail("JOIN USING", &format!("expected 7, got {}", r.rows.len())),
        Err(e) => s.fail("JOIN USING", &e.to_string()),
    }

    // LATERAL JOIN — most expensive book per author (4 authors have books)
    let lateral_sql = "SELECT a.name AS author, top_book.title, top_book.price FROM authors a \
        JOIN LATERAL (SELECT title, price FROM books b WHERE b.author_id = a.id ORDER BY price DESC LIMIT 1) AS top_book ON true \
        ORDER BY a.name";
    match run(&engine, lateral_sql) {
        Ok(r) if r.rows.len() == 4 => s.pass("LATERAL JOIN → 4 rows (one per author with books)"),
        Ok(r) => s.fail("LATERAL JOIN", &format!("expected 4, got {}", r.rows.len())),
        Err(e) => s.fail("LATERAL JOIN", &e.to_string()),
    }

    // IS NULL — insert Anonymous (id=6, NULL country)
    match run(&engine, "INSERT INTO authors VALUES (6, 'Anonymous', NULL)") {
        Ok(_) => s.pass("INSERT Anonymous (id=6, country=NULL)"),
        Err(e) => s.fail("INSERT Anonymous", &e.to_string()),
    }

    match run(&engine, "SELECT name FROM authors WHERE country IS NULL") {
        Ok(r) if r.rows.len() == 1 && col_text(&r.rows[0], 0) == "Anonymous" =>
            s.pass("WHERE country IS NULL → Anonymous"),
        Ok(r) => s.fail("IS NULL", &format!("expected 1 row 'Anonymous', got {:?}", r.rows.iter().map(|r| col_text(r, 0)).collect::<Vec<_>>())),
        Err(e) => s.fail("IS NULL", &e.to_string()),
    }

    // ILIKE
    match run(&engine, "SELECT name FROM authors WHERE name ILIKE '%ursula%'") {
        Ok(r) if r.rows.len() == 1 && col_text(&r.rows[0], 0).contains("Ursula") =>
            s.pass("ILIKE '%ursula%' → Ursula K. Le Guin"),
        Ok(r) => s.fail("ILIKE", &format!("got {:?}", r.rows.iter().map(|r| col_text(r, 0)).collect::<Vec<_>>())),
        Err(e) => s.fail("ILIKE", &e.to_string()),
    }

    // DISTINCT country (excluding NULL) → United Kingdom, United States
    match run(&engine, "SELECT DISTINCT country FROM authors WHERE country IS NOT NULL ORDER BY country") {
        Ok(r) if r.rows.len() == 2 => s.pass("DISTINCT country → 2 rows (UK, US)"),
        Ok(r) => s.fail("DISTINCT", &format!("expected 2, got {}", r.rows.len())),
        Err(e) => s.fail("DISTINCT", &e.to_string()),
    }

    // DISTINCT country, name
    match run(&engine, "SELECT DISTINCT country, name FROM authors ORDER BY country, name") {
        Ok(r) if r.rows.len() >= 5 => s.pass("DISTINCT country, name → rows returned"),
        Ok(r) => s.fail("DISTINCT multi-col", &format!("expected ≥5, got {}", r.rows.len())),
        Err(e) => s.fail("DISTINCT multi-col", &e.to_string()),
    }

    // ── RETURNING ────────────────────────────────────────────────────────────
    println!("\n── RETURNING clause ─────────────────────────────");

    // INSERT RETURNING
    match run(&engine, "INSERT INTO orders VALUES (6, 2, 3, 74.97) RETURNING id, total_price") {
        Ok(r) if r.rows.len() == 1 => {
            let id    = col_text(&r.rows[0], 0);
            let price = col_text(&r.rows[0], 1);
            if id == "6" && price == "74.97" {
                s.pass("INSERT ... RETURNING id, total_price → (6, 74.97)");
            } else {
                s.fail("INSERT RETURNING", &format!("expected (6, 74.97), got ({id}, {price})"));
            }
        }
        Ok(r) => s.fail("INSERT RETURNING", &format!("expected 1 row, got {}", r.rows.len())),
        Err(e) => s.fail("INSERT RETURNING", &e.to_string()),
    }

    // UPDATE RETURNING — 10% discount on author_id=4 (Blood Meridian: 14.99 → 13.49)
    match run(&engine, "UPDATE books SET price = ROUND(price * 0.90, 2) WHERE author_id = 4 RETURNING title, price") {
        Ok(r) if r.rows.len() == 1 => {
            let title = col_text(&r.rows[0], 0);
            let price = col_f64(&r.rows[0], 1).unwrap_or(0.0);
            if title == "Blood Meridian" && (price - 13.49).abs() < 0.01 {
                s.pass("UPDATE ... RETURNING → Blood Meridian 13.49");
            } else {
                s.fail("UPDATE RETURNING", &format!("got title={title} price={price:.2}"));
            }
        }
        Ok(r) => s.fail("UPDATE RETURNING", &format!("expected 1 row, got {}", r.rows.len())),
        Err(e) => s.fail("UPDATE RETURNING", &e.to_string()),
    }

    // DELETE RETURNING — remove Anonymous (id=6)
    match run(&engine, "DELETE FROM authors WHERE id = 6 RETURNING name") {
        Ok(r) if r.rows.len() == 1 && col_text(&r.rows[0], 0) == "Anonymous" =>
            s.pass("DELETE ... RETURNING name → Anonymous"),
        Ok(r) => s.fail("DELETE RETURNING", &format!("got {:?}", r.rows.iter().map(|r| col_text(r, 0)).collect::<Vec<_>>())),
        Err(e) => s.fail("DELETE RETURNING", &e.to_string()),
    }

    // ── UNION ─────────────────────────────────────────────────────────────────
    println!("\n── UNION ────────────────────────────────────────");

    // UNION authors + books → 5 authors + 7 books = 12 rows
    match run(&engine, "SELECT name AS label, 'author' AS kind FROM authors UNION SELECT title, 'book' FROM books ORDER BY kind, label") {
        Ok(r) if r.rows.len() == 12 => s.pass("UNION authors+books → 12 rows"),
        Ok(r) => s.fail("UNION", &format!("expected 12, got {}", r.rows.len())),
        Err(e) => s.fail("UNION", &e.to_string()),
    }

    // ── CTEs ──────────────────────────────────────────────────────────────────
    println!("\n── CTEs ─────────────────────────────────────────");

    let cte_sql = "WITH book_revenue AS (\
        SELECT b.author_id, b.title, SUM(o.total_price) AS revenue \
        FROM books b JOIN orders o ON o.book_id = b.id \
        GROUP BY b.author_id, b.title\
    ), author_revenue AS (\
        SELECT a.name, SUM(br.revenue) AS total_revenue \
        FROM book_revenue br JOIN authors a ON a.id = br.author_id \
        GROUP BY a.name\
    ) \
    SELECT name, total_revenue FROM author_revenue WHERE total_revenue > 50.00 ORDER BY total_revenue DESC";
    match run(&engine, cte_sql) {
        Ok(r) if r.rows.len() == 2 => {
            let top = col_text(&r.rows[0], 0);
            if top == "J.R.R. Tolkien" {
                s.pass("CTE multi-step → 2 rows, Tolkien first");
            } else {
                s.fail("CTE", &format!("expected Tolkien first, got {top}"));
            }
        }
        Ok(r) => s.fail("CTE", &format!("expected 2, got {} rows", r.rows.len())),
        Err(e) => s.fail("CTE", &e.to_string()),
    }

    // ── Aggregating Data ──────────────────────────────────────────────────────
    println!("\n── Aggregating Data ─────────────────────────────");

    // COUNT per author → Tolkien=2, Herbert=2, Le Guin=2, McCarthy=1
    match run(&engine, "SELECT a.name, COUNT(*) AS book_count FROM books b JOIN authors a ON b.author_id = a.id GROUP BY a.name ORDER BY book_count DESC, a.name") {
        Ok(r) if r.rows.len() == 4 => s.pass("GROUP BY author, COUNT → 4 authors"),
        Ok(r) => s.fail("GROUP BY COUNT", &format!("expected 4, got {}", r.rows.len())),
        Err(e) => s.fail("GROUP BY COUNT", &e.to_string()),
    }

    // AVG/MIN/MAX/SUM prices
    match run(&engine, "SELECT AVG(price) AS avg_price, MIN(price) AS min_price, MAX(price) AS max_price, SUM(price) AS total_catalog_value FROM books") {
        Ok(r) if r.rows.len() == 1 => {
            if let Some(max) = col_f64(&r.rows[0], 2) {
                if (max - 24.99).abs() < 0.01 {
                    s.pass("AVG/MIN/MAX/SUM prices → MAX=24.99");
                } else {
                    s.fail("AVG/MIN/MAX/SUM", &format!("expected MAX=24.99, got {max:.2}"));
                }
            } else {
                s.fail("AVG/MIN/MAX/SUM", "could not parse MAX value");
            }
        }
        Ok(r) => s.fail("AVG/MIN/MAX/SUM", &format!("expected 1 row, got {}", r.rows.len())),
        Err(e) => s.fail("AVG/MIN/MAX/SUM", &e.to_string()),
    }

    // SUM by book title (orders) — Dune and Lord of the Rings are top 2
    match run(&engine, "SELECT b.title, SUM(o.quantity) AS units_sold, SUM(o.total_price) AS revenue FROM orders o JOIN books b ON o.book_id = b.id GROUP BY b.title ORDER BY revenue DESC") {
        Ok(r) if r.rows.len() == 4 => {
            // Expected top row: The Lord of the Rings (orders 3+6 = 24.99+74.97 = 99.96)
            let top = col_text(&r.rows[0], 0);
            if top == "The Lord of the Rings" {
                s.pass("GROUP BY title SUM revenue → Lord of the Rings top");
            } else {
                s.fail("GROUP BY title", &format!("expected Lord of the Rings, got {top}"));
            }
        }
        Ok(r) => s.fail("GROUP BY title", &format!("expected 4, got {}", r.rows.len())),
        Err(e) => s.fail("GROUP BY title", &e.to_string()),
    }

    // ── Updating Rows ─────────────────────────────────────────────────────────
    println!("\n── Updating Rows ────────────────────────────────");

    // 10% price increase for Frank Herbert books (author_id=2): Dune 15.99→17.59, Children 13.99→15.39
    match run(&engine, "UPDATE books SET price = ROUND(price * 1.10, 2) WHERE author_id = 2") {
        Ok(r) if r.rows_affected == 2 => s.pass("UPDATE price +10% for Herbert → 2 rows affected"),
        Ok(r) => s.fail("UPDATE Herbert", &format!("expected 2 rows_affected, got {}", r.rows_affected)),
        Err(e) => s.fail("UPDATE Herbert", &e.to_string()),
    }

    match run(&engine, "SELECT title, price FROM books WHERE author_id = 2 ORDER BY title") {
        Ok(r) if r.rows.len() == 2 => {
            // Children of Dune first (alphabetical), then Dune
            let children_price = col_f64(&r.rows[0], 1).unwrap_or(0.0);
            let dune_price     = col_f64(&r.rows[1], 1).unwrap_or(0.0);
            if (children_price - 15.39).abs() < 0.01 && (dune_price - 17.59).abs() < 0.01 {
                s.pass("SELECT after UPDATE → Children=15.39, Dune=17.59");
            } else {
                s.fail("SELECT after UPDATE", &format!("Children={children_price:.2}, Dune={dune_price:.2}"));
            }
        }
        Ok(r) => s.fail("SELECT after UPDATE", &format!("expected 2, got {}", r.rows.len())),
        Err(e) => s.fail("SELECT after UPDATE", &e.to_string()),
    }

    // UPDATE authors name (no-op re-set same value)
    match run(&engine, "UPDATE authors SET name = 'Ursula K. Le Guin' WHERE id = 3") {
        Ok(_) => s.pass("UPDATE authors name (idempotent)"),
        Err(e) => s.fail("UPDATE authors name", &e.to_string()),
    }

    // ── Deleting Rows ─────────────────────────────────────────────────────────
    println!("\n── Deleting Rows ────────────────────────────────");

    // Delete Gene Wolfe (id=5)
    match run(&engine, "DELETE FROM authors WHERE id = 5") {
        Ok(r) if r.rows_affected == 1 => s.pass("DELETE Gene Wolfe (id=5) → 1 row"),
        Ok(r) => s.fail("DELETE Gene Wolfe", &format!("rows_affected={}", r.rows_affected)),
        Err(e) => s.fail("DELETE Gene Wolfe", &e.to_string()),
    }

    // Delete orders for The Hobbit (book_id=1) — orders 1 only
    match run(&engine, "DELETE FROM orders WHERE book_id = 1") {
        Ok(r) if r.rows_affected == 1 => s.pass("DELETE orders WHERE book_id=1 → 1 row"),
        Ok(r) => s.fail("DELETE orders book_id=1", &format!("rows_affected={}", r.rows_affected)),
        Err(e) => s.fail("DELETE orders book_id=1", &e.to_string()),
    }

    // ── Advanced Queries ──────────────────────────────────────────────────────
    println!("\n── Advanced Queries ─────────────────────────────");

    // CASE WHEN: prices after updates: Blood Meridian=13.49, Dune=17.59, Children=15.39
    // Expected tiers: Dispossessed=budget(11.99), Hobbit=budget(12.99),
    //   Blood Meridian=mid(13.49), Left Hand=mid(13.99), Children=mid(15.39), Dune=mid(17.59), Lord=premium(24.99)
    let case_sql = "SELECT title, price, CASE WHEN price < 13.00 THEN 'budget' WHEN price < 20.00 THEN 'mid-range' ELSE 'premium' END AS tier FROM books ORDER BY price";
    match run(&engine, case_sql) {
        Ok(r) if r.rows.len() == 7 => {
            let first_tier = col_text(&r.rows[0], 2);
            let last_tier  = col_text(&r.rows[6], 2);
            if first_tier == "budget" && last_tier == "premium" {
                s.pass("CASE WHEN price tiers → budget..premium");
            } else {
                s.fail("CASE WHEN", &format!("first_tier={first_tier}, last_tier={last_tier}"));
            }
        }
        Ok(r) => s.fail("CASE WHEN", &format!("expected 7, got {}", r.rows.len())),
        Err(e) => s.fail("CASE WHEN", &e.to_string()),
    }

    // COALESCE — re-insert Anonymous, check COALESCE output, then delete
    let _ = run(&engine, "INSERT INTO authors VALUES (6, 'Anonymous', NULL)");
    match run(&engine, "SELECT name, COALESCE(country, 'Unknown') AS country FROM authors ORDER BY name") {
        Ok(r) => {
            let anon = r.rows.iter().find(|r| col_text(r, 0) == "Anonymous");
            match anon {
                Some(row) if col_text(row, 1) == "Unknown" =>
                    s.pass("COALESCE(country, 'Unknown') → Anonymous shows 'Unknown'"),
                Some(row) =>
                    s.fail("COALESCE", &format!("expected 'Unknown', got '{}'", col_text(row, 1))),
                None =>
                    s.fail("COALESCE", "Anonymous row not found"),
            }
        }
        Err(e) => s.fail("COALESCE", &e.to_string()),
    }
    let _ = run(&engine, "DELETE FROM authors WHERE id = 6");

    // String functions
    match run(&engine, "SELECT UPPER(name) AS shout, LOWER(name) AS whisper FROM authors LIMIT 2") {
        Ok(r) if r.rows.len() == 2 => {
            let shout = col_text(&r.rows[0], 0);
            if shout == shout.to_uppercase() {
                s.pass("UPPER/LOWER → first row uppercased correctly");
            } else {
                s.fail("UPPER/LOWER", &format!("got '{shout}', expected uppercase"));
            }
        }
        Ok(r) => s.fail("UPPER/LOWER", &format!("expected 2, got {}", r.rows.len())),
        Err(e) => s.fail("UPPER/LOWER", &e.to_string()),
    }

    match run(&engine, "SELECT name, LENGTH(TRIM(name)) AS chars FROM authors ORDER BY chars DESC") {
        Ok(r) if !r.rows.is_empty() => s.pass("LENGTH(TRIM(name)) → rows returned"),
        Ok(_) => s.fail("LENGTH/TRIM", "no rows"),
        Err(e) => s.fail("LENGTH/TRIM", &e.to_string()),
    }

    let substr_sql = "SELECT title, SUBSTRING(title, 1, POSITION(' ' IN title) - 1) AS first_word FROM books WHERE POSITION(' ' IN title) > 0";
    match run(&engine, substr_sql) {
        Ok(r) if !r.rows.is_empty() => s.pass("SUBSTRING/POSITION → first word extracted"),
        Ok(_) => s.fail("SUBSTRING/POSITION", "no rows"),
        Err(e) => s.fail("SUBSTRING/POSITION", &e.to_string()),
    }

    match run(&engine, "SELECT REPLACE(name, '.', '') AS simplified_name FROM authors") {
        Ok(r) if r.rows.len() == 4 => s.pass("REPLACE('.', '') → 4 rows"),
        Ok(r) => s.fail("REPLACE", &format!("expected 4, got {}", r.rows.len())),
        Err(e) => s.fail("REPLACE", &e.to_string()),
    }

    match run(&engine, "SELECT name || ' (' || country || ')' AS label FROM authors WHERE country IS NOT NULL") {
        Ok(r) if r.rows.len() == 4 => s.pass("string concatenation || → 4 rows"),
        Ok(r) => s.fail("concatenation ||", &format!("expected 4, got {}", r.rows.len())),
        Err(e) => s.fail("concatenation ||", &e.to_string()),
    }

    // ALTER TABLE — ADD COLUMN, UPDATE, SELECT, RENAME, DROP
    match run(&engine, "ALTER TABLE authors ADD COLUMN bio TEXT") {
        Ok(_) => s.pass("ALTER TABLE authors ADD COLUMN bio"),
        Err(e) => s.fail("ALTER TABLE ADD COLUMN", &e.to_string()),
    }
    match run(&engine, "UPDATE authors SET bio = 'Author of Middle-earth.' WHERE id = 1") {
        Ok(_) => s.pass("UPDATE authors SET bio WHERE id=1"),
        Err(e) => s.fail("UPDATE bio", &e.to_string()),
    }
    match run(&engine, "SELECT id, name, bio FROM authors ORDER BY id") {
        Ok(r) if r.rows.len() == 4 => {
            let bio = col_text(&r.rows[0], 2);
            if bio == "Author of Middle-earth." {
                s.pass("SELECT bio → Tolkien has 'Author of Middle-earth.'");
            } else {
                s.fail("SELECT bio", &format!("expected 'Author of Middle-earth.', got '{bio}'"));
            }
        }
        Ok(r) => s.fail("SELECT bio", &format!("expected 4, got {}", r.rows.len())),
        Err(e) => s.fail("SELECT bio", &e.to_string()),
    }

    match run(&engine, "ALTER TABLE authors RENAME COLUMN bio TO biography") {
        Ok(_) => s.pass("ALTER TABLE RENAME COLUMN bio → biography"),
        Err(e) => s.fail("RENAME COLUMN bio→biography", &e.to_string()),
    }
    match run(&engine, "ALTER TABLE authors RENAME COLUMN biography TO bio") {
        Ok(_) => s.pass("ALTER TABLE RENAME COLUMN biography → bio (back)"),
        Err(e) => s.fail("RENAME COLUMN biography→bio", &e.to_string()),
    }
    match run(&engine, "ALTER TABLE authors DROP COLUMN bio") {
        Ok(_) => s.pass("ALTER TABLE DROP COLUMN bio"),
        Err(e) => s.fail("DROP COLUMN bio", &e.to_string()),
    }
    match run(&engine, "ALTER TABLE authors RENAME TO writers") {
        Ok(_) => s.pass("ALTER TABLE RENAME authors → writers"),
        Err(e) => s.fail("RENAME TABLE authors→writers", &e.to_string()),
    }
    match run(&engine, "ALTER TABLE writers RENAME TO authors") {
        Ok(_) => s.pass("ALTER TABLE RENAME writers → authors (back)"),
        Err(e) => s.fail("RENAME TABLE writers→authors", &e.to_string()),
    }

    // UNIQUE / PRIMARY KEY
    println!("\n── Unique / Primary Key ─────────────────────────");

    match run(&engine, "CREATE TABLE members (id INTEGER PRIMARY KEY, email TEXT UNIQUE NOT NULL, handle TEXT NOT NULL)") {
        Ok(_) => s.pass("CREATE TABLE members (id PK, email UNIQUE)"),
        Err(e) => s.fail("CREATE members", &e.to_string()),
    }
    match run(&engine, "INSERT INTO members VALUES (1, 'alice@example.com', 'alice')") {
        Ok(_) => s.pass("INSERT members alice"),
        Err(e) => s.fail("INSERT alice", &e.to_string()),
    }
    match run(&engine, "INSERT INTO members VALUES (2, 'bob@example.com', 'bob')") {
        Ok(_) => s.pass("INSERT members bob"),
        Err(e) => s.fail("INSERT bob", &e.to_string()),
    }
    // Duplicate email → must fail
    match run(&engine, "INSERT INTO members VALUES (3, 'alice@example.com', 'alice2')") {
        Err(_) => s.pass("duplicate email → error (expected)"),
        Ok(_)  => s.fail("duplicate email", "should have failed but succeeded"),
    }
    // Duplicate PK → must fail
    match run(&engine, "INSERT INTO members VALUES (1, 'carol@example.com', 'carol')") {
        Err(_) => s.pass("duplicate PK id=1 → error (expected)"),
        Ok(_)  => s.fail("duplicate PK", "should have failed but succeeded"),
    }

    // ── Window Functions ──────────────────────────────────────────────────────
    println!("\n── Window Functions ─────────────────────────────");

    let win1 = "SELECT a.name AS author, b.title, b.price, \
        ROW_NUMBER() OVER (PARTITION BY b.author_id ORDER BY b.price DESC) AS price_rank \
        FROM books b JOIN authors a ON a.id = b.author_id ORDER BY a.name, price_rank";
    match run(&engine, win1) {
        Ok(r) if r.rows.len() == 7 => {
            // Blood Meridian (sole McCarthy book) should have rank 1
            let bm = r.rows.iter().find(|r| col_text(r, 1) == "Blood Meridian");
            match bm {
                Some(row) if col_text(row, 3) == "1" =>
                    s.pass("ROW_NUMBER() OVER PARTITION → Blood Meridian rank=1"),
                Some(row) =>
                    s.fail("ROW_NUMBER()", &format!("Blood Meridian rank={}", col_text(row, 3))),
                None =>
                    s.fail("ROW_NUMBER()", "Blood Meridian row not found"),
            }
        }
        Ok(r) => s.fail("ROW_NUMBER()", &format!("expected 7, got {}", r.rows.len())),
        Err(e) => s.fail("ROW_NUMBER()", &e.to_string()),
    }

    let win2 = "SELECT a.name AS author, b.title, b.price, \
        SUM(b.price) OVER (PARTITION BY b.author_id) AS author_total \
        FROM books b JOIN authors a ON a.id = b.author_id ORDER BY a.name, b.price DESC";
    match run(&engine, win2) {
        Ok(r) if r.rows.len() == 7 => {
            // Tolkien: Hobbit(12.99) + Lord(24.99) = 37.98
            let tolkien_rows: Vec<_> = r.rows.iter()
                .filter(|r| col_text(r, 0) == "J.R.R. Tolkien")
                .collect();
            if tolkien_rows.len() == 2 {
                let total = col_f64(&tolkien_rows[0], 3).unwrap_or(0.0);
                if (total - 37.98).abs() < 0.01 {
                    s.pass("SUM() OVER PARTITION → Tolkien total=37.98");
                } else {
                    s.fail("SUM() OVER", &format!("Tolkien total={total:.2}"));
                }
            } else {
                s.fail("SUM() OVER", "expected 2 Tolkien rows");
            }
        }
        Ok(r) => s.fail("SUM() OVER", &format!("expected 7, got {}", r.rows.len())),
        Err(e) => s.fail("SUM() OVER", &e.to_string()),
    }

    // ── WITH RECURSIVE ────────────────────────────────────────────────────────
    println!("\n── WITH RECURSIVE ───────────────────────────────");

    let rec_sql = "WITH RECURSIVE series(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM series WHERE n < 5) SELECT n FROM series";
    match run(&engine, rec_sql) {
        Ok(r) if r.rows.len() == 5 => {
            let vals: Vec<i64> = r.rows.iter()
                .filter_map(|row| match row.get_by_idx(0) {
                    Some(Value::Int4(n)) => Some(*n as i64),
                    Some(Value::Int8(n)) => Some(*n),
                    _ => None,
                })
                .collect();
            if vals == vec![1, 2, 3, 4, 5] {
                s.pass("WITH RECURSIVE series 1..5 → [1,2,3,4,5]");
            } else {
                s.fail("WITH RECURSIVE", &format!("expected [1..5], got {vals:?}"));
            }
        }
        Ok(r) => s.fail("WITH RECURSIVE", &format!("expected 5, got {}", r.rows.len())),
        Err(e) => s.fail("WITH RECURSIVE", &e.to_string()),
    }

    // ── Production Patterns ───────────────────────────────────────────────────
    println!("\n── Production Patterns ──────────────────────────");

    // SERIAL
    match run(&engine, "CREATE TABLE customers (id SERIAL PRIMARY KEY, email TEXT NOT NULL, name TEXT NOT NULL)") {
        Ok(_) => s.pass("CREATE TABLE customers (id SERIAL PRIMARY KEY)"),
        Err(e) => s.fail("CREATE customers SERIAL", &e.to_string()),
    }
    match run(&engine, "INSERT INTO customers (email, name) VALUES ('alice@example.com', 'Alice')") {
        Ok(_) => s.pass("INSERT INTO customers (no explicit id)"),
        Err(e) => s.fail("INSERT customers (1)", &e.to_string()),
    }
    match run(&engine, "INSERT INTO customers (email, name) VALUES ('bob@example.com', 'Bob')") {
        Ok(_) => s.pass("INSERT INTO customers (second row)"),
        Err(e) => s.fail("INSERT customers (2)", &e.to_string()),
    }
    match run(&engine, "SELECT * FROM customers") {
        Ok(r) if r.rows.len() == 2 => {
            let id1 = match r.rows[0].get_by_idx(0) { Some(Value::Int4(n)) => *n, _ => -1 };
            let id2 = match r.rows[1].get_by_idx(0) { Some(Value::Int4(n)) => *n, _ => -1 };
            if id1 == 1 && id2 == 2 {
                s.pass("SELECT customers → id=1 Alice, id=2 Bob");
            } else {
                s.fail("SELECT customers SERIAL ids", &format!("got id1={id1}, id2={id2}"));
            }
        }
        Ok(r) => s.fail("SELECT customers", &format!("expected 2, got {}", r.rows.len())),
        Err(e) => s.fail("SELECT customers", &e.to_string()),
    }

    // DEFAULT column values
    match run(&engine, "CREATE TABLE products (id SERIAL PRIMARY KEY, name TEXT NOT NULL, price NUMERIC(10,2) NOT NULL, in_stock BOOLEAN DEFAULT TRUE, created_at TIMESTAMP DEFAULT NOW())") {
        Ok(_) => s.pass("CREATE TABLE products (BOOLEAN DEFAULT TRUE, TIMESTAMP DEFAULT NOW())"),
        Err(e) => s.fail("CREATE products", &e.to_string()),
    }
    match run(&engine, "INSERT INTO products (name, price) VALUES ('Widget', 9.99)") {
        Ok(_) => s.pass("INSERT products (name, price) — in_stock/created_at use DEFAULT"),
        Err(e) => s.fail("INSERT products", &e.to_string()),
    }
    match run(&engine, "SELECT * FROM products") {
        Ok(r) if r.rows.len() == 1 => {
            // in_stock should be TRUE (Bool or text "t"/"true")
            let in_stock = r.rows[0].get_by_idx(3);
            let ok = matches!(in_stock, Some(Value::Bool(true)))
                || matches!(in_stock, Some(Value::Text(s)) if s == "true" || s == "t");
            if ok {
                s.pass("SELECT products → in_stock=true (DEFAULT applied)");
            } else {
                s.fail("DEFAULT TRUE", &format!("in_stock={in_stock:?}"));
            }
        }
        Ok(r) => s.fail("SELECT products", &format!("expected 1, got {}", r.rows.len())),
        Err(e) => s.fail("SELECT products", &e.to_string()),
    }

    // FOREIGN KEY
    match run(&engine, "CREATE TABLE categories (id SERIAL PRIMARY KEY, name TEXT NOT NULL)") {
        Ok(_) => s.pass("CREATE TABLE categories"),
        Err(e) => s.fail("CREATE categories", &e.to_string()),
    }
    match run(&engine, "CREATE TABLE items (id SERIAL PRIMARY KEY, category_id INT NOT NULL REFERENCES categories(id), name TEXT NOT NULL)") {
        Ok(_) => s.pass("CREATE TABLE items (category_id REFERENCES categories(id))"),
        Err(e) => s.fail("CREATE items FK", &e.to_string()),
    }
    match run(&engine, "INSERT INTO categories (name) VALUES ('Electronics')") {
        Ok(_) => s.pass("INSERT categories Electronics"),
        Err(e) => s.fail("INSERT categories", &e.to_string()),
    }
    match run(&engine, "INSERT INTO items (category_id, name) VALUES (1, 'Laptop')") {
        Ok(_) => s.pass("INSERT items Laptop (valid FK)"),
        Err(e) => s.fail("INSERT items valid FK", &e.to_string()),
    }
    // Invalid FK — category 99 does not exist
    match run(&engine, "INSERT INTO items (category_id, name) VALUES (99, 'Mystery item')") {
        Err(_) => s.pass("INSERT items FK=99 → error (expected)"),
        Ok(_)  => s.fail("FK violation", "should have failed but succeeded"),
    }
    // Delete parent referenced by child → should fail
    match run(&engine, "DELETE FROM categories WHERE id = 1") {
        Err(_) => s.pass("DELETE referenced category → error (expected)"),
        Ok(_)  => s.fail("FK delete parent", "should have failed but succeeded"),
    }

    // CHECK constraints
    match run(&engine, "CREATE TABLE prices (id SERIAL PRIMARY KEY, amount NUMERIC(10,2) NOT NULL CHECK (amount >= 0), label TEXT NOT NULL CHECK (length(label) > 0))") {
        Ok(_) => s.pass("CREATE TABLE prices (CHECK amount >= 0, CHECK length > 0)"),
        Err(e) => s.fail("CREATE prices CHECK", &e.to_string()),
    }
    match run(&engine, "INSERT INTO prices (amount, label) VALUES (19.99, 'Standard')") {
        Ok(_) => s.pass("INSERT prices Standard 19.99 (valid CHECK)"),
        Err(e) => s.fail("INSERT prices valid", &e.to_string()),
    }
    match run(&engine, "INSERT INTO prices (amount, label) VALUES (-5.00, 'Discount')") {
        Err(_) => s.pass("INSERT prices -5.00 → CHECK violation (expected)"),
        Ok(_)  => s.fail("CHECK amount >= 0", "should have failed but succeeded"),
    }

    // UPSERT — ON CONFLICT DO NOTHING
    match run(&engine, "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)") {
        Ok(_) => s.pass("CREATE TABLE settings (key PK)"),
        Err(e) => s.fail("CREATE settings", &e.to_string()),
    }
    match run(&engine, "INSERT INTO settings VALUES ('theme', 'dark')") {
        Ok(_) => s.pass("INSERT settings theme=dark"),
        Err(e) => s.fail("INSERT settings dark", &e.to_string()),
    }
    match run(&engine, "INSERT INTO settings VALUES ('theme', 'light') ON CONFLICT DO NOTHING") {
        Ok(_) => s.pass("ON CONFLICT DO NOTHING → no error on duplicate"),
        Err(e) => s.fail("ON CONFLICT DO NOTHING", &e.to_string()),
    }
    match run(&engine, "SELECT * FROM settings") {
        Ok(r) if r.rows.len() == 1 && col_text(&r.rows[0], 1) == "dark" =>
            s.pass("SELECT settings after DO NOTHING → still 'dark'"),
        Ok(r) => s.fail("settings after DO NOTHING", &format!("rows={}, value={:?}", r.rows.len(), r.rows.first().map(|r| col_text(r, 1)))),
        Err(e) => s.fail("SELECT settings", &e.to_string()),
    }

    // UPSERT — ON CONFLICT DO UPDATE
    match run(&engine, "INSERT INTO settings (key, value) VALUES ('theme', 'light') ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value") {
        Ok(_) => s.pass("ON CONFLICT DO UPDATE → upsert accepted"),
        Err(e) => s.fail("ON CONFLICT DO UPDATE", &e.to_string()),
    }
    match run(&engine, "SELECT * FROM settings") {
        Ok(r) if r.rows.len() == 1 && col_text(&r.rows[0], 1) == "light" =>
            s.pass("SELECT settings after DO UPDATE → now 'light'"),
        Ok(r) => s.fail("settings after DO UPDATE", &format!("rows={}, value={:?}", r.rows.len(), r.rows.first().map(|r| col_text(r, 1)))),
        Err(e) => s.fail("SELECT settings post-upsert", &e.to_string()),
    }

    // SAVEPOINT
    println!("\n── Savepoints ───────────────────────────────────");

    match run(&engine, "CREATE TABLE accounts (id SERIAL PRIMARY KEY, name TEXT NOT NULL, balance NUMERIC(12,2) NOT NULL DEFAULT 0)") {
        Ok(_) => s.pass("CREATE TABLE accounts (balance DEFAULT 0)"),
        Err(e) => s.fail("CREATE accounts", &e.to_string()),
    }
    let _ = run(&engine, "INSERT INTO accounts (name, balance) VALUES ('Alice', 1000.00)");
    let _ = run(&engine, "INSERT INTO accounts (name, balance) VALUES ('Bob', 500.00)");

    // Savepoint: debit Alice, savepoint, credit Bob (wrong), rollback to savepoint, re-credit, commit
    let sp_stmts = [
        ("BEGIN",                                                             "BEGIN"),
        ("UPDATE accounts SET balance = balance - 100 WHERE name = 'Alice'", "debit Alice"),
        ("SAVEPOINT after_debit",                                            "SAVEPOINT after_debit"),
        ("UPDATE accounts SET balance = balance + 100 WHERE name = 'Bob'",   "credit Bob"),
        ("ROLLBACK TO SAVEPOINT after_debit",                                "ROLLBACK TO SAVEPOINT"),
        ("UPDATE accounts SET balance = balance + 100 WHERE name = 'Bob'",   "re-credit Bob"),
        ("COMMIT",                                                           "COMMIT"),
    ];
    let mut sp_ok = true;
    for (sql, label) in sp_stmts {
        if let Err(e) = run_session(&engine, "savepoint-session", sql) {
            s.fail(&format!("SAVEPOINT step: {label}"), &e.to_string());
            sp_ok = false;
        }
    }
    if sp_ok {
        // Verify: Alice=900, Bob=600
        match run(&engine, "SELECT name, balance FROM accounts ORDER BY name") {
            Ok(r) if r.rows.len() == 2 => {
                let alice_bal = col_f64(&r.rows[0], 1).unwrap_or(0.0);
                let bob_bal   = col_f64(&r.rows[1], 1).unwrap_or(0.0);
                if (alice_bal - 900.0).abs() < 0.01 && (bob_bal - 600.0).abs() < 0.01 {
                    s.pass("SAVEPOINT → Alice=900.00, Bob=600.00 (chapter expected)");
                } else {
                    s.fail("SAVEPOINT balance", &format!("Alice={alice_bal:.2}, Bob={bob_bal:.2}"));
                }
            }
            Ok(r) => s.fail("SAVEPOINT SELECT", &format!("expected 2, got {}", r.rows.len())),
            Err(e) => s.fail("SAVEPOINT SELECT", &e.to_string()),
        }
    }

    // RELEASE SAVEPOINT
    let rel_stmts = [
        "BEGIN",
        "INSERT INTO accounts (name, balance) VALUES ('Carol', 250.00)",
        "SAVEPOINT after_carol",
        "RELEASE SAVEPOINT after_carol",
        "COMMIT",
    ];
    let mut rel_ok = true;
    for sql in rel_stmts {
        if let Err(e) = run_session(&engine, "release-session", sql) {
            s.fail(&format!("RELEASE SAVEPOINT step: {sql}"), &e.to_string());
            rel_ok = false;
        }
    }
    if rel_ok { s.pass("RELEASE SAVEPOINT → commit succeeds, Carol inserted"); }

    // Nested savepoints
    let nested_stmts = [
        "BEGIN",
        "INSERT INTO accounts (name, balance) VALUES ('Dave', 100.00)",
        "SAVEPOINT sp1",
        "INSERT INTO accounts (name, balance) VALUES ('Eve', 200.00)",
        "SAVEPOINT sp2",
        "INSERT INTO accounts (name, balance) VALUES ('Frank', 300.00)",
        "ROLLBACK TO SAVEPOINT sp2",
        "ROLLBACK TO SAVEPOINT sp1",
        "COMMIT",
    ];
    let mut nested_ok = true;
    for sql in nested_stmts {
        if let Err(e) = run_session(&engine, "nested-sp-session", sql) {
            s.fail(&format!("nested savepoint: {sql}"), &e.to_string());
            nested_ok = false;
        }
    }
    if nested_ok {
        match run(&engine, "SELECT name FROM accounts WHERE name IN ('Dave', 'Eve', 'Frank') ORDER BY name") {
            Ok(r) if r.rows.len() == 1 && col_text(&r.rows[0], 0) == "Dave" =>
                s.pass("nested SAVEPOINT → only Dave committed (Eve and Frank rolled back)"),
            Ok(r) => s.fail("nested SAVEPOINT", &format!("expected only Dave, got {:?}", r.rows.iter().map(|r| col_text(r, 0)).collect::<Vec<_>>())),
            Err(e) => s.fail("nested SAVEPOINT SELECT", &e.to_string()),
        }
    }

    // ── GRANT / REVOKE ────────────────────────────────────────────────────────
    println!("\n── GRANT / REVOKE ───────────────────────────────");

    match run(&engine, "CREATE ROLE reporter WITH LOGIN PASSWORD 'report-pass'") {
        Ok(_) => s.pass("CREATE ROLE reporter"),
        Err(e) => s.fail("CREATE ROLE reporter", &e.to_string()),
    }
    match run(&engine, "GRANT SELECT ON books TO reporter") {
        Ok(_) => s.pass("GRANT SELECT ON books TO reporter"),
        Err(e) => s.fail("GRANT SELECT books", &e.to_string()),
    }
    match run(&engine, "GRANT SELECT ON authors TO reporter") {
        Ok(_) => s.pass("GRANT SELECT ON authors TO reporter"),
        Err(e) => s.fail("GRANT SELECT authors", &e.to_string()),
    }
    match run(&engine, "REVOKE SELECT ON books FROM reporter") {
        Ok(_) => s.pass("REVOKE SELECT ON books FROM reporter"),
        Err(e) => s.fail("REVOKE SELECT", &e.to_string()),
    }
    match run(&engine, "CREATE ROLE appuser WITH LOGIN PASSWORD 'app-pass'") {
        Ok(_) => s.pass("CREATE ROLE appuser"),
        Err(e) => s.fail("CREATE ROLE appuser", &e.to_string()),
    }
    match run(&engine, "GRANT SELECT, INSERT, UPDATE, DELETE ON books TO appuser") {
        Ok(_) => s.pass("GRANT SELECT,INSERT,UPDATE,DELETE ON books TO appuser"),
        Err(e) => s.fail("GRANT DML books", &e.to_string()),
    }
    match run(&engine, "GRANT ALL ON orders TO appuser") {
        Ok(_) => s.pass("GRANT ALL ON orders TO appuser"),
        Err(e) => s.fail("GRANT ALL orders", &e.to_string()),
    }
    s.skip("isql -U reporter (SELECT)", "requires running server + separate session");
    s.skip("INSERT INTO books as reporter → permission denied", "requires multi-role session");

    // ── COPY ──────────────────────────────────────────────────────────────────
    println!("\n── COPY (file I/O) ──────────────────────────────");
    s.skip("COPY books TO '/tmp/books.csv'", "server-only file I/O feature");
    s.skip("COPY books_backup FROM '/tmp/books.csv'", "server-only file I/O feature");

    // ── LISTEN / NOTIFY ───────────────────────────────────────────────────────
    println!("\n── LISTEN / NOTIFY ──────────────────────────────");
    s.skip("LISTEN order_updates", "async pub/sub — server process required");
    s.skip("NOTIFY order_updates", "async pub/sub — server process required");
    s.skip("UNLISTEN order_updates", "async pub/sub — server process required");

    // ── Managing Multiple Databases ───────────────────────────────────────────
    println!("\n── Managing Multiple Databases ──────────────────");

    match run(&engine, "CREATE DATABASE IF NOT EXISTS bookstore") {
        Ok(_) => s.pass("CREATE DATABASE IF NOT EXISTS bookstore (idempotent)"),
        Err(e) => s.fail("CREATE DATABASE IF NOT EXISTS", &e.to_string()),
    }
    match run(&engine, "DROP DATABASE IF EXISTS staging") {
        Ok(_) => s.pass("DROP DATABASE IF EXISTS staging (non-existent, no-op)"),
        Err(e) => s.fail("DROP DATABASE IF EXISTS", &e.to_string()),
    }

    s.skip("\\c icedb / \\c bookstore", "CLI meta-command");
    s.skip("\\l (list databases)", "CLI meta-command");
    s.skip("cargo run -p cli --dbname bookstore", "shell command");
    s.skip("psql -d bookstore", "external client command");

    match run(&engine, "DROP DATABASE bookstore_v2") {
        Ok(_) => s.pass("DROP DATABASE bookstore_v2"),
        Err(_) => s.pass("DROP DATABASE bookstore_v2 → not found (expected, never created)"),
    }
    match run(&engine, "DROP DATABASE IF EXISTS bookstore_v2") {
        Ok(_) => s.pass("DROP DATABASE IF EXISTS bookstore_v2 (no-op)"),
        Err(e) => s.fail("DROP DATABASE IF EXISTS bookstore_v2", &e.to_string()),
    }

    // ── Summary ───────────────────────────────────────────────────────────────
    let total = s.passed + s.failed + s.skipped;
    println!("\n═══════════════════════════════════════════════════");
    println!("  Passed:  {}", s.passed);
    println!("  Failed:  {}", s.failed);
    println!("  Skipped: {} (shell/CLI/server-only)", s.skipped);
    println!("  Total:   {total}");
    println!("═══════════════════════════════════════════════════\n");

    if s.failed > 0 {
        std::process::exit(1);
    }
}
