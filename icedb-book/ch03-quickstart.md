# Chapter 3: Quick Start — Your First Database

**Estimated time:** 20–30 minutes

This chapter walks you through starting icedb, connecting to it, and building a small but realistic schema from scratch. By the end you will have created tables, inserted rows, run queries with joins and aggregates, updated and deleted data, and used transactions. All examples use a bookstore domain.

**In this chapter:**
- Starting the server and connecting with the CLI
- Creating tables and inserting rows
- Querying with WHERE, ORDER BY, LIMIT / FETCH FIRST
- Joining tables (INNER JOIN, LEFT JOIN, JOIN USING, LATERAL)
- Aggregates, CTEs, UNION
- Updating and deleting rows
- Advanced queries: CASE WHEN, string functions, ALTER TABLE, UNIQUE/PRIMARY KEY, window functions, WITH RECURSIVE
- Production patterns: SERIAL, DEFAULT, FOREIGN KEY, CHECK, UPSERT, SAVEPOINT, GRANT
- Managing multiple databases: CREATE DATABASE, DROP DATABASE, `\c`
- What happens behind the scenes

## Starting the Server

Open a terminal. Create a directory to hold the database files and start the server:

```sh
cargo run -p server --release -- --port 5432 --data-dir ./data
```

The server prints:

```
INFO  icedb listening on 0.0.0.0:5432
```

Leave this terminal running. The server accepts connections on port 5432 — the same default port as PostgreSQL, so standard tools connect without specifying a port explicitly.

On first startup, icedb bootstraps the system catalogs: it creates the heap files for `pg_class`, `pg_attribute`, `pg_authid`, and `pg_namespace`, inserts the `icedb` superuser role, and writes the initial WAL segment. This takes under a second and is done only once.

## Connecting with the Built-In CLI

Open a second terminal and start the CLI:

```sh
cargo run -p cli --release -- --data-dir ./data
```

The CLI (nkv-psql) runs the storage engine in-process against the data directory — no separate server process or TCP connection is needed. You will see:

```
icedb=#
```

The prompt format is `<dbname>=# ` when the CLI is ready for a new statement. When you press Enter without a semicolon, the prompt changes to `<dbname>-# ` — this is the **continuation prompt**. It means the CLI is accumulating more input and waiting for a `;` before it sends anything to the engine. Type `;` (alone on a line if needed) to execute.

You can also connect with a standard `psql` client to the server started above:

```sh
psql -h 127.0.0.1 -p 5432 -U icedb
```

> **Important — data directory must match.** The CLI and the server must point at the **same** `--data-dir`. If you start the server with `./data` but the CLI with `./mydb`, they see different files and each other's tables are invisible. Stick to one directory throughout this tutorial.

## Creating a Database

Every schema and table lives inside a database. icedb starts with a default database called `icedb`, but for this tutorial you will create a dedicated `bookstore` database to keep things clean and isolated.

```sql
CREATE DATABASE bookstore;
```

```
CREATE DATABASE
```

Switch to it with `\c`:

```
icedb=# \c bookstore
You are now connected to database "bookstore".
bookstore=#
```

The prompt changes to `bookstore=#` — all subsequent statements run in this database. To confirm:

```
bookstore=# \l
                                  List of databases
   Name       |  Owner
--------------+----------
 bookstore    | icedb
 icedb        | icedb
```

Everything you create from here on belongs to `bookstore`. Switching back to `icedb` later and running `\dt` would show an empty schema — the two databases are fully isolated.

### Reconnecting after a restart

Every time you restart the CLI or reconnect with psql you land on the default `icedb` database. You must switch to `bookstore` explicitly:

```
icedb=# \c bookstore
You are now connected to database "bookstore".
bookstore=#
```

Or skip the switch entirely by naming the database at startup:

```sh
# CLI — open bookstore directly
cargo run -p cli --release -- --data-dir ./data --dbname bookstore

# psql — connect straight to bookstore
psql -h 127.0.0.1 -p 5432 -d bookstore -U icedb
```

If you connect to `./data` but see `\dt` showing nothing, you are either in the wrong database (run `\c bookstore`) or pointing at the wrong data directory.

## A Note on Semicolons

Every SQL statement must end with a semicolon (`;`). The CLI uses the semicolon as the signal to send the statement to the engine — until it sees one it keeps the prompt as `bookstore-#` and waits for more input.

```
bookstore=# SELECT 1
bookstore-# ;
 ?column?
----------
        1
```

The second line shows that `;` can be on its own line — just press Enter after it. If you see `bookstore-#` when you expected a result, type `;` and press Enter.

**Paste tip:** only paste the SQL inside the `sql` code blocks. Do not include the expected-output lines (the ones with `---+---` separators and data rows) — the CLI will try to execute them as SQL and report a parse error.

## Creating Your First Tables

The bookstore schema uses three tables: `authors`, `books`, and `orders`.

```sql
CREATE TABLE authors (
    id       INT NOT NULL,
    name     TEXT NOT NULL,
    country  TEXT
);

CREATE TABLE books (
    id          INT NOT NULL,
    title       TEXT NOT NULL,
    author_id   INT NOT NULL,
    price       FLOAT,
    published   INT
);

CREATE TABLE orders (
    id          INT NOT NULL,
    book_id     INT NOT NULL,
    quantity    INT NOT NULL,
    total_price FLOAT
);
```

