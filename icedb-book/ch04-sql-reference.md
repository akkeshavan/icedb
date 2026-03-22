# Chapter 4: SQL Reference

_This chapter describes icedb 0.1.x (Phases 1–9). Re-check the SQL reference for later versions._

This chapter documents every SQL feature supported by icedb. Use it as a reference alongside the tutorial. The final section lists features that are not yet implemented.

**In this chapter:**
- Data types (BOOLEAN, INT, BIGINT, FLOAT, TEXT, VARCHAR, BYTEA, DATE, TIMESTAMP, NUMERIC, UUID)
- Type casting (including Text-to-numeric, Infinity, NaN, boolean strings)
- DDL: CREATE TABLE (with PRIMARY KEY, UNIQUE, CHECK, FK), DROP TABLE, ALTER TABLE (ADD/DROP/RENAME COLUMN, RENAME TABLE), CREATE INDEX, CREATE ROLE, CREATE DATABASE, DROP DATABASE, CREATE SCHEMA
- DML: INSERT, UPDATE, DELETE (with RETURNING), INSERT ON CONFLICT (UPSERT)
- Queries: SELECT, WHERE, ORDER BY, LIMIT / OFFSET / FETCH FIRST, GROUP BY, JOINs (INNER/LEFT/RIGHT/FULL/CROSS/LATERAL), subqueries (correlated), CTEs (WITH RECURSIVE), window functions, set operations
- Conditional expressions: CASE WHEN, COALESCE, NULLIF
- String operators: `||` concatenation, UPPER, LOWER, LENGTH, SUBSTRING, POSITION, REPLACE, TRIM
- Arithmetic operators including modulo (`%`)
- NULL semantics (including IS UNKNOWN)
- Transaction control: BEGIN, COMMIT, ROLLBACK, SAVEPOINT, SET TRANSACTION
- COPY, PREPARE/EXECUTE, GRANT/REVOKE, LISTEN/NOTIFY, VACUUM
- Error codes (SQLSTATE)
- Unsupported features

## Data Types

icedb supports seven scalar types. The SQL type name and the internal Rust enum variant are shown for each.

### BOOLEAN

Stores `true` or `false`. Displayed as `t` or `f` (PostgreSQL convention).

```sql
CREATE TABLE flags (enabled BOOLEAN);
INSERT INTO flags VALUES (true);
INSERT INTO flags VALUES (false);
SELECT * FROM flags WHERE enabled = true;
```

Internal Rust type: `Value::Bool(bool)`. Serialized as 1 byte (1 = true, 0 = false).

Aliases accepted by the parser: `BOOL`.

### INT / INT4 / INTEGER

32-bit signed integer. Range: −2,147,483,648 to 2,147,483,647.

```sql
CREATE TABLE counters (n INT NOT NULL);
INSERT INTO counters VALUES (0), (100), (-50);
```

Internal Rust type: `Value::Int4(i32)`. Serialized as 4 bytes, little-endian.

Accepted type names: `INT`, `INT4`, `INTEGER`.

### BIGINT / INT8

64-bit signed integer. Range: −9,223,372,036,854,775,808 to 9,223,372,036,854,775,807.

```sql
CREATE TABLE large_ids (id BIGINT NOT NULL);
INSERT INTO large_ids VALUES (9000000000);
```

Internal Rust type: `Value::Int8(i64)`. Serialized as 8 bytes, little-endian.

Accepted type names: `BIGINT`, `INT8`.

### FLOAT / FLOAT8 / DOUBLE PRECISION

64-bit IEEE 754 double-precision floating-point number.

```sql
CREATE TABLE measurements (value FLOAT);
INSERT INTO measurements VALUES (3.14159265358979);
```

Internal Rust type: `Value::Float8(f64)`. Serialized as 8 bytes, little-endian.

Accepted type names: `FLOAT`, `FLOAT8`, `DOUBLE PRECISION`, `REAL`.

Note: icedb does not implement a separate FLOAT4 (single precision) type. All floating-point columns use FLOAT8 internally.

### TEXT

Variable-length Unicode string with no length limit (up to available page space).

```sql
CREATE TABLE notes (body TEXT);
INSERT INTO notes VALUES ('Hello, world!');
INSERT INTO notes VALUES ('Multi-line strings work too');
```

Internal Rust type: `Value::Text(String)`. Serialized as a 4-byte little-endian length prefix followed by the UTF-8 bytes.

### VARCHAR(n)

Variable-length string with a declared maximum character count. Stored identically to TEXT internally. The length limit is recorded in `pg_attribute` (`atttypmod`) for tooling compatibility but is not enforced at the storage layer in the current implementation.

```sql
CREATE TABLE codes (code VARCHAR(10) NOT NULL);
INSERT INTO codes VALUES ('ABC123');
```

Internal Rust type: `Value::Text(String)` (same as TEXT). The `n` in `VARCHAR(n)` is stored as `atttypmod`.

### SERIAL / BIGSERIAL

Auto-incrementing integer column. `SERIAL` produces a 32-bit integer; `BIGSERIAL` produces a 64-bit integer. Each table gets a per-column sequence stored on disk and restored on restart.

```sql
CREATE TABLE users (
    id   SERIAL PRIMARY KEY,
    name TEXT NOT NULL
);
INSERT INTO users (name) VALUES ('Alice');  -- id = 1
INSERT INTO users (name) VALUES ('Bob');    -- id = 2
```

### DATE

A calendar date (year, month, day). Stored as an i32 representing days since the Unix epoch (1970-01-01).

```sql
CREATE TABLE events (id INT, event_date DATE);
INSERT INTO events VALUES (1, '2024-01-15');
SELECT * FROM events WHERE event_date > '2024-01-01';
```

### TIMESTAMP

A date and time value (microsecond precision). Stored as an i64 representing microseconds since the Unix epoch.

```sql
CREATE TABLE logs (id INT, created_at TIMESTAMP);
INSERT INTO logs VALUES (1, '2024-01-15 10:30:00');
SELECT NOW();  -- returns current server timestamp
```

### NUMERIC

Arbitrary-precision decimal number. Stored internally as a string to preserve exact representation.

```sql
CREATE TABLE prices (id INT, amount NUMERIC);
INSERT INTO prices VALUES (1, 123.45);
```

### UUID

A 128-bit universally unique identifier. Stored as a string in the standard `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` format.

```sql
CREATE TABLE items (id UUID DEFAULT GEN_RANDOM_UUID(), name TEXT);
INSERT INTO items (name) VALUES ('widget');
SELECT id FROM items;  -- e.g. '550e8400-e29b-41d4-a716-446655440000'
```

### BYTEA

Raw binary data. Displayed in hex format with a `\x` prefix.

```sql
CREATE TABLE blobs (data BYTEA);
INSERT INTO blobs VALUES (E'\\xDEADBEEF');
```

Internal Rust type: `Value::Bytes(Vec<u8>)`. Serialized as a 4-byte length prefix followed by the raw bytes.

### Type Casting

icedb supports implicit and explicit casts between numeric types:

