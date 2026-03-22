# Chapter 8: CLI Reference (nkv-psql)

The `nkv-psql` CLI is icedb's interactive terminal. It provides a `rustyline`-based REPL with persistent history, tab completion, meta-commands for inspecting the database, and result rendering as ASCII tables. The CLI (nkv-psql) runs the storage engine in-process against the data directory — no separate server process or TCP connection is needed.

**In this chapter:**
- Starting the CLI and command-line flags
- The prompt and multiline input
- Command history and tab completion
- Timing queries (`\timing`) and expanded output (`\x`)
- Meta-commands reference (`\d`, `\dt`, `\du`, `\l`, `\c`, `\?`)
- Output formatting and error handling

## Starting the CLI

### Basic Usage

```sh
# Development build
cargo run -p cli -- --data-dir ./mydata

# Release build (faster startup)
cargo run -p cli --release -- --data-dir ./mydata

# If you have installed the binary
nkv-psql --data-dir /var/lib/icedb/data
```

The `--data-dir` flag specifies the data directory (default: `./data`). If the directory does not exist, the CLI creates it and bootstraps a fresh database.

### Command-Line Flags

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--data-dir DIR` | | `./data` | Path to the icedb data directory |
| `--user USER` | `-U` | `icedb` or `$PGUSER` | Username |
| `--dbname DB` | `-d` | `icedb` or `$PGDATABASE` | Database to open on startup; must exist in the data directory |
| `--help` | `-h` | | Print usage and exit |

### Environment Variables

| Variable | Description |
|----------|-------------|
| `PGUSER` | Default username if `--user` is not specified |
| `PGDATABASE` | Default database name if `--dbname` is not specified |

### Usage Examples

```sh
# Open the database at ./data as user icedb
cargo run -p cli -- --data-dir ./data

# Open as a specific user (informational in current version)
cargo run -p cli -- --data-dir ./data --user alice

# Using environment variables
PGUSER=alice cargo run -p cli -- --data-dir ./data
```

## The Prompt

After connecting, you see the prompt:

```
icedb=#
```

The format is `<dbname>=# ` in single-line mode. When a SQL statement spans multiple lines (the engine has not yet seen a semicolon), the continuation prompt changes to:

```
icedb-#
```

This signals that the CLI is accumulating more input before sending the statement.

**Example of multiline input:**

```
icedb=# SELECT id, name
icedb-#   FROM authors
icedb-#   WHERE id > 1
icedb-#   ORDER BY name;
```

The statement is sent to the engine only when the semicolon is entered. Until then, all continuation lines are buffered locally.

## Entering SQL

SQL statements are terminated by a semicolon (`;`). Statements can span multiple lines. The CLI accumulates all lines until a semicolon is found, then sends the complete statement to the engine.

```sql
-- Single-line statement
SELECT * FROM books;

-- Multi-line statement
SELECT b.title, a.name
FROM books b
JOIN authors a ON b.author_id = a.id
WHERE b.price < 15.00
ORDER BY b.title;
```

Multiple statements in one submission (separated by semicolons) are supported:

```sql
INSERT INTO authors VALUES (10, 'Isaac Asimov', 'United States');
INSERT INTO books VALUES (10, 'Foundation', 10, 8.99, 1951);
SELECT * FROM books WHERE author_id = 10;
```

All three statements are executed in order. Results from the last SELECT are displayed.

## Command History

Command history is persisted across sessions. Each completed statement (sent with a semicolon) is saved to `~/.nkv_psql_history`. The history file is read on startup, so commands from previous sessions are available via the up/down arrow keys.

The CLI uses `rustyline` for line editing, which provides:
- Up/Down arrows: navigate history
- Ctrl-R: reverse history search
- Ctrl-A / Home: go to beginning of line
- Ctrl-E / End: go to end of line
- Ctrl-K: delete from cursor to end of line
- Ctrl-U: delete from cursor to beginning of line
- Tab: trigger completion

## Tab Completion