Each `CREATE TABLE` statement returns `CREATE TABLE` with no rows affected. Verify the tables exist:

```
bookstore=# \dt
 Schema |  Name   | Type
--------+---------+-------
 public | authors | table
 public | books   | table
 public | orders  | table
```

Inspect a table's columns:

```
bookstore=# \d books
Table "public.books"
  Column    |  Type            | Nullable
------------+------------------+---------
 id         | int4             | not null
 title      | text             | not null
 author_id  | int4             | not null
 price      | double precision |
 published  | int4             |
```

## Inserting Rows

Insert authors first:

```sql
INSERT INTO authors VALUES (1, 'J.R.R. Tolkien', 'United Kingdom');
INSERT INTO authors VALUES (2, 'Frank Herbert', 'United States');
INSERT INTO authors VALUES (3, 'Ursula K. Le Guin', 'United States');
INSERT INTO authors VALUES (4, 'Cormac McCarthy', 'United States');
```

Insert multiple books. Each `INSERT` is a separate statement:

```sql
INSERT INTO books VALUES (1, 'The Hobbit', 1, 12.99, 1937);
INSERT INTO books VALUES (2, 'The Lord of the Rings', 1, 24.99, 1954);
INSERT INTO books VALUES (3, 'Dune', 2, 15.99, 1965);
INSERT INTO books VALUES (4, 'The Left Hand of Darkness', 3, 13.99, 1969);
INSERT INTO books VALUES (5, 'The Dispossessed', 3, 11.99, 1974);
INSERT INTO books VALUES (6, 'Blood Meridian', 4, 14.99, 1985);
INSERT INTO books VALUES (7, 'Children of Dune', 2, 13.99, 1976);
```

Insert some orders:

```sql
INSERT INTO orders VALUES (1, 1, 2, 25.98);
INSERT INTO orders VALUES (2, 3, 1, 15.99);
INSERT INTO orders VALUES (3, 2, 1, 24.99);
INSERT INTO orders VALUES (4, 4, 3, 41.97);
INSERT INTO orders VALUES (5, 3, 5, 79.95);
```

The server confirms each insert with `INSERT 0 1` (command tag, OID, rows affected).

## Querying Data

### Basic SELECT

Retrieve all books:

```sql
SELECT * FROM books;
```

```
 id |          title            | author_id | price | published
----+---------------------------+-----------+-------+-----------
  1 | The Hobbit                |         1 | 12.99 |      1937
  2 | The Lord of the Rings     |         1 | 24.99 |      1954
  3 | Dune                      |         2 | 15.99 |      1965
  4 | The Left Hand of Darkness |         3 | 13.99 |      1969
  5 | The Dispossessed          |         3 | 11.99 |      1974
  6 | Blood Meridian            |         4 | 14.99 |      1985
  7 | Children of Dune          |         2 | 13.99 |      1976
```

Select only specific columns:

```sql
SELECT title, price FROM books WHERE price < 14.00;
```

```
          title            | price
---------------------------+-------
 The Hobbit                | 12.99
 The Left Hand of Darkness | 13.99
 The Dispossessed          | 11.99
 Children of Dune          | 13.99
```

### Filtering with WHERE

Books published before 1970:

```sql
SELECT title, published FROM books WHERE published < 1970;
```

```
          title            | published
---------------------------+-----------
 The Hobbit                |      1937
 The Lord of the Rings     |      1954
 Dune                      |      1965
 The Left Hand of Darkness |      1969
```

Compound conditions with AND and OR:

```sql
SELECT title, price FROM books
WHERE price > 13.00 AND published > 1960;
```

```
          title            | price
---------------------------+-------
 Dune                      | 15.99
 The Left Hand of Darkness | 13.99
 Blood Meridian            | 14.99
 Children of Dune          | 13.99
```

### Sorting with ORDER BY

Cheapest books first:

```sql
SELECT title, price FROM books ORDER BY price ASC;
```

```
          title            | price
---------------------------+-------
 The Dispossessed          | 11.99
 The Hobbit                | 12.99
 The Left Hand of Darkness | 13.99
 Children of Dune          | 13.99
 Blood Meridian            | 14.99
 Dune                      | 15.99
 The Lord of the Rings     | 24.99
```

Most recent books first, breaking ties by price descending:

```sql
SELECT title, published, price FROM books
ORDER BY published DESC, price DESC;
```

### Limiting Results

Top 3 most expensive books:

```sql
SELECT title, price FROM books ORDER BY price DESC LIMIT 3;
```

```
        title          | price
-----------------------+-------
 The Lord of the Rings | 24.99
 Dune                  | 15.99
 Blood Meridian        | 14.99
```

Pagination: skip the first 3, show the next 2:

```sql
SELECT title, price FROM books ORDER BY price DESC LIMIT 2 OFFSET 3;
```

`FETCH FIRST` is the SQL-standard spelling of `LIMIT` and is interchangeable with it:

