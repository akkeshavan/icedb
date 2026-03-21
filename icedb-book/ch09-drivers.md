# Chapter 9: Client Drivers

icedb provides three embedded drivers: one for Rust, one for Python, and one for Node.js/TypeScript. All three operate in **embedded mode** — the query engine runs in the same process as your application, reading and writing the data directory directly. There is no server process, no TCP connection, and no network overhead.

This is analogous to SQLite's embedded model, but with full SQL semantics, MVCC isolation, and WAL durability.

**In this chapter:**
- Rust driver: opening a database, running queries, connection pool, explicit transactions
- Python driver: `connect()`, `execute()`, `execute_dml()`, type mapping, pandas integration
- Node.js/TypeScript driver: `connect()`, `query()`, `execute()`, type handling
- Choosing the right driver

---

## Rust Driver

The Rust driver (`drivers/rust`) is the native layer that all other drivers are built on. It exposes the `QueryEngine` directly via a convenient `Connection` and `ConnectionPool` API.

### Adding the Dependency

In your `Cargo.toml`:

```toml
[dependencies]
icedb_driver = { path = "path/to/icedb/drivers/rust" }
```

Or, once published to crates.io:

```toml
[dependencies]
icedb_driver = "0.1"
```

### Opening a Database

```rust
use icedb_driver;
use std::path::Path;

fn main() -> Result<(), icedb_driver::DriverError> {
    // Open or create the database at the given path.
    // This initializes the WAL, transaction manager, and catalog.
    let engine = icedb_driver::open(Path::new("./mydata"))?;
    Ok(())
}
```

`icedb_driver::open` returns an `Arc<QueryEngine>`. You can clone this `Arc` and share it across threads. The engine is internally synchronized with a `Mutex` on the WAL writer and `RwLock` on the catalog.

### Running Queries

Use `Connection::new` to get a connection handle that wraps the engine:

```rust
use icedb_driver::{Connection, DriverError};
use std::path::Path;
use std::sync::Arc;

fn main() -> Result<(), DriverError> {
    let engine = icedb_driver::open(Path::new("./mydata"))?;
    let conn = Connection::new(Arc::clone(&engine));

    // DDL
    conn.execute("CREATE TABLE books (id INT NOT NULL, title TEXT NOT NULL, price FLOAT)")?;

    // DML
    conn.execute("INSERT INTO books VALUES (1, 'The Hobbit', 12.99)")?;
    conn.execute("INSERT INTO books VALUES (2, 'Dune', 15.99)")?;

    // Query — returns an ExecutionResult
    let result = conn.execute("SELECT * FROM books WHERE price < 15.00")?;
    for row in &result.rows {
        println!("{:?}", row.values);
    }
    // Output: [Int4(1), Text("The Hobbit"), Float8(12.99)]

    Ok(())
}
```

`Connection::execute` uses `QueryEngine::execute` under the hood, which auto-commits each statement in a `ReadCommitted` transaction.

### Connection Pool

For multi-threaded applications, use the connection pool:

```rust
use icedb_driver;
use std::path::Path;

fn main() -> Result<(), icedb_driver::DriverError> {
    // Open a pool with a maximum of 10 concurrent connections.
    // All connections share the same underlying engine (and thus the same data).
    let pool = icedb_driver::open_pool(Path::new("./mydata"), 10)?;

    // Acquire a connection from the pool.
    let conn = pool.acquire()?;
    conn.execute("INSERT INTO events (user_id, type) VALUES (42, 'login')")?;
    // conn is returned to the pool when it drops.

    Ok(())
}
```

The `ConnectionPool` holds an `Arc<QueryEngine>` internally. `acquire()` returns a `PooledConnection` that wraps a `Connection`. When the `PooledConnection` drops, the slot is freed for other callers.

### Explicit Transactions

For multi-statement transactions, use the transaction API. Set the isolation level before calling `begin()`:

```rust
use icedb_driver::Connection;
use txn::transaction::IsolationLevel;
use std::sync::Arc;

fn transfer(
    conn: &mut Connection,
    from_id: i32,
    to_id: i32,
    amount: f64,
) -> Result<(), icedb_driver::DriverError> {
    // Set isolation level, then begin
    conn.set_isolation_level(IsolationLevel::RepeatableRead);
    let xid = conn.begin()?;

    conn.execute_in_txn(
        &format!("UPDATE accounts SET balance = balance - {amount} WHERE id = {from_id}"),
        xid,
    )?;
    conn.execute_in_txn(
        &format!("UPDATE accounts SET balance = balance + {amount} WHERE id = {to_id}"),
        xid,
    )?;

    conn.commit()?;
    Ok(())
}
```

`begin()` takes no arguments; the isolation level is configured with `set_isolation_level()` before calling `begin()`. If `execute_in_txn` returns an error, call `conn.rollback()` to release the transaction. If the `Connection` drops while a transaction is open, the transaction is automatically aborted.

### Type Mapping