Pressing Tab triggers completion. The CLI provides completions for:

- **SQL keywords**: `SELECT`, `INSERT`, `UPDATE`, `DELETE`, `CREATE`, `DROP`, `FROM`, `WHERE`, `ORDER`, `BY`, `GROUP`, `JOIN`, `ON`, `AND`, `OR`, `NOT`, `NULL`, `LIMIT`, `OFFSET`, `BEGIN`, `COMMIT`, `ROLLBACK`, and more.
- **Table names**: resolved from the catalog at completion time.

Completion is case-insensitive. Type `sel` and press Tab to expand to `SELECT`.

## Timing Queries

Toggle query timing with `\timing`:

```
icedb=# \timing
Timing is on.

icedb=# SELECT COUNT(*) FROM orders;
 COUNT(*)
----------
      500
(1 row)
Time: 12.8 ms

icedb=# \timing
Timing is off.
```

When timing is on, each query's wall-clock execution time is printed after the result. Toggle it off with another `\timing`.

## Expanded Output

For tables with many columns, the normal tabular output can be hard to read. The `\x` command toggles expanded output, which displays each row as a vertical list of `column: value` pairs:

```
icedb=# \x
Expanded display is on.

icedb=# SELECT * FROM books WHERE id = 1;
-[ RECORD 1 ]---+---------------------------
id              | 1
title           | The Hobbit
author_id       | 1
price           | 12.99
published       | 1937

icedb=# \x
Expanded display is off.
```

Expanded output is particularly useful for tables with more than 5–6 columns or with wide text values.

## Meta-Commands Reference