```sql
SELECT title, price FROM books ORDER BY price DESC FETCH FIRST 3 ROWS ONLY;
```

## Joining Tables

Find each book's author by joining `books` to `authors`:

```sql
SELECT b.title, a.name AS author, b.price
FROM books b
JOIN authors a ON b.author_id = a.id
ORDER BY a.name, b.title;
```

```
 title                     | author            | price
---------------------------+-------------------+-------
 Blood Meridian            | Cormac McCarthy   | 14.99
 Children of Dune          | Frank Herbert     | 13.99
 Dune                      | Frank Herbert     | 15.99
 The Hobbit                | J.R.R. Tolkien    | 12.99
 The Lord of the Rings     | J.R.R. Tolkien    | 24.99
 The Dispossessed          | Ursula K. Le Guin | 11.99
 The Left Hand of Darkness | Ursula K. Le Guin | 13.99
```

Find all orders with book title and total:

```sql
SELECT o.id AS order_id, b.title, o.quantity, o.total_price
FROM orders o
JOIN books b ON o.book_id = b.id
ORDER BY o.id;
```

```
 order_id |          title            | quantity | total_price
----------+---------------------------+----------+-------------
        1 | The Hobbit                |        2 |       25.98
        2 | Dune                      |        1 |       15.99
        3 | The Lord of the Rings     |        1 |       24.99
        4 | The Left Hand of Darkness |        3 |       41.97
        5 | Dune                      |        5 |       79.95
```

### LEFT JOIN

Show all authors, including those with no books (Cormac McCarthy has one, but let us add an author with none to demonstrate):

```sql
INSERT INTO authors VALUES (5, 'Gene Wolfe', 'United States');

SELECT a.name, b.title
FROM authors a
LEFT JOIN books b ON b.author_id = a.id
ORDER BY a.name, b.title;
```

```
        name         |          title
---------------------+--------------------------
 Cormac McCarthy     | Blood Meridian
 Frank Herbert       | Children of Dune
 Frank Herbert       | Dune
 Gene Wolfe          | NULL
 J.R.R. Tolkien      | The Hobbit
 J.R.R. Tolkien      | The Lord of the Rings
 Ursula K. Le Guin   | The Dispossessed
 Ursula K. Le Guin   | The Left Hand of Darkness
```

Gene Wolfe appears with a NULL title because no books reference his author ID. NULLs sort last within each author group.

### JOIN USING

When the column you are joining on has the same name in both tables, `JOIN USING` is a shorthand that avoids repeating the column name. It also deduplicates the join column in the result, matching PostgreSQL semantics.

Add a `genre` column to make this concrete. First, create a genres reference table and a mapping table:

```sql
CREATE TABLE genres (
    id   INT NOT NULL,
    name TEXT NOT NULL
);

CREATE TABLE book_genres (
    book_id  INT NOT NULL,
    genre_id INT NOT NULL
);

INSERT INTO genres VALUES (1, 'Fantasy'), (2, 'Science Fiction'), (3, 'Western');

INSERT INTO book_genres VALUES (1, 1), (2, 1), (3, 2), (4, 2), (5, 2), (6, 3), (7, 2);
```

Now join `book_genres` to `genres` using the shared `genre_id` column:

```sql
SELECT bg.book_id, g.name AS genre
FROM book_genres bg
JOIN genres g USING (genre_id)
ORDER BY bg.book_id;
```

```
 book_id |      genre
---------+----------------
       1 | Fantasy
       2 | Fantasy
       3 | Science Fiction
       4 | Science Fiction
       5 | Science Fiction
       6 | Western
       7 | Science Fiction
```

The `genre_id` column appears only once in the output even though it exists in both tables. With `JOIN ON` you would have written `ON bg.genre_id = g.genre_id` and needed to pick which table's copy to project.

### LATERAL — One Subquery Result Per Outer Row

A common reporting pattern is "for each X, give me the top Y." For example: the most expensive book per author. The `LATERAL` keyword allows a subquery in the `FROM` clause to reference columns from earlier tables in the same `FROM` list — it re-runs once per outer row.

```sql
SELECT a.name AS author, top_book.title, top_book.price
FROM authors a
JOIN LATERAL (
    SELECT title, price
    FROM books b
    WHERE b.author_id = a.id
    ORDER BY price DESC
    LIMIT 1
) AS top_book ON true
ORDER BY a.name;
```

```
        author         |          title           | price
-----------------------+--------------------------+-------
 Cormac McCarthy       | Blood Meridian           | 14.99
 Frank Herbert         | Dune                     | 15.99
 J.R.R. Tolkien        | The Lord of the Rings    | 24.99
 Ursula K. Le Guin     | The Left Hand of Darkness| 13.99
```

The `ON true` is required syntax — it says "always join these rows." Use `LEFT JOIN LATERAL ... ON true` if you want to keep authors who have no books (they will appear with NULL columns from the lateral subquery).

LATERAL joins are more readable than correlated scalar subqueries when you need more than one column from the "inner" result, or when you need `LIMIT` inside the subquery.

### Finding Books with IS NULL and ILIKE

The `authors` table has a nullable `country` column. Find any author whose country is not recorded:

```sql
SELECT name FROM authors WHERE country IS NULL;
```

All authors in the table currently have a known country (Gene Wolfe, inserted for the LEFT JOIN example, has `'United States'`), so this returns zero rows. To demonstrate, add an author with an unknown country:

```sql
INSERT INTO authors VALUES (6, 'Anonymous', NULL);

SELECT name FROM authors WHERE country IS NULL;
```

```
   name
-----------
 Anonymous
```

Never use `= NULL` for this check — in SQL, `NULL = NULL` evaluates to `NULL` (unknown), not `TRUE`. `IS NULL` is the correct predicate.

`ILIKE` performs a case-insensitive pattern match, which is useful for searches on user-entered text:

```sql
SELECT name FROM authors WHERE name ILIKE '%ursula%';
```

```
        name
-------------------
 Ursula K. Le Guin
```

The pattern uses `%` as a wildcard (matches any sequence of characters). `ILIKE` treats upper and lower case as equivalent; `LIKE` is the case-sensitive version.

### DISTINCT — Unique Values

To see which countries are represented in the author table without duplicates, use `SELECT DISTINCT`:

```sql
SELECT DISTINCT country FROM authors WHERE country IS NOT NULL ORDER BY country;
```

```
    country
---------------
 United Kingdom
 United States
```

Without `DISTINCT`, each author row would contribute its country, producing repeated values. `DISTINCT` removes duplicates from the result before returning rows.

`DISTINCT` works on multiple columns too — rows are deduplicated by the combination of all selected columns:

```sql
SELECT DISTINCT country, name FROM authors ORDER BY country, name;
```

### RETURNING — Getting Back What You Inserted

Normally `INSERT` returns only a row count. The `RETURNING` clause lets you get column values from the inserted rows back in the same statement — useful when you need the stored value immediately without a follow-up `SELECT`.

Insert a new order and retrieve the assigned `id` and computed `total_price` in one shot:

```sql
INSERT INTO orders VALUES (6, 2, 3, 74.97) RETURNING id, total_price;
```

```
 id | total_price
----+-------------
  6 |       74.97
```

`RETURNING` also works with `UPDATE` and `DELETE`. After updating prices, see the new values immediately:

```sql
UPDATE books SET price = price * 0.90
WHERE author_id = 4
RETURNING title, price;
```

```
     title      | price
----------------+--------
 Blood Meridian | 13.491
```

And after a delete, confirm which rows were removed:

```sql
DELETE FROM authors WHERE id = 6 RETURNING name;
```

```
   name
-----------
 Anonymous
```

### UNION — Combining Result Sets

`UNION` merges two query results into one, removing duplicates. `UNION ALL` keeps duplicates for better performance when you know there are none or want to count them.

Suppose you want a single list of all entity names in the bookstore — both author names and book titles:

```sql
SELECT name AS label, 'author' AS kind FROM authors
UNION
SELECT title, 'book' FROM books
ORDER BY kind, label;
```

```
            label            |  kind
-----------------------------+--------
 Anonymous                   | author
 Cormac McCarthy             | author
 Frank Herbert               | author
 J.R.R. Tolkien              | author
 Ursula K. Le Guin           | author
 Blood Meridian              | book
 Children of Dune            | book
 Dune                        | book
 The Dispossessed            | book
 The Hobbit                  | book
 The Left Hand of Darkness   | book
 The Lord of the Rings       | book
```

`INTERSECT` returns only rows present in both queries; `EXCEPT` returns rows from the first query that are not in the second.

### CTEs — Readable Multi-Step Queries

A Common Table Expression (CTE), written with the `WITH` keyword, lets you name an intermediate result and reference it later in the same query. This is especially useful for breaking a complex analysis into readable steps.

Compute each author's total revenue from orders, then filter to authors who have earned more than $50:

```sql
WITH book_revenue AS (
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
ORDER BY total_revenue DESC;
```

```
      name      | total_revenue
----------------+---------------
 Frank Herbert  |         95.94
 Ursula K. Le Guin |      41.97
```

Each CTE block (`book_revenue`, `author_revenue`) is evaluated once and can be referenced multiple times in the main query. They make queries with multiple aggregation steps far easier to read and maintain than equivalent nested subqueries.

## Aggregating Data

How many books does each author have?

```sql
SELECT a.name, COUNT(*) AS book_count
FROM books b
JOIN authors a ON b.author_id = a.id
GROUP BY a.name
ORDER BY book_count DESC;
```

```
        name         | book_count
---------------------+------------
 Frank Herbert       |          2
 J.R.R. Tolkien      |          2
 Ursula K. Le Guin   |          2
 Cormac McCarthy     |          1
```

Average, minimum, maximum, and sum of prices:

```sql
SELECT
    AVG(price) AS avg_price,
    MIN(price) AS min_price,
    MAX(price) AS max_price,
    SUM(price) AS total_catalog_value
FROM books;
```

```
    avg_price     | min_price | max_price | total_catalog_value
------------------+-----------+-----------+---------------------
 15.6957142857143 |     11.99 |     24.99 |              109.87
```

Revenue per book (summing order quantities):