| icedb SQL type | Rust Value variant | Rust native type |
|----------------|--------------------|------------------|
| BOOLEAN | `Value::Bool(bool)` | `bool` |
| INT / INT4 | `Value::Int4(i32)` | `i32` |
| BIGINT / INT8 | `Value::Int8(i64)` | `i64` |
| FLOAT / FLOAT8 | `Value::Float8(f64)` | `f64` |
| TEXT / VARCHAR | `Value::Text(String)` | `String` |
| BYTEA | `Value::Bytes(Vec<u8>)` | `Vec<u8>` |
| NULL | `Value::Null` | — |

Row values are returned as `Vec<Value>`. The `Value` enum is exported from the `sql` crate and re-exported by the driver.

### Error Handling

All fallible operations return `Result<_, DriverError>`. `DriverError` variants:

- `DriverError::Connection(String)` — failed to open the data directory or WAL
- `DriverError::Sql(SqlError)` — the SQL engine returned an error (parse error, type error, constraint violation, etc.)
- `DriverError::Query(String)` — a query-level error with a plain message
- `DriverError::Txn(TxnError)` — a transaction manager error (commit/abort failure)
- `DriverError::PoolExhausted` — all pool slots are in use; retry after releasing a connection
- `DriverError::TypeConversion(String)` — a Rust-side value conversion error

---

## Python Driver

The Python driver (`drivers/python`) is built with PyO3 and Maturin. It exposes a `PyConnection` class (returned by the `connect()` function) and a `PyRow` class for result rows.

### Installation

Once published to PyPI:

```sh
pip install icedb
```

For development, build and install from source using Maturin:

```sh
cd drivers/python
pip install maturin
maturin develop   # installs into the current virtualenv
```

> **Note**: The Python driver uses PyO3 0.22, which officially supports CPython 3.8–3.13. On
> Python 3.14 or later, pass `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1` to opt in to the stable ABI
> and build anyway:
>
> ```sh
> PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 maturin develop
> ```
>
> For the safest experience, use Python 3.8–3.13 (e.g. via `pyenv`). Full Python 3.14 support will
> be available once PyO3 0.23 is released.

### Connecting

```python
import icedb

# Open or create the database at the given path.
conn = icedb.connect("./mydata")
```

`connect()` takes a filesystem path as a string. It initializes the database (WAL, catalog) if the directory is empty, or opens the existing database otherwise.

### Executing Queries

```python
# SELECT — returns a list of PyRow objects
rows = conn.execute("SELECT * FROM books WHERE price < 15.00")
for row in rows:
    # Access by column name
    print(row["title"], row["price"])

    # Or convert to dict
    d = row.as_dict()
    print(d)  # {'id': 1, 'title': 'The Hobbit', 'price': 12.99}
```

`PyRow` supports:
- `row["column_name"]` — access by column name (raises `KeyError` if not found)
- `row.as_dict()` — returns a `dict` with column names as keys
- `row.keys()` — returns a list of column names
- `repr(row)` — returns `Row({column: value, ...})`

### DML Statements

```python
# Returns the number of affected rows
n = conn.execute_dml("INSERT INTO books VALUES (42, 'Dune', 15.99)")
print(f"Inserted {n} row(s)")

n = conn.execute_dml("UPDATE books SET price = 14.99 WHERE id = 42")
print(f"Updated {n} row(s)")

n = conn.execute_dml("DELETE FROM books WHERE id = 42")
print(f"Deleted {n} row(s)")
```

Use `execute()` for SELECT (returns rows); use `execute_dml()` for INSERT/UPDATE/DELETE (returns row count).

### Type Mapping

| icedb SQL type | Python type |
|----------------|-------------|
| BOOLEAN | `bool` |
| INT / INT4 | `int` |
| BIGINT / INT8 | `int` |
| FLOAT / FLOAT8 | `float` |
| TEXT / VARCHAR | `str` |
| BYTEA | `bytes` |
| NULL | `None` |

### Working with pandas

Since rows can be converted to dicts, it is straightforward to build a DataFrame:

```python
import icedb
import pandas as pd

conn = icedb.connect("./mydata")
rows = conn.execute("SELECT * FROM sales WHERE year = 2024")
df = pd.DataFrame([r.as_dict() for r in rows])

print(df.head())
print(df.describe())
```

### DDL from Python

```python
conn = icedb.connect("./mydata")

# DDL uses execute() — returns an empty list (no rows)
conn.execute("CREATE TABLE metrics (ts INT, value FLOAT, tag TEXT)")

# Bulk insert
for i in range(1000):
    conn.execute_dml(f"INSERT INTO metrics VALUES ({i}, {i * 1.5}, 'sensor-1')")
```

### Limitations