| From | To | Behavior |
|------|----|----------|
| INT4 | INT8 | widening, always safe |
| INT8 | INT4 | truncating (wraps on overflow) |
| INT4 | FLOAT8 | widening |
| INT8 | FLOAT8 | widening (may lose precision for very large integers) |
| FLOAT8 | INT4 | truncating toward zero |
| FLOAT8 | INT8 | truncating toward zero |
| any | TEXT | formats as string |
| BOOL | INT4 | true => 1, false => 0 |
| TEXT | INT4 | parses decimal string; error if invalid |
| TEXT | INT8 | parses decimal string; error if invalid |
| TEXT | FLOAT8 | parses decimal string; `'Infinity'`, `'-Infinity'`, `'NaN'` are accepted |
| TEXT | BOOLEAN | `'true'`/`'t'`/`'yes'`/`'on'`/`'1'` → true; `'false'`/`'f'`/`'no'`/`'off'`/`'0'` → false |

**Special float literals** — when casting from TEXT to FLOAT8, the following spellings (case-insensitive) are accepted:

```sql
SELECT 'Infinity'::FLOAT8;    -- +infinity
SELECT '-Infinity'::FLOAT8;   -- -infinity
SELECT 'NaN'::FLOAT8;         -- not-a-number

-- IEEE 754 special values also arise from arithmetic
SELECT 1.0 / 0.0;             -- ERROR 22012 (integer zero); use float zero:
SELECT 1.0e0 / 0.0e0;         -- Infinity
```

**Boolean text literals** — the TEXT → BOOLEAN cast accepts all PostgreSQL-compatible spellings:

```sql
SELECT 'true'::BOOLEAN;   -- t
SELECT 'yes'::BOOLEAN;    -- t
SELECT 'on'::BOOLEAN;     -- t
SELECT '1'::BOOLEAN;      -- t
SELECT 'false'::BOOLEAN;  -- f
SELECT 'no'::BOOLEAN;     -- f
SELECT 'off'::BOOLEAN;    -- f
SELECT '0'::BOOLEAN;      -- f
```

### NULL

Every column type can hold a `NULL` value unless the column is declared `NOT NULL`. NULL represents an absent value. Arithmetic or comparison with NULL yields NULL (three-valued logic).

---

## DDL — Schema Definition

### CREATE TABLE

```sql
CREATE TABLE table_name (
    column_name data_type [NOT NULL] [UNIQUE] [PRIMARY KEY] [DEFAULT expr],
    ...,
    [FOREIGN KEY (column) REFERENCES other_table(column) [ON DELETE action]],
    [CHECK (expression)]
);
```

Creates a new heap file and registers the table in `pg_class` and its columns in `pg_attribute`. The schema defaults to `public`.

**DEFAULT values** — specify a default expression evaluated at INSERT time:

```sql
CREATE TABLE orders (
    id         SERIAL PRIMARY KEY,
    status     TEXT DEFAULT 'pending',
    created_at TIMESTAMP DEFAULT NOW(),
    quantity   INT DEFAULT 1
);
INSERT INTO orders (status) VALUES (DEFAULT);  -- uses defaults for id, created_at, quantity
```

**FOREIGN KEY** — reference another table's column:

```sql
CREATE TABLE authors (id INT PRIMARY KEY, name TEXT);
CREATE TABLE books (
    id        INT PRIMARY KEY,
    author_id INT REFERENCES authors(id) ON DELETE RESTRICT,
    title     TEXT
);
```

ON DELETE actions: `RESTRICT` (default, prevents deletion), `CASCADE` (delete child rows).

**CHECK constraints** — validate column values:

```sql
CREATE TABLE products (
    id    INT PRIMARY KEY,
    price FLOAT CHECK (price > 0),
    stock INT  CHECK (stock >= 0)
);
```

```sql
CREATE TABLE products (
    id       INT NOT NULL,
    name     TEXT NOT NULL,
    price    FLOAT,
    in_stock BOOLEAN
);
```

With `PRIMARY KEY` and `UNIQUE` constraints:

```sql
CREATE TABLE users (
    id        INTEGER PRIMARY KEY,
    email     TEXT    UNIQUE NOT NULL,
    username  TEXT    NOT NULL,
    bio       TEXT
);
```

Constraint behavior:

- `PRIMARY KEY`: the column must be unique and non-NULL; the constraint is enforced on every INSERT. A table may have at most one primary key.
- `UNIQUE`: column values must be distinct. NULL values are allowed and do not conflict with each other (PostgreSQL semantics — two NULL values in a UNIQUE column do not violate the constraint). Violations return SQLSTATE `23000`.
- `NOT NULL`: NULL values are rejected at insert time. Violations return SQLSTATE `23502`.

To avoid an error if the table already exists:

```sql
CREATE TABLE IF NOT EXISTS products (
    id   INT NOT NULL,
    name TEXT NOT NULL
);
```

### DROP TABLE

```sql
DROP TABLE table_name;
DROP TABLE IF EXISTS table_name;
```

Removes the table from `pg_class` and `pg_attribute` (soft delete via MVCC). The underlying heap file is not physically deleted in this version.

### CREATE INDEX

```sql
CREATE INDEX ON table_name (column_name);
```

Creates a persistent B+ tree index. The index file is named `idx_<table_oid>_<column_name>.btree` in the data directory. Index creation scans the table and inserts all existing rows into the tree.

```sql
CREATE INDEX ON orders (customer_id);
CREATE INDEX ON products (name);
```

Named indexes are not yet supported. The index name cannot be specified; it is derived from the table OID and column name.

### CREATE ROLE

```sql
CREATE ROLE role_name WITH LOGIN PASSWORD 'password';
CREATE ROLE role_name WITH LOGIN SUPERUSER PASSWORD 'adminpass';
```

Creates a new role in `pg_authid`. The password is stored as a SCRAM-SHA-256 verifier (never plaintext). See Chapter 7 for details on the verifier format.

### ALTER TABLE

`ALTER TABLE` modifies an existing table's schema. Four operations are supported:

**Add a column:**

```sql
ALTER TABLE products ADD COLUMN description TEXT;
ALTER TABLE orders ADD COLUMN notes TEXT;
```

The new column is added to `pg_attribute`. Existing rows will return NULL for the new column until updated.

**Drop a column:**

```sql
ALTER TABLE products DROP COLUMN description;
```

Removes the column from `pg_attribute`. The column data in existing heap pages is no longer visible.

**Rename a column:**

```sql
ALTER TABLE products RENAME COLUMN name TO product_name;
```

Updates the column name in `pg_attribute`.

**Rename a table:**

```sql
ALTER TABLE products RENAME TO items;
```

Updates the table name in `pg_class`. All indexes on the table continue to function.

### CREATE DATABASE

Creates a new database. Each database has its own heap files, catalog, and WAL in a subdirectory of the data directory.

```sql
CREATE DATABASE myapp;
CREATE DATABASE IF NOT EXISTS analytics;
```

The new database is registered in `pg_database.json` (the database registry) in the data directory. Its data lives at `<data_dir>/databases/<dbname>/`. The special default database `icedb` continues to use `<data_dir>/` directly for backward compatibility.

To switch to the new database from the CLI, use `\c`:

```
icedb=# CREATE DATABASE myapp;
CREATE DATABASE
icedb=# \c myapp
You are now connected to database "myapp".
myapp=# CREATE TABLE customers (id INT PRIMARY KEY, name TEXT);
```

Over a network connection (psql), specify the database in the connection string:

```sh
psql -h 127.0.0.1 -p 5432 -d myapp -U icedb
```

### DROP DATABASE

Drops a database and removes its entry from the registry. The data directory for the database is **not** deleted from disk in this version (for safety).

```sql
DROP DATABASE myapp;
DROP DATABASE IF EXISTS myapp;
```

The default database `icedb` cannot be dropped.

### CREATE SCHEMA