```sql
SELECT b.title, SUM(o.quantity) AS units_sold, SUM(o.total_price) AS revenue
FROM orders o
JOIN books b ON o.book_id = b.id
GROUP BY b.title
ORDER BY revenue DESC;
```

```
          title            | units_sold | revenue
---------------------------+------------+---------
 Dune                      |          6 |   95.94
 The Left Hand of Darkness |          3 |   41.97
 The Lord of the Rings     |          1 |   24.99
 The Hobbit                |          2 |   25.98
```

## Updating Rows

Apply a 10% price increase to all Herbert books:

```sql
UPDATE books SET price = price * 1.10 WHERE author_id = 2;
```

```
UPDATE 2
```

Verify:

```sql
SELECT title, price FROM books WHERE author_id = 2;
```

```
      title       | price
------------------+-------
 Dune             | 17.589
 Children of Dune | 15.389
```

Correct a typo in an author's name:

```sql
UPDATE authors SET name = 'Ursula K. Le Guin' WHERE id = 3;
```

## Deleting Rows

Remove the Gene Wolfe entry we added for the LEFT JOIN example:

```sql
DELETE FROM authors WHERE id = 5;
```

```
DELETE 1
```

Delete all orders for a specific book:

```sql
DELETE FROM orders WHERE book_id = 1;
```

After deletion the rows are no longer visible to new queries. The physical space is not immediately reclaimed — that is the job of VACUUM. Run `VACUUM books;` periodically (or `VACUUM;` to process all tables) to reclaim space from dead tuple versions. See Chapter 4 for the full VACUUM reference.

## Exiting the CLI

Type `\q` or press `Ctrl-D` to exit the CLI. The server continues running. Stop it with `Ctrl-C` in the server terminal.

The data directory persists everything. When you restart the server pointing at the same `--data-dir`, all tables and data are exactly where you left them.

## Advanced Queries

This section demonstrates more powerful SQL features using the bookstore data already in the database.

### Conditional Expressions

Use `CASE WHEN` to add a computed label to each row:

```sql
SELECT title, price,
       CASE WHEN price < 13.00 THEN 'budget'
            WHEN price < 20.00 THEN 'mid-range'
            ELSE 'premium'
       END AS tier
FROM books
ORDER BY price;
```

```
          title            | price |    tier
---------------------------+-------+-----------
 The Dispossessed          | 11.99 | budget
 The Hobbit                | 12.99 | budget
 The Left Hand of Darkness | 13.99 | mid-range
 Children of Dune          | 13.99 | mid-range
 Blood Meridian            | 14.99 | mid-range
 Dune                      | 15.99 | mid-range
 The Lord of the Rings     | 24.99 | premium
```

Use `COALESCE` to provide a fallback value for a nullable column:

```sql
SELECT name, COALESCE(country, 'Unknown') AS country FROM authors ORDER BY name;
```

Any author with a NULL `country` will show `'Unknown'` instead of blank.

### String Functions

icedb provides a full set of text manipulation functions. These are especially useful for normalising data at query time or doing lightweight text search without a full-text index.

Convert to upper or lower case:

```sql
SELECT UPPER(name) AS shout, LOWER(name) AS whisper FROM authors LIMIT 2;
```

```
         shout          |        whisper
------------------------+------------------------
 J.R.R. TOLKIEN         | j.r.r. tolkien
 FRANK HERBERT          | frank herbert
```

Strip leading/trailing whitespace with `TRIM`, then measure a string's length:

```sql
SELECT name, LENGTH(TRIM(name)) AS chars FROM authors ORDER BY chars DESC;
```

Extract a substring and find the position of a pattern:

```sql
-- Extract just the first word of each title
SELECT title, SUBSTRING(title, 1, POSITION(' ' IN title) - 1) AS first_word
FROM books
WHERE POSITION(' ' IN title) > 0;
```

```
          title            | first_word
---------------------------+------------
 The Hobbit                | The
 The Lord of the Rings     | The
 The Left Hand of Darkness | The
 Children of Dune          | Children
```

Replace text and build display strings:

```sql
SELECT REPLACE(name, '.', '') AS simplified_name FROM authors;
```

The `||` operator concatenates two strings. `CONCAT(a, b, ...)` is a NULL-safe alternative that treats NULL as an empty string instead of propagating it:

```sql
SELECT name || ' (' || country || ')' AS label FROM authors WHERE country IS NOT NULL;
```

For a complete list of string functions — including `LPAD`, `RPAD`, `SPLIT_PART`, `REPEAT`, `REVERSE`, `LEFT`, and `RIGHT` — see Chapter 4.

### Renaming and Extending the Schema

Add a `bio` column to the authors table. Existing rows receive NULL for the new column automatically.

```sql
ALTER TABLE authors ADD COLUMN bio TEXT;
```

Update one author with a bio:

```sql
UPDATE authors SET bio = 'Author of Middle-earth.' WHERE id = 1;
```

Check that the other rows still have NULL for the new column:

```sql
SELECT id, name, bio FROM authors ORDER BY id;
```