Meta-commands begin with a backslash (`\`). They are processed locally by the CLI, not sent to the SQL engine.

| Command | Description |
|---------|-------------|
| `\q` | Quit nkv-psql |
| `\quit` | Quit nkv-psql (alias for `\q`) |
| `\d` | List all tables in the `public` schema |
| `\dt` | List all tables in the `public` schema (same as `\d`) |
| `\d tablename` | Describe the columns of `tablename` |
| `\du` | List roles |
| `\l` | List all databases |
| `\c [DBNAME]` | Connect to a different database |
| `\connect [DBNAME]` | Connect to a different database (alias for `\c`) |
| `\timing` | Toggle query timing display |
| `\x` | Toggle expanded (vertical) output |
| `\dump path` | Write all table schemas and data to `path` as SQL statements |
| `\restore path` | Execute all SQL statements in `path` against the current database |
| `\?` | Show this help |
| `\help` | Show this help (alias for `\?`) |

### `\d` — List Tables

```
icedb=# \d
 Schema |   Name    | Type
--------+-----------+-------
 public | authors   | table
 public | books     | table
 public | orders    | table
```

### `\d tablename` — Describe a Table

```
icedb=# \d books
Table "public.books"
  Column    |  Type   | Nullable
------------+---------+---------
 id         | int4    | not null
 title      | text    | not null
 author_id  | int4    | not null
 price      | float8  |
 published  | int4    |
```

The Nullable column shows `not null` for columns declared with `NOT NULL`, and blank for nullable columns.

### `\du` — List Roles

```
icedb=# \du
                                   List of roles
 Role name |  Attributes
-----------+--------------
 icedb     | Superuser
```

### `\l` — List Databases

Lists all databases registered in the data directory. The list is read from `pg_database.json` in the data directory.

```
icedb=# \l
                                  List of databases
   Name      |  Owner
-------------+----------
 icedb       | icedb
 analytics   | icedb
 staging     | icedb
```

The `icedb` database is always present — it is the default database created on first startup.

### `\c` / `\connect` — Connect to a Different Database

Switches the active database without restarting the CLI. After switching, the prompt updates to reflect the new database name.

```
icedb=# \c analytics
You are now connected to database "analytics".
analytics=# SELECT * FROM page_views LIMIT 5;
```

If the database does not exist, an error is printed and the current connection is unchanged:

```
icedb=# \c nonexistent
ERROR: database "nonexistent" does not exist
icedb=#
```

You can also use the long form:

```
icedb=# \connect staging
You are now connected to database "staging".
staging=#
```

**Typical workflow** when working with multiple databases:

```
-- Create a new database
icedb=# CREATE DATABASE myapp;
CREATE DATABASE

-- Switch to it
icedb=# \c myapp
You are now connected to database "myapp".

-- Create tables in the new database
myapp=# CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT UNIQUE NOT NULL);
CREATE TABLE

-- Switch back
myapp=# \c icedb
You are now connected to database "icedb".
icedb=#
```

### `\dump` — Dump Database to File

```
icedb=# \dump /backups/mydb.sql
Dumped 312 statements to /backups/mydb.sql
```

Writes `CREATE TABLE IF NOT EXISTS` statements followed by `INSERT INTO` statements for all user tables in the `public` schema. The output file is valid SQL that can be executed with `\restore` or fed to any PostgreSQL-compatible tool.

Note: indexes, roles, sequences, and ACL grants are not included in the dump.

### `\restore` — Restore from File

```
icedb=# \restore /backups/mydb.sql
Restored 312 statements from /backups/mydb.sql
```

Reads and executes each SQL statement in the file sequentially. Errors in individual statements are printed but do not stop the restore; subsequent statements continue to execute.

### `\?` — Help

```
icedb=# \?
General
  \q             quit nkv-psql
  \?             show this help

Connection
  \c [DBNAME]    connect to new database (default: current)
  \connect       alias for \c

Informational
  \d [NAME]      describe table, or list all tables
  \dt            list tables
  \du            list roles
  \l             list databases

Formatting
  \timing        toggle timing of commands
  \x             toggle expanded output

Import/Export
  \dump PATH     dump all schemas and data to PATH as SQL
  \restore PATH  execute all SQL statements in PATH
```

## Prepared Statements in the CLI

The CLI supports the full `PREPARE` / `EXECUTE` / `DEALLOCATE` workflow interactively:

```
icedb=# PREPARE find_book AS SELECT title, price FROM books WHERE id = $1;
PREPARE

icedb=# EXECUTE find_book(1);
     title     | price
---------------+-------
 The Hobbit    | 12.99
(1 row)

icedb=# EXECUTE find_book(3);
 title | price
-------+-------
 Dune  | 15.99
(1 row)

icedb=# DEALLOCATE find_book;
DEALLOCATE
```

Prepared statements are session-scoped and are lost when the CLI exits.

## Output Formatting

Query results are displayed as aligned ASCII tables:

```
 id |          title            | price
----+---------------------------+-------
  1 | The Hobbit                | 12.99
  2 | The Lord of the Rings     | 24.99
  3 | Dune                      | 17.58
(3 rows)
```

Column widths are determined by the maximum width of the column header or any value in that column. Values are left-aligned for text types and right-aligned for numeric types.

NULL values are displayed as `NULL` in the output.

Boolean values display as `t` (true) and `f` (false), following PostgreSQL convention.

BYTEA values display as `\x` followed by the hex-encoded bytes: e.g., `\xdeadbeef`.

DML command tags are displayed without a table:

```
icedb=# INSERT INTO books VALUES (8, 'Neuromancer', 5, 12.00, 1984);
INSERT 0 1

icedb=# UPDATE books SET price = 11.99 WHERE id = 8;
UPDATE 1

icedb=# DELETE FROM books WHERE id = 8;
DELETE 1
```

## Error Handling

When a statement produces an error, the CLI prints the error message and returns to the prompt:

```
icedb=# SELECT * FROM nonexistent_table;
ERROR:  Table not found: nonexistent_table

icedb=#
```

The error does not terminate the session. Any open transaction remains open (but likely in an error state — issue `ROLLBACK` to clear it before running further DML).

## Exiting

```
icedb=# \q
```

Or press `Ctrl-D`. The data directory is not affected by how you exit — all committed data is already safely on disk.