Creates a named schema within the current database.

```sql
CREATE SCHEMA reporting;
CREATE SCHEMA reporting IF NOT EXISTS;
```

Currently, icedb uses schemas as a namespace in the catalog but all user tables are placed in `public` by default. Querying `pg_catalog.*` and `information_schema.*` tables uses the schema prefix.

---

## DML — Data Manipulation

### INSERT

Single-row insert:

```sql
INSERT INTO products VALUES (1, 'Widget', 9.99, true);
```

If the table has `NOT NULL` columns, all of them must receive a non-null value. NULL literals can be passed explicitly:

```sql
INSERT INTO products VALUES (2, 'Gadget', NULL, NULL);
```

Column list syntax (allows reordering and omission of nullable columns):

```sql
INSERT INTO products (id, name) VALUES (3, 'Doohickey');
```

Multi-row insert (all rows in a single statement):

```sql
INSERT INTO products VALUES
    (4, 'Alpha', 1.00, true),
    (5, 'Beta',  2.00, false),
    (6, 'Gamma', 3.00, true);
```

Each `INSERT` returns a command tag: `INSERT 0 N` where N is the number of rows inserted.

#### RETURNING

The `RETURNING` clause causes `INSERT`, `UPDATE`, or `DELETE` to return column values from the affected rows, avoiding a follow-up `SELECT`:

```sql
INSERT INTO products (id, name, price, in_stock)
VALUES (7, 'Sprocket', 4.50, true)
RETURNING id, name;
```

```
 id |   name
----+----------
  7 | Sprocket
```

Any column from the target table may appear in `RETURNING`. Expressions are also allowed:

```sql
INSERT INTO products VALUES (8, 'Cog', 2.25, false)
RETURNING id, name, price * 1.2 AS price_with_tax;
```

#### INSERT ON CONFLICT (UPSERT)

The `ON CONFLICT` clause handles unique constraint violations gracefully:

```sql
-- Silently ignore duplicate inserts
INSERT INTO users VALUES (1, 'alice') ON CONFLICT DO NOTHING;

-- Update the conflicting row instead of inserting
INSERT INTO counters (id, cnt) VALUES (1, 1)
ON CONFLICT (id) DO UPDATE SET cnt = counters.cnt + 1;
```

`DO NOTHING` leaves the existing row unchanged and does not count as an inserted row. `DO UPDATE` updates the existing row with the given assignments; column references refer to the existing row.

### UPDATE

```sql
UPDATE table_name SET column = expression [, ...] WHERE condition;
```

Updates matching rows. The WHERE clause is evaluated against the current snapshot; all matching visible rows are updated.

```sql
UPDATE products SET price = price * 1.05 WHERE in_stock = true;
UPDATE products SET in_stock = false WHERE price > 100.0;
UPDATE products SET name = 'Widget Pro', price = 19.99 WHERE id = 1;
```

Without a WHERE clause, all rows in the table are updated.

Each UPDATE is implemented as an MVCC soft-delete of the old tuple version plus an insert of the new version. The command tag is `UPDATE N`.

`RETURNING` is supported on `UPDATE` to see the updated values:

```sql
UPDATE products SET price = price * 1.10 WHERE in_stock = true
RETURNING id, name, price;
```

### DELETE

```sql
DELETE FROM table_name WHERE condition;
```

Marks matching rows as deleted (sets `t_xmax` in the tuple header to the current transaction ID). Rows remain physically present until a future VACUUM reclaims them.

```sql
DELETE FROM products WHERE in_stock = false;
DELETE FROM orders WHERE created_at < 1000;
```

Without a WHERE clause, all rows are deleted. The command tag is `DELETE N`.

`RETURNING` is supported on `DELETE` to see which rows were removed:

```sql
DELETE FROM products WHERE in_stock = false RETURNING id, name;
```

---

## COPY — Bulk Import and Export

### COPY TO — Export table to CSV

```sql
COPY table_name TO '/path/to/file.csv' (FORMAT CSV, HEADER);
COPY table_name TO '/path/to/file.csv' (FORMAT CSV);      -- no header
```

### COPY FROM — Import CSV into table

```sql
COPY table_name FROM '/path/to/file.csv' (FORMAT CSV, HEADER);
COPY table_name FROM '/path/to/file.csv' (FORMAT CSV, DELIMITER ',');
```

**Notes:**
- `FORMAT CSV` is the only supported format
- `HEADER` causes the first row to be treated as column names (COPY FROM skips it; COPY TO writes it)
- Column order in the file must match the table column order
- String values containing the delimiter or newlines are automatically quoted
- `COPY ... FROM STDIN` is not supported in embedded mode

### Example

```sql
CREATE TABLE products (id INT, name TEXT, price FLOAT);
INSERT INTO products VALUES (1, 'Widget', 9.99), (2, 'Gadget', 24.99);
COPY products TO '/tmp/products.csv' (FORMAT CSV, HEADER);
-- /tmp/products.csv now contains:
-- id,name,price
-- 1,Widget,9.99
-- 2,Gadget,24.99

CREATE TABLE products_copy (id INT, name TEXT, price FLOAT);
COPY products_copy FROM '/tmp/products.csv' (FORMAT CSV, HEADER);
SELECT COUNT(*) FROM products_copy;  -- 2
```

---

## Prepared Statements

Prepared statements allow you to parse and plan a query once, then execute it multiple times with different parameters.

```sql
PREPARE stmt_name AS SELECT * FROM orders WHERE customer_id = $1;
EXECUTE stmt_name(42);
EXECUTE stmt_name(99);
DEALLOCATE stmt_name;
DEALLOCATE ALL;    -- deallocate all prepared statements
```

**Notes:**
- `$1`, `$2`, etc. are positional parameters
- Parameters are substituted as SQL literals at execution time
- Prepared statements are session-scoped (lost on reconnect)
- `DEALLOCATE ALL` removes all prepared statements

### Example

```sql
PREPARE find_user AS SELECT name, email FROM users WHERE id = $1;
EXECUTE find_user(1);
EXECUTE find_user(42);
DEALLOCATE find_user;
```

---

## Queries

### SELECT

```sql
[WITH [RECURSIVE] cte_name AS (subquery) [, ...]]
SELECT [DISTINCT]
    expr [[AS] alias], ...
    [window_func() OVER (
        [PARTITION BY expr, ...]
        [ORDER BY expr [ASC|DESC], ...]
    ) AS alias]
FROM table_ref [alias]
    [JOIN table_ref ON condition | USING (col)]
    [LEFT | RIGHT | FULL [OUTER] JOIN table_ref ON condition | USING (col)]
    [CROSS JOIN table_ref]
[WHERE condition]
[GROUP BY expr, ...]
[HAVING aggregate_condition]
[{ UNION | UNION ALL | INTERSECT | EXCEPT } select_statement]
[ORDER BY sort_key [ASC | DESC] [, ...]]
[LIMIT n [OFFSET m]]
```

Select all columns:

```sql
SELECT * FROM products;
```

Select specific columns with expressions:

```sql
SELECT id, name, price * 1.2 AS price_with_tax FROM products;
```

Table aliases:

```sql
SELECT p.name, p.price FROM products p WHERE p.price < 10.0;
```

#### DISTINCT

`DISTINCT` removes duplicate rows from the result. Deduplication is based on the entire set of selected columns.

```sql
SELECT DISTINCT category FROM products ORDER BY category;
```

For multiple columns, a row is a duplicate only if every selected column matches:

```sql
SELECT DISTINCT category, in_stock FROM products ORDER BY category;
```

#### WITH — Common Table Expressions (CTEs)

A CTE names a subquery result so it can be referenced later in the same statement. CTEs make complex multi-step queries readable.

```sql
WITH expensive AS (
    SELECT id, name, price FROM products WHERE price > 50.0
),
summary AS (
    SELECT COUNT(*) AS cnt, AVG(price) AS avg_price FROM expensive
)
SELECT * FROM summary;
```

Multiple CTEs are separated by commas. Each CTE can reference CTEs defined before it in the same `WITH` clause. The main query at the end can reference all named CTEs.

#### WITH RECURSIVE

`WITH RECURSIVE` allows a CTE to reference itself, enabling iterative traversal of hierarchical or graph-structured data.

The recursive CTE has two parts separated by `UNION ALL`:
1. The **base case** (non-recursive): the initial set of rows.
2. The **recursive step**: references the CTE by name and is evaluated repeatedly until it produces no new rows.

```sql
-- Count down from 10
WITH RECURSIVE countdown(n) AS (
    SELECT 10
    UNION ALL
    SELECT n - 1 FROM countdown WHERE n > 1
)
SELECT n FROM countdown ORDER BY n DESC;
```

```sql
-- Org-chart traversal: find all employees and their reporting level
WITH RECURSIVE org(id, name, level) AS (
    -- Base: top-level employees (no manager)
    SELECT id, name, 0 FROM employees WHERE manager_id IS NULL
    UNION ALL
    -- Recursive: employees whose manager is already in the result
    SELECT e.id, e.name, o.level + 1
    FROM employees e
    JOIN org o ON e.manager_id = o.id
)
SELECT level, name FROM org ORDER BY level, name;
```

Safety: icedb limits recursive CTE iterations to 1,000 to prevent infinite loops. Queries that would require more iterations return an error.

Not yet supported: `SEARCH` and `CYCLE` clauses for controlling traversal order and detecting cycles explicitly.

#### Set Operations — UNION, INTERSECT, EXCEPT

Set operations combine results from two or more `SELECT` statements. The column count and compatible types must match.

```sql
-- All product names plus all category names, deduped
SELECT name FROM products
UNION
SELECT category FROM product_categories;

-- Only names that appear in both result sets
SELECT name FROM products WHERE in_stock = true
INTERSECT
SELECT name FROM products WHERE price < 20.0;

-- Names in the first set but not the second
SELECT name FROM products
EXCEPT
SELECT name FROM discontinued_products;
```

`UNION` and `INTERSECT` and `EXCEPT` remove duplicates by default. Use `UNION ALL` to keep duplicates (faster when you know results are already distinct or you need to count repetitions).

#### Multi-table FROM (Cross Joins)

Listing multiple tables in `FROM` separated by commas produces a cross join (Cartesian product), filtered by any WHERE condition:

```sql
-- Equivalent to: FROM orders o JOIN books b ON o.book_id = b.id
SELECT o.id, b.title
FROM orders o, books b
WHERE o.book_id = b.id;
```

#### JOIN USING

When the join column has the same name in both tables, `JOIN USING` is a compact alternative to `JOIN ... ON`. It also deduplicates the join column in the output (PostgreSQL semantics):

```sql
SELECT book_id, g.name AS genre
FROM book_genres
JOIN genres g USING (genre_id);
```

#### Qualified Column References

In queries involving multiple tables or aliases, columns can be referenced as `alias.column` or `table_name.column`:

```sql
SELECT b.title, a.name AS author
FROM books b
JOIN authors a ON b.author_id = a.id
WHERE b.price > 15.0;
```

### WHERE Clause

Comparison operators: `=`, `<>`, `!=`, `<`, `>`, `<=`, `>=`.

```sql
SELECT * FROM products WHERE price = 9.99;
SELECT * FROM products WHERE name <> 'Widget';
SELECT * FROM products WHERE price >= 5.00 AND price <= 20.00;
```

Logical operators: `AND`, `OR`, `NOT`.

```sql
SELECT * FROM products WHERE (price < 5.0 OR price > 50.0) AND in_stock = true;
```

#### NULL Checks

```sql
SELECT * FROM products WHERE price IS NULL;
SELECT * FROM products WHERE price IS NOT NULL;
```

For boolean columns, `IS UNKNOWN` tests whether the value is NULL (NULL is the SQL "unknown" truth value):

```sql
SELECT * FROM flags WHERE enabled IS UNKNOWN;       -- same as: enabled IS NULL
SELECT * FROM flags WHERE enabled IS NOT UNKNOWN;   -- same as: enabled IS NOT NULL
```

Never use `= NULL` or `<> NULL`. Because NULL represents an unknown value, any comparison with NULL using `=` or `<>` yields NULL (not TRUE), which is treated as false in a WHERE filter. `IS NULL` and `IS NOT NULL` are the correct predicates. See the NULL Semantics section below for a detailed explanation.

#### IS DISTINCT FROM / IS NOT DISTINCT FROM

These predicates compare two values treating NULL as an ordinary comparable value:

- `a IS DISTINCT FROM b` is TRUE when a and b differ, including when one is NULL and the other is not. It is FALSE when both are NULL.
- `a IS NOT DISTINCT FROM b` is the NULL-safe equality check: TRUE when both are NULL or both have the same non-NULL value.

```sql
-- Find rows where price differs from a reference value, even if one side is NULL
SELECT * FROM products WHERE price IS DISTINCT FROM 9.99;

-- Find rows where price equals 9.99, or both are NULL
SELECT * FROM products WHERE price IS NOT DISTINCT FROM NULL;
```

#### ILIKE — Case-Insensitive Pattern Matching

`ILIKE` is the case-insensitive version of `LIKE`. Both use `%` (any sequence of characters) and `_` (any single character) as wildcards.

```sql
-- Matches 'Widget', 'WIDGET', 'widget', etc.
SELECT * FROM products WHERE name ILIKE 'widget%';

-- Match anywhere in the string
SELECT * FROM products WHERE name ILIKE '%pro%';
```

`LIKE` is case-sensitive. `ILIKE` is unique to PostgreSQL-compatible databases (not in standard SQL).

#### IN (subquery) and EXISTS

`IN` with a subquery tests whether a value appears in the subquery result:

```sql
SELECT name FROM products
WHERE id IN (SELECT product_id FROM orders WHERE quantity > 5);
```

`EXISTS` tests whether a subquery returns at least one row. It short-circuits as soon as the first matching row is found:

```sql
SELECT name FROM products p
WHERE EXISTS (
    SELECT 1 FROM orders o WHERE o.product_id = p.id AND o.quantity > 5
);
```

`NOT IN` and `NOT EXISTS` are the negated forms. Note: `NOT IN` with a subquery that can return NULL values requires care — if any value in the subquery result is NULL, the entire `NOT IN` check returns unknown (no rows pass). `NOT EXISTS` is generally safer when NULLs may be present.

#### HAVING — Filtering Aggregate Results

`HAVING` filters groups after `GROUP BY` aggregation, unlike `WHERE` which filters individual rows before aggregation:

```sql
SELECT category, COUNT(*) AS cnt, AVG(price) AS avg_price
FROM products
GROUP BY category
HAVING COUNT(*) > 1 AND AVG(price) < 50.0
ORDER BY cnt DESC;
```

Only aggregate functions and GROUP BY columns may appear in a HAVING condition.

### Conditional Expressions