```
 id |        name         |           bio
----+---------------------+-------------------------
  1 | J.R.R. Tolkien      | Author of Middle-earth.
  2 | Frank Herbert       | NULL
  3 | Ursula K. Le Guin   | NULL
  4 | Cormac McCarthy     | NULL
  5 | Gene Wolfe          | NULL
```

You can also rename or drop a column, or rename the whole table:

```sql
-- Rename a column
ALTER TABLE authors RENAME COLUMN bio TO biography;
ALTER TABLE authors RENAME COLUMN biography TO bio;  -- rename it back

-- Drop a column
ALTER TABLE authors DROP COLUMN bio;

-- Rename the table itself
ALTER TABLE authors RENAME TO writers;
ALTER TABLE writers RENAME TO authors;   -- rename it back
```

### Ensuring Uniqueness

Create a table with a `PRIMARY KEY` and a `UNIQUE` column:

```sql
CREATE TABLE members (
    id      INTEGER PRIMARY KEY,
    email   TEXT    UNIQUE NOT NULL,
    handle  TEXT    NOT NULL
);

INSERT INTO members VALUES (1, 'alice@example.com', 'alice');
INSERT INTO members VALUES (2, 'bob@example.com',   'bob');
```

Attempting to insert a duplicate email raises a constraint violation (SQLSTATE 23000):

```sql
INSERT INTO members VALUES (3, 'alice@example.com', 'alice2');
-- ERROR 23000: duplicate key value violates unique constraint
```

The primary key column is also enforced:

```sql
INSERT INTO members VALUES (1, 'carol@example.com', 'carol');
-- ERROR 23000: duplicate key value violates unique constraint
```

### Analytics with Window Functions

Rank books by price within each author, from most to least expensive:

```sql
SELECT a.name AS author, b.title, b.price,
       ROW_NUMBER() OVER (PARTITION BY b.author_id ORDER BY b.price DESC) AS price_rank
FROM books b
JOIN authors a ON a.id = b.author_id
ORDER BY a.name, price_rank;
```

```
        author       |          title            | price | price_rank
---------------------+---------------------------+-------+------------
 Cormac McCarthy     | Blood Meridian            | 14.99 |          1
 Frank Herbert       | Dune                      | 15.99 |          1
 Frank Herbert       | Children of Dune          | 13.99 |          2
 J.R.R. Tolkien      | The Lord of the Rings     | 24.99 |          1
 J.R.R. Tolkien      | The Hobbit                | 12.99 |          2
 Ursula K. Le Guin   | The Left Hand of Darkness | 13.99 |          1
 Ursula K. Le Guin   | The Dispossessed          | 11.99 |          2
```

`ROW_NUMBER()` assigns a unique number within each `PARTITION BY` group, ordered by `ORDER BY`. The partition resets for each new author.

Use `SUM() OVER` to show a running per-author total alongside each row without collapsing the result:

```sql
SELECT a.name AS author, b.title, b.price,
       SUM(b.price) OVER (PARTITION BY b.author_id) AS author_total
FROM books b
JOIN authors a ON a.id = b.author_id
ORDER BY a.name, b.price DESC;
```

### Recursive Queries

`WITH RECURSIVE` lets a CTE reference itself, which is useful for generating sequences or traversing hierarchies.

Generate the numbers 1 through 5:

```sql
WITH RECURSIVE series(n) AS (
    SELECT 1
    UNION ALL
    SELECT n + 1 FROM series WHERE n < 5
)
SELECT n FROM series;
```

```
 n
---
 1
 2
 3
 4
 5
```

The base case (`SELECT 1`) seeds the result. The recursive step (`SELECT n + 1 FROM series WHERE n < 5`) keeps adding rows until the condition fails. icedb stops after 1,000 iterations to protect against infinite loops.

## Production Patterns

This section shows patterns that come up immediately in real applications: auto-increment IDs, default column values, referential integrity, handling duplicate inserts gracefully, and controlling who can read or write your tables.

### Auto-Increment IDs with SERIAL

Manually assigning integer IDs is error-prone. Use `SERIAL` (an alias for an auto-incrementing `INTEGER`) to let the database assign IDs automatically:

```sql
CREATE TABLE customers (
    id      SERIAL PRIMARY KEY,
    email   TEXT NOT NULL,
    name    TEXT NOT NULL
);

INSERT INTO customers (email, name) VALUES ('alice@example.com', 'Alice');
INSERT INTO customers (email, name) VALUES ('bob@example.com',   'Bob');

SELECT * FROM customers;
```

```
 id |       email        | name
----+--------------------+------
  1 | alice@example.com  | Alice
  2 | bob@example.com    | Bob
```

You do not provide a value for `id`; the engine picks the next value from the internal sequence. `SERIAL` generates `INTEGER` values; `BIGSERIAL` generates `BIGINT` values for tables expected to exceed 2 billion rows.

### DEFAULT Column Values

Columns can carry a default expression that is used when the column is omitted from an `INSERT`:

```sql
CREATE TABLE products (
    id          SERIAL PRIMARY KEY,
    name        TEXT NOT NULL,
    price       NUMERIC(10, 2) NOT NULL,
    in_stock    BOOLEAN DEFAULT TRUE,
    created_at  TIMESTAMP DEFAULT NOW()
);

INSERT INTO products (name, price) VALUES ('Widget', 9.99);

SELECT * FROM products;
```