- No explicit transaction API in the current Python driver. Each `execute()` or `execute_dml()` call auto-commits in a `ReadCommitted` transaction. Transaction API is planned — not yet available.
- No async/await support. The driver is synchronous. Async support (integrating with Python's `asyncio` via Tokio) is planned.
- No connection pooling at the Python level. The `PyConnection` wraps a single `QueryEngine` instance. For concurrent access from Python threads, all calls are serialized by the engine's internal locks.

---

## Node.js / TypeScript Driver

The Node.js driver (`drivers/nodejs`) is built with NAPI-RS. It exports a `Connection` class and a `connect()` function. Values are returned as strings for simplicity; typed parsing is planned.

### Installation

Once published to npm:

```sh
npm install @icedb/driver
```

For development, build from source:

```sh
cd drivers/nodejs
npm install
npm run build   # runs napi build
```

> **Note**: NAPI-RS requires a `build.rs` file in the crate root that calls `napi_build::setup()`.
> Without it the linker cannot resolve NAPI symbols. The file ships with the repository; if you
> are starting a new crate from scratch, add:
>
> ```rust
> // build.rs
> extern crate napi_build;
> fn main() { napi_build::setup(); }
> ```
>
> The `napi-build` crate must also be listed under `[build-dependencies]` in `Cargo.toml`.
>
> The minimum required Rust toolchain version is dictated by `napi-build`. If your `rustc` is
> older than what the latest `napi-build` requires, pin the version in `Cargo.lock`:
>
> ```sh
> cargo update napi-build --precise <compatible-version>
> ```

### Connecting

```typescript
import { connect } from '@icedb/driver';

const conn = connect('./mydata');
```

`connect()` takes a filesystem path. The connection is synchronous — the database opens during the `connect()` call.

### Running Queries

```typescript
import { connect } from '@icedb/driver';

const conn = connect('./mydata');

// DDL
conn.execute("CREATE TABLE products (id INT NOT NULL, name TEXT NOT NULL, price FLOAT)");

// Insert
conn.execute("INSERT INTO products VALUES (1, 'Widget', 9.99)");
conn.execute("INSERT INTO products VALUES (2, 'Gadget', 19.99)");

// SELECT — returns an array of JsRow objects
const rows = conn.query("SELECT * FROM products WHERE price < 15.00");
for (const row of rows) {
    console.log(row.columns);   // ['id', 'name', 'price']
    console.log(row.values);    // ['1', 'Widget', '9.99']  ← all strings
}
```

Each `JsRow` has two properties:
- `columns: string[]` — ordered array of column names
- `values: (string | null)[]` — column values serialized as strings; NULL becomes `null`

### DML Statements

```typescript
// execute() returns the number of affected rows as a number
const affected = conn.execute(
    "UPDATE products SET price = 8.99 WHERE id = 1"
);
console.log(`Updated ${affected} row(s)`);

const deleted = conn.execute("DELETE FROM products WHERE price > 100");
console.log(`Deleted ${deleted} row(s)`);
```

`conn.execute()` is used for both DML (returns row count) and DDL (returns 0). `conn.query()` is used for SELECT (returns rows).

### Type Handling

All values are returned as strings in the current version. Parse them yourself based on the expected schema:

```typescript
const rows = conn.query("SELECT id, price FROM products");
for (const row of rows) {
    const id = parseInt(row.values[0]!, 10);
    const price = parseFloat(row.values[1]!);
    console.log(id, price);
}
```

NULL values appear as `null` (not the string `"null"`):

```typescript
const rows = conn.query("SELECT * FROM products WHERE price IS NULL");
for (const row of rows) {
    // row.values[2] is null if price is NULL
    const price = row.values[2];  // null
}
```

### TypeScript Types

The NAPI-RS build generates TypeScript declarations automatically. The exported types are:

```typescript
interface JsRow {
    columns: string[];
    values: (string | null)[];
}

export function connect(dataDir: string): Connection;

export class Connection {
    query(sql: string): JsRow[];
    execute(sql: string): number;
}
```

### Limitations

- No async/await API. All calls are synchronous. Wrapping in `worker_threads` is possible for non-blocking access from async Node.js code.
- Values are returned as strings. Numeric types must be parsed manually until type-aware output is implemented.
- No connection pooling. The `Connection` object holds a single `QueryEngine` reference.
- No explicit transaction API. Each call auto-commits.

---

## Choosing the Right Driver

| Criterion | Rust Driver | Python Driver | Node.js Driver |
|-----------|-------------|---------------|----------------|
| Type safety | Full (Rust types) | Dynamic (Python types) | Strings only |
| Async support | Tokio (planned) | Planned | Planned |
| Transaction API | Yes | No | No |
| Connection pool | Yes | No | No |
| Build tooling | Cargo | Maturin/pip | NAPI-RS/npm |
| Performance | Native | Near-native | Near-native |
| Best for | Rust applications | Data analysis, ML pipelines | Web backends, CLI tools |

All three drivers use the same underlying `QueryEngine` and thus have identical SQL semantics, ACID guarantees, and MVCC behavior. The differences are purely in language ergonomics and current API completeness.