#### CASE WHEN ... THEN ... ELSE ... END

The searched form evaluates each WHEN condition in order and returns the first matching THEN result. The ELSE branch is returned if no condition matches; if ELSE is omitted and no condition matches, the result is NULL.

```sql
-- Searched CASE: condition in each WHEN
SELECT title,
       CASE WHEN price < 10 THEN 'budget'
            WHEN price < 30 THEN 'mid-range'
            ELSE 'premium'
       END AS tier
FROM books;
```

The simple form compares a single expression against a list of values:

```sql
-- Simple CASE: compare one expression to fixed values
SELECT name,
       CASE country
           WHEN 'United States' THEN 'US'
           WHEN 'United Kingdom' THEN 'UK'
           ELSE 'Other'
       END AS region
FROM authors;
```

#### COALESCE

`COALESCE(expr, expr, ...)` returns the first non-NULL value in its argument list. It is short-circuit: evaluation stops as soon as a non-NULL value is found.

```sql
-- Provide a fallback when phone or email may be NULL
SELECT name, COALESCE(phone, email, 'no contact') AS contact FROM customers;
```

#### NULLIF

`NULLIF(expr1, expr2)` returns NULL if expr1 equals expr2, otherwise returns expr1. This is most commonly used to avoid division-by-zero errors:

```sql
-- Avoid division by zero: if num_orders = 0, the result is NULL
SELECT total_sales / NULLIF(num_orders, 0) AS avg_order_value FROM summary;
```

### String Operators

#### || Concatenation

The `||` operator concatenates two text values. If either operand is NULL, the result is NULL. Use COALESCE to guard against NULL operands.

```sql
-- Concatenate first and last name
SELECT first_name || ' ' || last_name AS full_name FROM authors;

-- Build a greeting string
SELECT 'Hello, ' || name || '!' AS greeting FROM users;

-- Guard a nullable column with COALESCE
SELECT COALESCE(middle_name, '') || ' ' || last_name AS display FROM people;
```

### Arithmetic Operators

Standard arithmetic operators work on numeric types (`INT`, `BIGINT`, `FLOAT`):

| Operator | Description | Example |
|----------|-------------|---------|
| `+` | Addition | `price + 1.00` |
| `-` | Subtraction | `salary - bonus` |
| `*` | Multiplication | `price * 1.20` |
| `/` | Division (integer division truncates toward zero for integers) | `total / quantity` |
| `%` | Modulo (remainder after integer division) | `id % 2` |

```sql
-- Even/odd check using modulo
SELECT name, id % 2 AS is_odd FROM products;

-- Hourly rate calculation
SELECT name, salary, salary / 2080 AS hourly_rate FROM employees;

-- Assign rows to 4 buckets
SELECT id, id % 4 AS bucket FROM items ORDER BY id % 4, id;
```

Overflow is checked: `SELECT 2147483647 + 1` returns `SQLSTATE 22003`. Division or modulo by zero returns `SQLSTATE 22012`.

### String Functions

In addition to `||` concatenation, icedb provides the following string functions:

| Function | Description | Example |
|----------|-------------|---------|
| `UPPER(s)` | Convert to upper case | `UPPER('hello')` → `'HELLO'` |
| `LOWER(s)` | Convert to lower case | `LOWER('WORLD')` → `'world'` |
| `LENGTH(s)` | Number of characters | `LENGTH('abc')` → `3` |
| `TRIM(s)` | Remove leading and trailing spaces | `TRIM('  hi  ')` → `'hi'` |
| `LTRIM(s)` | Remove leading spaces | `LTRIM('  hi')` → `'hi'` |
| `RTRIM(s)` | Remove trailing spaces | `RTRIM('hi  ')` → `'hi'` |
| `SUBSTRING(s, start, len)` | Extract substring (1-based) | `SUBSTRING('hello', 2, 3)` → `'ell'` |
| `POSITION(needle IN haystack)` | 1-based position, 0 if not found | `POSITION('ll' IN 'hello')` → `3` |
| `STRPOS(haystack, needle)` | Same as POSITION, alternate form | `STRPOS('hello', 'll')` → `3` |
| `REPLACE(s, from, to)` | Replace all occurrences | `REPLACE('foo bar', 'bar', 'baz')` → `'foo baz'` |
| `CONCAT(s, ...)` | Concatenate, NULL-safe (NULLs become empty string) | `CONCAT('a', NULL, 'b')` → `'ab'` |
| `REPEAT(s, n)` | Repeat string n times | `REPEAT('ab', 3)` → `'ababab'` |
| `REVERSE(s)` | Reverse characters | `REVERSE('abc')` → `'cba'` |
| `LPAD(s, n, fill)` | Pad left to width n | `LPAD('5', 3, '0')` → `'005'` |
| `RPAD(s, n, fill)` | Pad right to width n | `RPAD('hi', 5, '.')` → `'hi...'` |
| `SPLIT_PART(s, delim, n)` | Split on delimiter, return nth field | `SPLIT_PART('a,b,c', ',', 2)` → `'b'` |
| `LEFT(s, n)` | First n characters | `LEFT('hello', 3)` → `'hel'` |
| `RIGHT(s, n)` | Last n characters | `RIGHT('hello', 3)` → `'llo'` |

```sql
-- Clean up dirty data
SELECT TRIM(LOWER(name)) AS clean_name FROM users;

-- Extract domain from email
SELECT SPLIT_PART(email, '@', 2) AS domain FROM users;

-- Find rows where a substring appears
SELECT title FROM books WHERE POSITION('the' IN LOWER(title)) > 0;

-- Pad numeric codes to a fixed width
SELECT LPAD(id::TEXT, 6, '0') AS padded_id FROM orders;
```

### ORDER BY

```sql
SELECT name, price FROM products ORDER BY price ASC;
SELECT name, price FROM products ORDER BY price DESC;
SELECT name, price FROM products ORDER BY name ASC, price DESC;
```

`ASC` is the default and can be omitted. NULL values sort before non-NULL values in ascending order.

### LIMIT, OFFSET, and FETCH FIRST

`LIMIT` and `OFFSET` restrict which rows are returned. `FETCH FIRST` is the SQL-standard equivalent of `LIMIT`.

```sql
-- Return at most 10 rows
SELECT * FROM products ORDER BY id LIMIT 10;

-- Skip the first 20, return the next 10
SELECT * FROM products ORDER BY id LIMIT 10 OFFSET 20;

-- SQL-standard equivalent of LIMIT (both forms are accepted)
SELECT * FROM products ORDER BY id FETCH FIRST 10 ROWS ONLY;
SELECT * FROM products ORDER BY id FETCH FIRST 1 ROW ONLY;
```

`FETCH FIRST` and `LIMIT` are interchangeable. `OFFSET` can be combined with `FETCH FIRST`:

```sql
SELECT * FROM products ORDER BY price DESC OFFSET 5 FETCH FIRST 10 ROWS ONLY;
```

### GROUP BY and Aggregates

```sql
SELECT category, COUNT(*), AVG(price)
FROM products
GROUP BY category
ORDER BY COUNT(*) DESC;
```

Supported aggregate functions:

| Function | Description |
|----------|-------------|
| `COUNT(*)` | Count all rows in the group |
| `COUNT(column)` | Count non-null values |
| `COUNT(DISTINCT column)` | Count distinct non-null values |
| `SUM(column)` | Sum of numeric values |
| `AVG(column)` | Arithmetic mean |
| `MIN(column)` | Minimum value |
| `MAX(column)` | Maximum value |