```
 id |  name  | price | in_stock |      created_at
----+--------+-------+----------+-----------------------
  1 | Widget |  9.99 | t        | 2026-03-19 12:00:00
```

`in_stock` defaulted to `TRUE` and `created_at` was filled with the current timestamp at insert time — without either value appearing in the `INSERT` statement.

### Foreign Keys for Referential Integrity

A `FOREIGN KEY` constraint ensures that every row in a child table references an existing row in the parent table:

```sql
CREATE TABLE categories (
    id   SERIAL PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE items (
    id          SERIAL PRIMARY KEY,
    category_id INT NOT NULL REFERENCES categories(id),
    name        TEXT NOT NULL
);

INSERT INTO categories (name) VALUES ('Electronics');

-- This works: category 1 exists
INSERT INTO items (category_id, name) VALUES (1, 'Laptop');

-- This fails: category 99 does not exist
INSERT INTO items (category_id, name) VALUES (99, 'Mystery item');
-- ERROR: Foreign key constraint violated: category_id references categories(id)
```

The database rejects the second insert automatically — no application-level check required. Similarly, deleting the parent row while child rows still reference it raises an error:

```sql
DELETE FROM categories WHERE id = 1;
-- ERROR: Foreign key constraint violated: items references categories
```

### CHECK Constraints

A `CHECK` constraint enforces a boolean condition on every inserted or updated row:

```sql
CREATE TABLE prices (
    id      SERIAL PRIMARY KEY,
    amount  NUMERIC(10, 2) NOT NULL CHECK (amount >= 0),
    label   TEXT NOT NULL CHECK (length(label) > 0)
);

INSERT INTO prices (amount, label) VALUES (19.99, 'Standard');  -- OK
INSERT INTO prices (amount, label) VALUES (-5.00, 'Discount');  -- ERROR
-- ERROR: Check constraint violated: amount >= 0
```

### UPSERT — Insert or Handle Conflicts Gracefully

`INSERT ... ON CONFLICT` lets you handle a duplicate key situation without raising an error. This is commonly called "upsert" (update-or-insert).

**DO NOTHING** — silently skip if the row already exists:

```sql
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

INSERT INTO settings VALUES ('theme', 'dark');

-- Re-inserting the same key does nothing instead of erroring:
INSERT INTO settings VALUES ('theme', 'light') ON CONFLICT DO NOTHING;

SELECT * FROM settings;
```

```
  key  | value
-------+-------
 theme | dark
```

The second insert was silently ignored.

**DO UPDATE** — update the existing row with new values:

```sql
INSERT INTO settings (key, value) VALUES ('theme', 'light')
ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value;

SELECT * FROM settings;
```

```
  key  | value
-------+-------
 theme | light
```

`EXCLUDED` refers to the row that was proposed but conflicted. This pattern is safe to run repeatedly — each run is idempotent.

### Savepoints — Partial Rollback Within a Transaction

A savepoint marks a named point inside an open transaction. If a later step fails or you change your mind, you can roll back to the savepoint without discarding everything the transaction did before it.

```sql
BEGIN;

-- Step 1: deduct from Alice's account
UPDATE accounts SET balance = balance - 100 WHERE name = 'Alice';
SAVEPOINT after_debit;

-- Step 2: credit Bob's account (imagine this fails due to a constraint)
UPDATE accounts SET balance = balance + 100 WHERE name = 'Bob';

-- Oops — something was wrong. Roll back only step 2:
ROLLBACK TO SAVEPOINT after_debit;

-- Retry step 2 with corrected values, then commit:
UPDATE accounts SET balance = balance + 100 WHERE name = 'Bob';
COMMIT;
```

`RELEASE SAVEPOINT name` discards the savepoint (freeing the name) without rolling back:

```sql
SAVEPOINT sp1;
-- ... do work ...
RELEASE SAVEPOINT sp1;   -- savepoint consumed; no rollback
COMMIT;
```

> **Note:** In the current version of icedb, `ROLLBACK TO SAVEPOINT` aborts the entire transaction and starts a new one — pre-savepoint changes are not preserved. Full partial-rollback (true page-level undo) is planned for a future release. SAVEPOINT and RELEASE are accepted and tracked, and the commands are safe to use.

### Granting Table Access to a Role

By default, only a superuser can read or write tables. To allow an application role to access specific tables, use `GRANT`:

```sql
-- Create a read-only reporting role
CREATE ROLE reporter WITH LOGIN PASSWORD 'report-pass';

-- Grant SELECT on specific tables
GRANT SELECT ON books TO reporter;
GRANT SELECT ON authors TO reporter;

-- The reporter role can now query these tables but not insert or update
```

Connect as `reporter` and verify access works:

```sql
SELECT title FROM books LIMIT 3;
```

And verify that INSERT is blocked:

```sql
INSERT INTO books VALUES (99, 'Unauthorized', 1, 0.00, 2024);
-- ERROR: Permission denied for table 'books': role 'reporter' does not have INSERT privilege
```

To remove access later:

```sql
REVOKE SELECT ON books FROM reporter;
```

For an application role that needs full DML access:

```sql
CREATE ROLE appuser WITH LOGIN PASSWORD 'app-pass';
GRANT SELECT, INSERT, UPDATE, DELETE ON books TO appuser;
GRANT SELECT, INSERT, UPDATE, DELETE ON authors TO appuser;
```

To grant all four privileges at once:

```sql
GRANT ALL ON orders TO appuser;
```

See Chapter 7 for the complete security and privilege reference.

### Bulk Operations with COPY

When loading or exporting large amounts of data, `COPY` is far more efficient than individual `INSERT` statements.

**Exporting a table to CSV:**

```sql
COPY books TO '/tmp/books.csv' (FORMAT CSV, HEADER);
```

This writes the entire `books` table to `/tmp/books.csv` with a header row:

```
id,title,author_id,price,published
1,The Hobbit,1,12.99,1937
2,The Lord of the Rings,1,24.99,1954
...
```

**Importing from CSV:**

```sql
CREATE TABLE books_backup (
    id        INT,
    title     TEXT,
    author_id INT,
    price     FLOAT,
    published INT
);

COPY books_backup FROM '/tmp/books.csv' (FORMAT CSV, HEADER);

SELECT COUNT(*) FROM books_backup;
```

```
 COUNT(*)
----------
        7
```

The column order in the CSV must match the table's column order. The `HEADER` option tells icedb to skip the first row when importing (and write it when exporting). Use `COPY ... (FORMAT CSV)` without `HEADER` if your file has no header row.

### Event Notifications with LISTEN / NOTIFY

icedb supports a lightweight pub/sub mechanism compatible with PostgreSQL's LISTEN/NOTIFY interface. This is useful for triggering work in one session based on activity in another.

```sql
-- In session 1: subscribe to a channel
LISTEN order_updates;

-- In session 2: send a notification
NOTIFY order_updates, 'new order id=1234';

-- Session 1 receives the notification asynchronously.
-- In the CLI, notifications are printed when the next prompt is shown.
```

To stop listening on a channel:

```sql
UNLISTEN order_updates;
UNLISTEN *;    -- stop listening on all channels
```

Channel names are case-sensitive strings. The payload is optional — `NOTIFY channel_name` sends a bare notification with no payload.

## Managing Multiple Databases

You created the `bookstore` database at the start of this chapter. Here is the full picture of database management commands.

### IF NOT EXISTS / IF EXISTS

```sql
-- Won't error if bookstore already exists
CREATE DATABASE IF NOT EXISTS bookstore;

-- Won't error if staging doesn't exist
DROP DATABASE IF EXISTS staging;
```

### Database isolation

Tables in one database are completely invisible from another. Try it:

```
bookstore=# \c icedb
You are now connected to database "icedb".
icedb=# \dt
-- (empty — authors/books/orders live in bookstore, not icedb)
icedb=# \c bookstore
You are now connected to database "bookstore".
bookstore=# \dt
 Schema |  Name   | Type
--------+---------+-------
 public | authors | table
 public | books   | table
 public | orders  | table
```

### Connecting directly on startup

Skip the `\c` step by naming the database when starting the CLI:

```sh
cargo run -p cli --release -- --data-dir ./data --dbname bookstore
```

Over TCP with psql:

```sh
psql -h 127.0.0.1 -p 5432 -d bookstore -U icedb
```

If the database name does not exist, icedb returns SQLSTATE `3D000`:

```
psql: error: FATAL: database "nonexistent" does not exist
```

### Dropping a database

```sql
DROP DATABASE bookstore_v2;
DROP DATABASE IF EXISTS bookstore_v2;
```

The default `icedb` database cannot be dropped. Data files on disk are retained after drop (for safety) — only the registry entry is removed.

### Listing databases

```
bookstore=# \l
                                  List of databases
   Name      |  Owner
-------------+----------
 bookstore   | icedb
 icedb       | icedb
```

For the complete database management SQL reference, see Chapter 4.

---

## What Happened Behind the Scenes

Every statement you ran went through the full icedb stack:

1. The CLI passed the SQL string to the query engine directly (in embedded mode, bypassing TCP).
2. The `sql` crate parsed the statement into an AST using `sqlparser-rs`.
3. The planner converted the AST into a `LogicalPlan` (a tree of plan nodes: `TableScan`, `Filter`, `Project`, `Join`, `Aggregate`, `Sort`).
4. The executor walked the plan tree in Volcano/iterator style, calling `next()` on each operator.
5. For each table scan, the transaction manager fetched a snapshot and scanned visible tuples from the heap file, applying MVCC visibility rules.
6. INSERT and UPDATE statements wrote WAL records (`PageImage` records containing the full modified page) before writing the page to disk.
7. On COMMIT, the WAL was fsynced, making the data durable.

Every row you inserted has a `t_xmin` field in its tuple header recording which transaction created it. When you issued an UPDATE, the old row version got `t_xmax` set to the updating transaction's ID, and a new version with `t_xmin` set to the same transaction appeared. DELETE sets `t_xmax` on the target row, hiding it from future snapshots. Chapter 5 covers this in depth.