Aggregates can appear in SELECT expressions. All non-aggregate columns in SELECT must appear in GROUP BY.

### JOINs

**INNER JOIN** (also spelled `JOIN`): returns only rows that match in both tables.

```sql
SELECT o.id, c.name, o.total
FROM orders o
JOIN customers c ON o.customer_id = c.id;
```

**LEFT JOIN** (also `LEFT OUTER JOIN`): returns all rows from the left table. Unmatched right-table columns are filled with NULL.

```sql
SELECT c.name, o.id AS order_id
FROM customers c
LEFT JOIN orders o ON o.customer_id = c.id;
```

**RIGHT JOIN** (also `RIGHT OUTER JOIN`): like LEFT JOIN but keeps all unmatched rows from the right table, NULL-filling the left side.

```sql
SELECT c.name, o.id AS order_id
FROM orders o
RIGHT JOIN customers c ON o.customer_id = c.id;
```

**FULL JOIN** (also `FULL OUTER JOIN`): keeps unmatched rows from both the left and the right tables. Columns from the side that has no match are NULL.

```sql
SELECT a.name AS author, b.title
FROM authors a
FULL JOIN books b ON b.author_id = a.id
ORDER BY a.name;
```

**CROSS JOIN**: produces the Cartesian product of both tables — every combination of rows. No ON or USING clause is used.

```sql
-- Pair every size with every color
SELECT s.label AS size, c.name AS color
FROM sizes s
CROSS JOIN colors c;
```

**Multi-condition JOIN**: the ON clause may combine multiple predicates with AND.

```sql
-- Match on two columns simultaneously
SELECT *
FROM shipments s
JOIN warehouses w ON s.warehouse_id = w.id AND s.region = w.region;
```

**JOIN USING**: when the join column has the same name in both tables, `JOIN USING` deduplicates the column in the output (PostgreSQL semantics). The join column appears only once in the result even though it exists in both tables.

```sql
SELECT book_id, g.name AS genre
FROM book_genres
JOIN genres g USING (genre_id);
```

Multi-table joins:

```sql
SELECT o.id, c.name, p.name AS product, o.quantity
FROM orders o
JOIN customers c ON o.customer_id = c.id
JOIN products p ON o.product_id = p.id
WHERE o.quantity > 1;
```

#### LATERAL Joins

A `LATERAL` subquery in the `FROM` clause can reference columns from tables that appear earlier in the same `FROM` list. This makes it possible to run a correlated subquery once per outer row and join the results inline — similar to a correlated subquery in `WHERE`, but it can return multiple rows and columns.

```sql
-- For each author, get their most expensive book
SELECT a.name, recent.title, recent.price
FROM authors a
JOIN LATERAL (
    SELECT title, price
    FROM books b
    WHERE b.author_id = a.id
    ORDER BY price DESC
    LIMIT 1
) AS recent ON true
ORDER BY a.name;
```

```sql
-- Expand a fixed set of offsets per group (cross join lateral)
SELECT a.name, offsets.n
FROM authors a
CROSS JOIN LATERAL (VALUES (1), (2), (3)) AS offsets(n)
ORDER BY a.name, offsets.n;
```

Key points:
- Use `JOIN LATERAL ... ON true` when the lateral subquery always returns rows you want to keep.
- Use `LEFT JOIN LATERAL ... ON true` to preserve outer rows even when the lateral subquery returns no rows (NULL-filled columns).
- The lateral subquery is re-evaluated for each outer row; it is not cached.
- `LATERAL` is implicit for functions that appear in `FROM` (but explicit `LATERAL` is required for subqueries).

### Subqueries

Subqueries in WHERE with `IN` and comparison operators:

```sql
SELECT name FROM products
WHERE id IN (SELECT product_id FROM orders WHERE quantity > 10);

SELECT name FROM products
WHERE price > (SELECT AVG(price) FROM products);
```

Subqueries are evaluated as part of the filter expression. The result is computed once and compared against each outer row.

#### Correlated Subqueries

A correlated subquery references a column from the outer query. It is re-evaluated for each outer row, making it flexible but potentially slow on large tables.

```sql
-- EXISTS: find authors who have at least one book
SELECT a.name
FROM authors a
WHERE EXISTS (
    SELECT 1 FROM books b WHERE b.author_id = a.id
);

-- NOT EXISTS: find authors with no books
SELECT a.name
FROM authors a
WHERE NOT EXISTS (
    SELECT 1 FROM books b WHERE b.author_id = a.id
);

-- Correlated IN
SELECT o.id
FROM orders o
WHERE o.customer_id IN (
    SELECT id FROM customers c WHERE c.country = 'US'
);
```

Note: correlated subqueries are re-evaluated for each outer row. For large tables, consider rewriting as a JOIN for better performance.

### Window Functions

Window functions compute a value for each row based on a set of related rows (the "window"), without collapsing them into a single aggregate row. They require an `OVER` clause.

```sql
-- ROW_NUMBER: unique sequential number within each partition
SELECT name, dept, salary,
       ROW_NUMBER() OVER (PARTITION BY dept ORDER BY salary DESC) AS rank_in_dept
FROM employees;

-- RANK: same value = same rank, with gaps after ties
SELECT name, score,
       RANK() OVER (ORDER BY score DESC) AS rank
FROM leaderboard;

-- DENSE_RANK: same value = same rank, no gaps
SELECT name, score,
       DENSE_RANK() OVER (ORDER BY score DESC) AS dense_rank
FROM leaderboard;

-- Aggregate window function: SUM over a partition without collapsing rows
SELECT name, dept, salary,
       SUM(salary) OVER (PARTITION BY dept) AS dept_total
FROM employees;
```

**Supported window functions:** `ROW_NUMBER()`, `RANK()`, `DENSE_RANK()`, `SUM()`, `AVG()`, `MIN()`, `MAX()`, `COUNT()`, `LEAD()`, `LAG()`, `FIRST_VALUE()`, `LAST_VALUE()`, `NTH_VALUE()`, `CUME_DIST()`, `PERCENT_RANK()`, `NTILE()`.

**Not yet supported:** window frame clauses (`ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW` and similar).

### COUNT(DISTINCT ...)

`COUNT(DISTINCT column)` counts only the unique non-NULL values in the column.

```sql
-- Count unique categories
SELECT COUNT(DISTINCT category) AS unique_categories FROM products;

-- Count distinct values per group
SELECT dept, COUNT(DISTINCT job_title) AS unique_roles
FROM employees
GROUP BY dept;
```

---

## ALTER TABLE

`ALTER TABLE` modifies an existing table's schema. Changes take effect immediately.

```sql
-- Add a column (existing rows receive NULL for the new column)
ALTER TABLE employees ADD COLUMN department TEXT;

-- Drop a column
ALTER TABLE employees DROP COLUMN old_notes;

-- Rename a column
ALTER TABLE employees RENAME COLUMN dept TO department;

-- Rename a table
ALTER TABLE old_name RENAME TO new_name;
```

---

## Access Control

### GRANT

Grants one or more privileges on a table to a role.

```sql
GRANT privilege [, ...] ON table_name TO role_name;
```

Supported privileges: `SELECT`, `INSERT`, `UPDATE`, `DELETE`, `ALL` (or `ALL PRIVILEGES`).

```sql
-- Allow a reporting role to read two tables
GRANT SELECT ON books TO reporter;
GRANT SELECT ON authors TO reporter;

-- Allow an application role full DML access
GRANT SELECT, INSERT, UPDATE, DELETE ON books TO app_role;

-- Grant all privileges at once
GRANT ALL ON books TO app_role;
```

### REVOKE

Removes previously granted privileges.

```sql
REVOKE privilege [, ...] ON table_name FROM role_name;
```

```sql
REVOKE INSERT ON books FROM app_role;
```

ACL entries are persisted to `{data_dir}/acls/{schema}_{table}.acl` as JSON files and survive server restarts.

**Privilege check order:** superusers bypass all privilege checks. If no role is configured (embedded/bootstrap mode), all operations are permitted. For regular roles, the ACL is checked and an error is returned if the required privilege is absent.

---

## LISTEN / NOTIFY — Event Notifications

icedb supports PostgreSQL-compatible pub/sub notifications.

```sql
LISTEN channel_name;
NOTIFY channel_name, 'optional payload';
UNLISTEN channel_name;
UNLISTEN *;           -- stop listening on all channels
```

**Notes:**
- Channel names are case-sensitive
- Payload is optional — omit for a bare notification
- In embedded mode, notifications are delivered synchronously within the same process
- Over the wire protocol, NOTIFY delivers notifications to all sessions listening on the channel

### Example

```sql
LISTEN order_updates;
-- In another session / from application code:
NOTIFY order_updates, 'new order id=1234';
```

---

## User-Defined Functions (SQL Language)

icedb supports creating SQL-language functions using dollar-quoted strings.

### CREATE FUNCTION

```sql
CREATE FUNCTION function_name(param1 type1, param2 type2, ...)
    RETURNS return_type
    LANGUAGE SQL
AS $$
    SELECT expression;
$$;
```

### DROP FUNCTION

```sql
DROP FUNCTION function_name;
DROP FUNCTION IF EXISTS function_name;
```

### Example

```sql
-- Simple arithmetic function
CREATE FUNCTION add_tax(price FLOAT, rate FLOAT)
    RETURNS FLOAT
    LANGUAGE SQL
AS $$
    SELECT $1 * (1 + $2)
$$;

SELECT add_tax(100.0, 0.08);  -- 108.0

-- Function using a table
CREATE FUNCTION active_user_count()
    RETURNS INT
    LANGUAGE SQL
AS $$
    SELECT COUNT(*) FROM users WHERE active = true
$$;

SELECT active_user_count();
```

**Limitations:**
- Only `LANGUAGE SQL` is supported (PL/pgSQL is not yet implemented)
- Functions must return a single scalar value
- No function overloading by arity yet
- Functions are not persisted across server restarts (in-memory only)

---

## VACUUM

Reclaims space from dead tuples (rows that were deleted or updated but whose storage was not yet freed). VACUUM scans each page, identifies dead tuples (those with a committed `t_xmax`), marks their item slots as dead, and updates `pd_prune_xid` on modified pages.

```sql
-- Vacuum a specific table
VACUUM books;

-- Vacuum all user tables
VACUUM;

-- Vacuum and update statistics
VACUUM ANALYZE books;
VACUUM ANALYZE;
```

VACUUM does **not** require an exclusive lock and can run concurrently with queries. In this version, page compaction (defragmentation) is not performed — dead slots are marked but not physically reclaimed. `VACUUM ANALYZE` logs that statistics are not yet collected (a TODO for a future version).

---

## Transaction Control

### BEGIN

Starts an explicit transaction. Without `BEGIN`, each statement is auto-committed in its own transaction at `READ COMMITTED` isolation.

```sql
BEGIN;
```

To specify an isolation level:

```sql
BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ;
BEGIN TRANSACTION ISOLATION LEVEL SERIALIZABLE;
```

The default when `BEGIN` is used without a level is `READ COMMITTED`.

### COMMIT

Persists all changes made in the current transaction to durable storage. The WAL COMMIT record is fsynced before the command returns.

```sql
COMMIT;
```

### ROLLBACK

Discards all changes made in the current transaction. All tuple versions written by the transaction are hidden from future snapshots (their `t_xmin` XID is in the aborted set).

```sql
ROLLBACK;
```

### SAVEPOINT

Savepoints let you mark a point inside a transaction so that a partial rollback is possible without aborting the entire transaction.

```sql
BEGIN;
INSERT INTO accounts VALUES (1, 1000);
SAVEPOINT sp1;
INSERT INTO accounts VALUES (2, 500);    -- this will be undone
ROLLBACK TO SAVEPOINT sp1;               -- rolls back to sp1
INSERT INTO accounts VALUES (3, 250);    -- this replaces the undone insert
COMMIT;
```

```sql
-- Release a savepoint (frees the name but does not roll back)
SAVEPOINT sp2;
RELEASE SAVEPOINT sp2;
```

> **Implementation note:** icedb does not implement page-level undo. `ROLLBACK TO SAVEPOINT` aborts the entire current transaction and starts a new one. Changes made before the savepoint are lost. This behaviour differs from PostgreSQL, where pre-savepoint changes survive. The SAVEPOINT/RELEASE commands are accepted and tracked, but the partial-rollback guarantee requires true undo support, which is planned for a future release.

### SET TRANSACTION

`SET TRANSACTION` can be used after `BEGIN` and before the first data-modifying statement to declare the isolation level for the current transaction.

```sql
BEGIN;
SET TRANSACTION ISOLATION LEVEL REPEATABLE READ;
SELECT ...;
COMMIT;

-- Combined form (equivalent):
BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ;
```

> **Implementation note:** `SET TRANSACTION` is accepted and parsed without error. In the current version, the isolation level is applied at `BEGIN` time; `SET TRANSACTION` after `BEGIN` is accepted but has no additional effect because the snapshot has already been taken.

### Isolation Levels

| Level | SQL syntax | Snapshot behavior |
|-------|------------|-------------------|
| Read Committed | `BEGIN TRANSACTION ISOLATION LEVEL READ COMMITTED` | Fresh snapshot per statement |
| Repeatable Read | `BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ` | Single snapshot taken at BEGIN |
| Serializable | `BEGIN TRANSACTION ISOLATION LEVEL SERIALIZABLE` | Single snapshot + SSI conflict tracking |

See Chapter 5 for a detailed explanation of what each level prevents.

---

## NULL Semantics

NULL represents an absent or unknown value. Its behavior can be surprising if you are used to languages where "unknown" and "false" are the same thing.

### NULL Propagates Through Expressions

Any arithmetic or comparison expression that has a NULL operand yields NULL:

```
NULL + 5       => NULL
NULL * 0       => NULL   (not 0!)
NULL = NULL    => NULL   (not TRUE)
NULL <> NULL   => NULL   (not FALSE)
NULL = 'foo'   => NULL
```

In a WHERE clause, only rows where the condition evaluates to TRUE are returned. A NULL condition is treated as "not TRUE", so those rows are silently excluded:

```sql
-- This returns zero rows even if some prices ARE 9.99
-- because NULL = 9.99 evaluates to NULL, which is not TRUE
SELECT * FROM products WHERE price = NULL;
```

Always use `IS NULL` or `IS NOT NULL` to test for NULL:

```sql
SELECT * FROM products WHERE price IS NULL;
SELECT * FROM products WHERE price IS NOT NULL;
```

### NULL in Aggregates

Aggregate functions (`SUM`, `AVG`, `MIN`, `MAX`, `COUNT(column)`) ignore NULL values:

```sql
-- If three rows have price = NULL, they are excluded from the average
SELECT AVG(price) FROM products;

-- COUNT(*) counts all rows; COUNT(price) counts only non-NULL price rows
SELECT COUNT(*), COUNT(price) FROM products;
```

`COUNT(*)` always returns the total row count. `COUNT(column)` returns the number of non-NULL values in that column.

### NULL-Safe Equality

Use `IS NOT DISTINCT FROM` when you need equality that treats NULL as equal to NULL:

```sql
-- True when both columns are NULL, or both have the same non-NULL value
SELECT * FROM products WHERE price IS NOT DISTINCT FROM NULL;
```

### NULL in Set Operations

`UNION`, `INTERSECT`, and `EXCEPT` treat NULL as equal to NULL for the purpose of deduplication: two rows that are both NULL in the same column position are considered duplicates.

### NULL Ordering

In `ORDER BY`, NULL values sort before non-NULL values in ascending order (PostgreSQL default). They sort after non-NULL values in descending order.

---

## information_schema Views

icedb implements the standard `information_schema` views for catalog introspection. These are compatible with PostgreSQL and work with tools like DBeaver and pgAdmin.

| View | Description |
|------|-------------|
| `information_schema.tables` | All tables and views in the database |
| `information_schema.columns` | All columns with type information |
| `information_schema.schemata` | All schemas |
| `information_schema.table_constraints` | PK, FK, UNIQUE, CHECK constraints |
| `information_schema.key_column_usage` | Columns participating in constraints |
| `information_schema.referential_constraints` | Foreign key details |

### Examples

```sql
-- List all user tables
SELECT table_name, table_type
FROM information_schema.tables
WHERE table_schema = 'public';

-- List columns of a specific table
SELECT column_name, data_type, is_nullable
FROM information_schema.columns
WHERE table_name = 'orders'
ORDER BY ordinal_position;

-- List all constraints
SELECT constraint_name, constraint_type, table_name
FROM information_schema.table_constraints
WHERE table_schema = 'public';

-- Check foreign key relationships
SELECT constraint_name, table_name, column_name
FROM information_schema.key_column_usage
WHERE table_name = 'orders';
```

---

## pg_catalog Views

icedb also implements PostgreSQL's `pg_catalog` system views:

| View | Description |
|------|-------------|
| `pg_catalog.pg_tables` | Table listing with owner info |
| `pg_catalog.pg_class` | Table/index OID registry |
| `pg_catalog.pg_attribute` | Column definitions |
| `pg_catalog.pg_namespace` | Schema listing |
| `pg_catalog.pg_indexes` | Index information |
| `pg_catalog.pg_type` | Data type registry |
| `pg_catalog.pg_roles` | Role definitions |
| `pg_catalog.pg_views` | View definitions |
| `pg_catalog.pg_stat_user_tables` | Per-table statistics (stub) |

### Examples

```sql
-- List tables via pg_catalog
SELECT schemaname, tablename FROM pg_catalog.pg_tables
WHERE schemaname = 'public';

-- List columns via pg_attribute
SELECT attname, atttypid
FROM pg_catalog.pg_attribute
WHERE attrelid = (
    SELECT oid FROM pg_catalog.pg_class WHERE relname = 'orders'
);

-- List indexes
SELECT tablename, indexname
FROM pg_catalog.pg_indexes
WHERE schemaname = 'public';
```

These views enable compatibility with database management tools like DBeaver, pgAdmin, and TablePlus that use them for schema browsing.

---

## Error Codes (SQLSTATE)

icedb returns standard five-character SQLSTATE codes on all errors. These match the PostgreSQL SQLSTATE codes so that client libraries and ORMs that parse error codes work correctly.

| SQLSTATE | Condition Name | Returned When |
|----------|---------------|---------------|
| `00000` | `successful_completion` | Statement succeeded with no warnings |
| `22003` | `numeric_value_out_of_range` | Integer or float arithmetic overflows the type's range |
| `22012` | `division_by_zero` | Division or modulo by zero |
| `22004` | `null_value_not_allowed` | NULL inserted into a NOT NULL column |
| `42601` | `syntax_error` | SQL could not be parsed |
| `42703` | `undefined_column` | Column name not found in any referenced table |
| `42P01` | `undefined_table` | Table name not found in the catalog |
| `42702` | `ambiguous_column` | Column name matches more than one table in scope |
| `23502` | `not_null_violation` | NOT NULL constraint violation |
| `23000` | `integrity_constraint_violation` | UNIQUE, PRIMARY KEY, or CHECK constraint violated |
| `40001` | `serialization_failure` | Serializable transaction aborted due to conflict (SSI) |
| `3D000` | `invalid_catalog_name` | Database name in connection request does not exist |
| `57014` | `query_canceled` | Query canceled by the client |
| `XX000` | `internal_error` | Unexpected internal error (should be reported as a bug) |

SQLSTATE codes are included in the `ErrorResponse` wire protocol message (field `C`). Standard clients expose these as a `.code` or `.sqlstate` property on error objects.

### Arithmetic Safety

icedb performs overflow-safe arithmetic using Rust's checked operations. Arithmetic that would overflow the column's integer type returns `SQLSTATE 22003` rather than silently wrapping around or producing an incorrect result:

```sql
-- i32 max is 2,147,483,647
SELECT 2147483647 + 1;
-- ERROR 22003: integer out of range

SELECT 2000000000 * 2;
-- ERROR 22003: integer out of range
```

Division by zero returns `SQLSTATE 22012`:

```sql
SELECT 10 / 0;
-- ERROR 22012: division by zero
```

---

## Unsupported Features

The following features are **not yet implemented** in the current version. They are listed honestly so you can make an informed decision about whether icedb is appropriate for your use case. For a detailed explanation of *why* each item is missing and what is required to add it, see [Chapter 14 — Roadmap & Known Limitations](ch14-roadmap.md).

**DDL:**
- `ALTER TABLE` operations beyond ADD/DROP/RENAME COLUMN and RENAME TABLE (e.g., `ALTER COLUMN TYPE`, constraint changes, `SET DEFAULT`)
- Column-level `GRANT`/`REVOKE` (only table-level privileges are supported)
- Named indexes (`CREATE INDEX idx_name ON ...` — index name is ignored; derived from table OID and column)
- `CREATE SEQUENCE` (use `SERIAL`/`BIGSERIAL` instead)
- Table partitioning (`PARTITION BY RANGE/LIST/HASH`)
- Tablespaces

**Advanced query features:**
- `WITH RECURSIVE … SEARCH` and `CYCLE` clauses
- `NATURAL JOIN` (join on all identically-named columns)
- `ROLLUP`, `CUBE`, `GROUPING SETS` (multi-dimensional aggregation)
- Full window frame clauses (`ROWS BETWEEN … AND …`); basic `OVER (PARTITION BY … ORDER BY …)` works
- PL/pgSQL stored procedures (only `LANGUAGE SQL` functions are supported)

**System queries:**
- Some `pg_catalog` and `information_schema` columns return stub/zero values (statistics, replication state, OID cross-references)
- `VACUUM ANALYZE` — `VACUUM` works; the `ANALYZE` pass does not yet update `pg_statistic` histograms

**Operations:**
- Physical and logical replication
- Connection limit enforcement and graceful `SIGTERM` shutdown
- SSL/TLS on the wire

If a feature you need is not listed here but is also not documented above, the safe assumption is that it is not implemented. Unsupported SQL will return an error; always verify behaviour against the test suite.
