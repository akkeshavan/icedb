# IceDB Test Architecture

## Overview

IceDB has three test workspaces, each run independently with `cargo test`:

| Workspace | Location | Command | Tests | Ignored |
|-----------|----------|---------|-------|---------|
| Unit tests | `crates/` (root workspace) | `cargo test --workspace` | **313** | 0 |
| Integration tests | `tests/` | `cargo test --manifest-path tests/Cargo.toml` | **2204** | 6 |
| Chapter 3 sandbox | `sandbox/ch03/` | `cargo test --manifest-path sandbox/ch03/Cargo.toml` | **3** | 0 |
| **Grand total** | | | **2520** | **6** |

The `tests/` and `sandbox/ch03/` directories are **separate Cargo workspaces**. Running `cargo test --workspace` from the project root only covers the 313 unit tests in the main workspace.

---

## Three Test Modes

The most important architectural principle of the integration test suite is that **every test runs in three modes**:

| Mode | Suffix | Description |
|------|--------|-------------|
| **Embedded** | _(none)_ | SQL dispatched directly through an in-process `QueryEngine`. No TCP, no server process, direct Rust API calls. |
| **Plain TCP** | `_net` | A real `icedb-server` subprocess is started on a free port. A `PgClient` connects over plaintext PostgreSQL wire protocol v3.0. |
| **TLS** | `_net_tls` | Identical to plain TCP but with `sslmode=require`. The test harness generates a self-signed certificate with `openssl req` and passes `--tls-cert` / `--tls-key` to the server. |

For a function `test_foo_body(b: &Backend)`, the suite automatically registers:

```
test_foo           ← embedded, plain #[test]
test_foo_net       ← plain TCP, #[serial(network_plain)]
test_foo_net_tls   ← TLS, #[serial(network_tls)]
```

This triples coverage: the same SQL logic is validated against the in-process engine, the PostgreSQL wire protocol over TCP, and the wire protocol over TLS.

---

## Running the Tests

```bash
# All integration tests, all three modes
cargo test --manifest-path tests/Cargo.toml

# Embedded only (no server startup, fastest)
cargo test --manifest-path tests/Cargo.toml -- --skip _net

# Network variants only
cargo test --manifest-path tests/Cargo.toml -- _net

# Skip TLS (when openssl is unavailable)
cargo test --manifest-path tests/Cargo.toml -- --skip _net_tls

# Single module — all three modes
cargo test --manifest-path tests/Cargo.toml sql_conformance::joins

# Single test — all three modes
cargo test --manifest-path tests/Cargo.toml test_join_inner_basic

# Verbose output (see SQL and results)
cargo test --manifest-path tests/Cargo.toml -- --nocapture

# Run ignored tests (psql-external and SSI cycle tests)
cargo test --manifest-path tests/Cargo.toml -- --include-ignored

# Control parallelism
cargo test --manifest-path tests/Cargo.toml -- --test-threads=4

# Chapter 3 sandbox (embedded + plain TCP + TLS)
cargo test --manifest-path sandbox/ch03/Cargo.toml
cargo run  --manifest-path sandbox/ch03/Cargo.toml   # prints formatted table
```

> **Prerequisite for network tests:** Build the server binary first:
> ```sh
> cargo build -p server
> ```

---

## The `Backend` Abstraction (`tests/src/common.rs`)

Every test receives a `&Backend` argument that abstracts over the two transport modes:

```rust
pub enum Backend {
    /// Direct in-process QueryEngine call — no TCP.
    Embedded(Arc<QueryEngine>),
    /// PostgreSQL wire protocol client connected to a real server.
    Network(Mutex<PgClient>),
}
```

### Constructors

```rust
Backend::embedded(dir: &Path) -> Backend
    // Creates a QueryEngine backed by a TempDir.

net_server::plain_backend(test_name: &str) -> Backend
    // Connects to the shared plain-TCP server.
    // Creates an isolated database named after the test.

net_server::tls_backend(test_name: &str) -> Backend
    // Connects to the shared TLS server.
    // Creates an isolated database named after the test.
```

### Test helpers

```rust
exec(b, sql)                    // run SQL, panic on error → ExecutionResult
exec_err(b, sql)                // run SQL, panic if it succeeds → SqlError
count_rows(b, sql)              // return row count as usize
query_int(b, sql)               // return first col of first row as i64
exec_session(b, session_id, sql)// session-aware exec (supports BEGIN/SAVEPOINT)
exec_session_err(b, sid, sql)   // session-aware, expect error
b.try_execute(sql)              // returns Result<ExecutionResult, SqlError>
b.is_network()                  // true for Network variants
b.as_engine()                   // panics for Network; use only in embedded branches
```

`exec_session` maintains per-session transaction state across calls, which is required for `BEGIN` / `SAVEPOINT` / `ROLLBACK` sequences to work correctly. In embedded mode this routes through `QueryEngine::execute_session()`; in network mode each `exec_session` call sends the SQL over the wire and the server maintains session state per connection.

---

## The `net_tests!` Macro (`tests/src/lib.rs`)

Writing a new test that runs in all three modes requires only:

1. Write a `_body` function that accepts `&Backend`.
2. Write the usual `#[test]` function that creates an `Embedded` backend and calls the body.
3. Invoke the `net_tests!` macro with the test name.

```rust
fn test_foo_body(b: &crate::common::Backend) {
    exec(b, "CREATE TABLE t (id INT)");
    exec(b, "INSERT INTO t VALUES (1)");
    assert_eq!(count_rows(b, "SELECT * FROM t"), 1);
}

#[test]
fn test_foo() {
    let dir = tempfile::TempDir::new().unwrap();
    let b = crate::common::Backend::embedded(dir.path());
    test_foo_body(&b);
}

crate::net_tests!(test_foo);
```

`net_tests!(test_foo)` expands (via the `paste` crate) to:

```rust
#[test]
#[serial(network_plain)]
fn test_foo_net() {
    let b = crate::net_server::plain_backend("test_foo_net");
    test_foo_body(&b);
}

#[test]
#[serial(network_tls)]
fn test_foo_net_tls() {
    let b = crate::net_server::tls_backend("test_foo_net_tls");
    test_foo_body(&b);
}
```

The `#[serial]` attributes (from the `serial_test` crate) ensure that all `_net` tests share a single serialization slot and all `_net_tls` tests share another. This prevents port exhaustion and race conditions on server startup while still allowing full parallelism between the two server instances and all embedded tests.

---

## Network Infrastructure (`tests/src/net_server.rs`)

### `NetServer`

Wraps a spawned `icedb-server` child process:

```rust
pub struct NetServer {
    pub port: u16,
    pub use_tls: bool,
    _cert_dir: Option<TempDir>,   // keeps TLS cert files alive
    _data_dir: TempDir,           // keeps server data directory alive
    _process: Mutex<Child>,
}
```

`NetServer::start(use_tls)`:
1. Finds a free port via `TcpListener::bind("127.0.0.1:0")`.
2. Creates a `TempDir` for the server's data directory.
3. For TLS: calls `openssl req -x509 ...` to generate a self-signed certificate into a second `TempDir`.
4. Spawns `icedb-server` with `--port`, `--data-dir`, and optionally `--tls-cert` / `--tls-key`.
5. Redirects the server's `stdout` and `stderr` to `Stdio::null()` — critical to prevent the server process from inheriting the test binary's stdout/stderr pipe (which would cause `cargo test | tail -N` style invocations to hang indefinitely).
6. Polls `PgClient::connect()` every 100 ms until the server accepts connections, up to 15 seconds.

### Global singleton servers

Two `OnceLock<NetServer>` instances ensure the server starts once per test binary invocation:

```rust
static PLAIN_SERVER: OnceLock<NetServer> = OnceLock::new();
static TLS_SERVER:   OnceLock<NetServer> = OnceLock::new();
```

All `_net` tests share the plain server; all `_net_tls` tests share the TLS server. Database isolation is achieved at the SQL level (see below).

### Per-test database isolation

Each network test runs in its own database named after the test function:

```rust
fn provision_database(server: &NetServer, db_name: &str) {
    let mut admin = server.new_client();   // connects to "icedb" default db
    let _ = admin.query(&format!("DROP DATABASE IF EXISTS {db_name}"));
    admin.query(&format!("CREATE DATABASE {db_name}")).unwrap();
}
```

Test names are sanitized to valid SQL identifiers (lowercase alphanumeric + underscores, prefixed with `t_`).

### Server binary discovery

`find_server_binary()` looks for the server binary at:
1. `../target/debug/icedb-server` (relative to `CARGO_MANIFEST_DIR`)
2. `../target/release/icedb-server`
3. Falls back to `icedb-server` on `PATH`

---

## `PgClient` — Wire Protocol Client (`crates/cli/src/pg_client.rs`)

`PgClient` is a minimal synchronous PostgreSQL wire protocol v3.0 client used exclusively for testing. It handles:

- TCP connection and optional TLS handshake (via `native-tls`)
- PostgreSQL startup message and `AuthenticationOK` handshake
- Simple Query protocol: sends `Q` message, reads `RowDescription` + `DataRow` + `CommandComplete`
- Error parsing: extracts `SQLSTATE` code from `ErrorResponse` fields and embeds it as `"SQLSTATE[CODE]: message"` for structured error mapping in test assertions

### Type OID mapping

`PgResult` carries `col_type_oids: Vec<u32>` extracted from the `RowDescription` message. The test helper `pg_result_to_execution_result()` in `common.rs` uses these OIDs to reconstruct typed `Value` variants:

| PG OID | `DataType` | `Value` variant |
|--------|-----------|-----------------|
| 23 | `Int4` | `Value::Int4` |
| 20 | `Int8` | `Value::Int8` |
| 701 | `Float8` | `Value::Float8` |
| 16 | `Boolean` | `Value::Bool` |
| 25 / 1043 | `Text` | `Value::Text` |
| 1082 | `Date` | `Value::Date(epoch_days)` |
| 1114 | `Timestamp` | `Value::Timestamp(epoch_micros)` |
| 1700 | `Numeric` | `Value::Numeric` |
| 2950 | `Uuid` | `Value::Uuid` |

For `OID 25` (Text), the helper applies `classify_text_value()` which inspects the string to detect PostgreSQL array syntax (`{a,b,c}`) and JSON objects, returning `Value::Array` or `Value::Json` accordingly.

### Error mapping

Network errors arrive as strings of the form `"SQLSTATE[42P01]: ERROR: Table 'foo' not found"`. `pg_error_to_sql_error()` in `common.rs` parses the SQLSTATE prefix and maps it to the matching `SqlError` variant:

| SQLSTATE | `SqlError` variant |
|----------|-------------------|
| `42P01` | `TableNotFound` |
| `42703` | `ColumnNotFound` |
| `42601` | `Parse` |
| `42P07` | `Catalog(DuplicateTable)` |
| `23505` | `UniqueViolation` |
| `23000` | `ConstraintViolation` |
| `22012` | `DivisionByZero` |
| `22003` | `NumericOverflow` |
| `0A000` | `NotImplemented` |
| `42804` | `TypeError` |

---

## `build_field_infos` — Server-Side Type Inference (`crates/network/src/handler.rs`)

When streaming result rows to a client, `build_field_infos` constructs the `RowDescription` message. It must assign a correct PG type OID to each column so that `PgClient` can reconstruct typed values on the other side.

The logic:
1. **Prefer the declared schema `dtype`**: if the executor's `Row.schema` declares a non-Text type, use `datatype_to_pg_type(dtype)`. This handles nullable columns whose first row is `NULL` correctly.
2. **Scan all rows for the first non-null value** when `dtype` is `Text` (the "unknown" sentinel): walk every row in the result to find the first non-null value and infer the type from it. This fixes expressions like `NULLIF(val, 0)` where the first output row may be `NULL` even though subsequent rows are typed integers.
3. **Fall back to `Type::TEXT`** if all rows are null or the result is empty.

---

## Tests That Run Embedded-Only

A small number of tests use internal Rust APIs that are not accessible over the PostgreSQL wire protocol. These tests call `if b.is_network() { return; }` at the start of the body function, making them a no-op for `_net` and `_net_tls` variants while still exercising the full logic in embedded mode:

| Test group | Reason |
|------------|--------|
| `test_dump_to_file`, `test_dump_and_restore_roundtrip`, `test_dump_empty_database` | Uses `engine.dump_to_file()` / `restore_from_file()` — internal Rust API |
| `test_listen_notify_roundtrip` | Uses `engine.catalog.listen()` — internal pub/sub receiver |
| `test_tables_needing_vacuum_initially_all`, `test_tables_needing_vacuum_after_vacuum` | Uses `engine.catalog.tables_needing_vacuum()` — internal API |
| `test_tutorial_chapter3_values` | Uses `val(&engine, ...)` helper that takes `&QueryEngine` directly |

Tests that can express assertions entirely in SQL use dual-branch logic:

```rust
if b.is_network() {
    // Verify functionally via SQL
    exec(b, "INSERT INTO def_test (id) VALUES (1)");
    assert_eq!(query_int(b, "SELECT val FROM def_test WHERE id = 1"), 42);
} else {
    // Verify via catalog introspection
    let schema = b.as_engine().catalog.get_table("public", "def_test").unwrap();
    assert!(schema.columns[1].has_default);
}
```

---

## Unit Tests (`cargo test --workspace`) — 313 total

These live alongside source code in each crate's `src/` directory under `#[cfg(test)]` blocks.

| Crate | Tests | What's covered |
|-------|-------|----------------|
| `sql` | 133 | Parser round-trips, planner, optimizer, executor unit tests |
| `network` | 109 | SQL conformance via in-process engine (no TCP) |
| `wal` | 13 | WAL record format, segment rotation, recovery |
| `catalog` | 11 | Table/column/index registration, ACL enforcement, stats persistence |
| `btree` | 10 | B+ tree insert/search/delete, range scan, page splits |
| `txn` | 10 | Snapshot visibility, MVCC, isolation level state machine |
| `cli` | 7 | CLI flag parsing, meta-command dispatch |
| `server` | 7 | Server startup, TLS config, connection limit wiring |
| `auth` | 6 | Password hashing, SCRAM-SHA-256 verification, role lookup |
| `storage` | 4 | Page layout, slotted-page item pointers, buffer pool eviction |

---

## Integration Tests — 2204 total across all modes

### `sql_conformance` — 25 modules

Each module runs its tests in all three modes via the `net_tests!` macro.
"Embedded" is the number of `#[test]` functions; "Network" is the number of additional variants (2× the `net_tests!` macro call count).

| Module | Embedded | +Network | Total | Source |
|--------|----------|----------|-------|--------|
| `joins` | 50 | 100 | **150** | `join.sql` |
| `aggregates` | 49 | 98 | **147** | `aggregates.sql` |
| `set_operations` | 48 | 96 | **144** | `union.sql` |
| `select` | 43 | 86 | **129** | `select.sql` |
| `subqueries` | 41 | 82 | **123** | `subselect.sql` |
| `dml` | 41 | 80 | **121** | `insert.sql`, `update.sql`, `delete.sql` |
| `null_handling` | 37 | 74 | **111** | NULL semantics from PostgreSQL docs |
| `advanced_features` | 37 | 74 | **111** | IceDB tier-2 features |
| `string_functions` | 34 | 68 | **102** | `strings.sql` |
| `transactions` | 34 | 0 | **34** | `transactions.sql` (session-state tests; embedded-only) |
| `int_types` | 31 | 62 | **93** | `int4.sql`, `int8.sql` |
| `boolean_type` | 31 | 62 | **93** | `boolean.sql` |
| `ctes` | 30 | 60 | **90** | `with.sql` |
| `new_features` | 28 | 56 | **84** | IceDB-specific features |
| `limit_offset` | 26 | 52 | **78** | `limit.sql` |
| `case_expr` | 23 | 46 | **69** | `case.sql` |
| `float_types` | 21 | 42 | **63** | `float8.sql` |
| `ddl` | 20 | 40 | **60** | PostgreSQL DDL regression tests |
| `catalog_views` | 20 | 40 | **60** | `information_schema` tests |
| `error_handling` | 18 | 36 | **54** | PostgreSQL error code specification |
| `date_type` | 17 | 34 | **51** | `date.sql` |
| `timestamp_type` | 15 | 30 | **45** | `timestamp.sql` |
| `array_json` | 14 | 28 | **42** | Array and JSON type tests |
| `tpch` | 14 | 28 | **42** | TPC-H benchmark subset |
| `hermitage` | 12 | 0 | **12** | Hermitage isolation suite (concurrent; embedded-only) |

### Origin of test cases

**1. PostgreSQL regression test suite** — Test cases ported from `src/test/regress/sql/` in the PostgreSQL source tree, translated to use the `Backend` abstraction.

**2. Hermitage isolation suite** — Canonical anomaly tests from [github.com/ept/hermitage](https://github.com/ept/hermitage) (G0, G1a, G1b, G1c, P4, G2-item) using concurrent threads on a shared `Arc<QueryEngine>`.

**3. TPC-H benchmark** — Simplified TPC-H schema (6 tables, ~50 rows) covering queries Q1, Q3, Q5, Q6, Q10.

**4. IceDB-specific features** — `new_features`, `advanced_features`, `array_json`, `catalog_views`.

---

### `acid` — 80 total

| Sub-module | Embedded | +Network | Total | What's covered |
|------------|----------|----------|-------|----------------|
| `consistency` | 14 | 28 | **42** | Schema constraints, concurrent inserts, table lifecycle |
| `isolation` | 13 | 0 | **13** | Dirty reads, write skew, lost updates, snapshot isolation (concurrent) |
| `atomicity` | 10 | 0 | **10** | All-or-nothing commits, partial update rollback, abort on error |
| `durability` | 10 | 0 | **10** | Engine restart, aborted-not-persisted, crash simulation |
| `fault_injection` | 5 | 0 | **5** | SIGKILL mid-transaction, recovery verification |

Isolation, durability, and fault injection tests use `Arc<QueryEngine>` concurrency patterns incompatible with network mode and run embedded-only.

### `concurrency` — 12 total

Multi-threaded tests using `std::thread::spawn` and `Arc<QueryEngine>`:

| Sub-module | Tests | What's covered |
|------------|-------|----------------|
| `hermitage` | 8 | G0, G1a, G1b, P4, concurrent snapshot read — all embedded |
| `tpcb` | 4 | TPC-B bank transfer at varying concurrency levels — all embedded |

### `tutorial_validation` — 6 total

Runs every SQL example from the icedb-book tutorial chapters in sequence to verify that documentation stays in sync with the engine.

| Mode | Tests |
|------|-------|
| Embedded (`test_tutorial_chapter3`, `test_tutorial_chapter3_values`) | 2 |
| Plain TCP | 2 |
| TLS | 2 |

`test_tutorial_chapter3_values` uses the `val(&engine, ...)` helper that takes `&QueryEngine` directly; its `_net` and `_net_tls` variants return immediately via `if b.is_network() { return; }`.

---

## Chapter 3 Sandbox (`sandbox/ch03`) — 3 tests

The chapter 3 sandbox is a standalone binary that also registers three `#[test]` functions:

| Test | Mode | Description |
|------|------|-------------|
| `tests::ch03_embedded` | Embedded | Runs ~116 assertions against an in-process engine |
| `tests::ch03_plain_tcp` | Plain TCP | Starts a fresh `icedb-server`; sends every example over the wire |
| `tests::ch03_tls` | TLS | Same but with `sslmode=require` |

`Stats::assert_all_passed()` collects all failure messages and panics with a consolidated report if any assertion fails, giving clean `cargo test` failure output.

The binary entrypoint (`cargo run`) prints a human-readable pass/fail/skip table with per-mode summaries and exits with code 1 if any assertion fails.

---

## The 6 Ignored Tests

| Test | Reason |
|------|--------|
| `integration::psql_smoke::test_psql_connect_select_one` | Requires `psql` binary on PATH |
| `integration::psql_smoke::test_psql_crud_smoke` | Requires `psql` binary on PATH |
| `integration::psql_smoke::test_psql_transaction_smoke` | Requires `psql` binary on PATH |
| `integration::dbeaver_compat::test_dbeaver_connection_checklist` | Requires running server; connection string checklist |
| `concurrency::hermitage::test_g_single_serializable` | SSI cycle detection not yet fully implemented |
| `concurrency::hermitage::test_g2_serializable_cycle_detection` | SSI rw-antidependency tracking in development |

---

## What Was Excluded from the PostgreSQL Suite

| PostgreSQL test file | Reason excluded |
|----------------------|-----------------|
| `plpgsql.sql` | PL/pgSQL procedural language not implemented |
| `triggers.sql` | Triggers not implemented |
| `rules.sql` | Rule system not implemented |
| `inherit.sql` | Table inheritance not implemented |
| `rangefuncs.sql` | Table-returning functions not implemented |
| `groupingsets.sql` | `ROLLUP`/`CUBE` not implemented |

---

## Engine Fixes Made During Test Work

Writing comprehensive tests across three modes revealed and fixed several bugs:

| Fix | Location |
|-----|----------|
| `build_field_infos` used first-row value for OID inference — NULLIF(val,0) returned Text when first row was NULL | `crates/network/src/handler.rs` |
| Server processes inherited test binary stdout/stderr pipe, blocking `\| tail -N` | `tests/src/net_server.rs` |
| `Row::new(values, vec![])` passed empty schema — `get("col_name")` always returned None | `tests/src/common.rs` |
| `parse_error_response` discarded SQLSTATE code — all network errors mapped to `Execution` variant | `crates/cli/src/pg_client.rs` |
| Integer literals returned as `Int8` for all magnitudes — OID 23 (INT4) not mapped | `tests/src/common.rs` |
| Date/Timestamp columns returned as `Text` — OID 1082/1114 not mapped to typed parsers | `tests/src/common.rs` |
| Array values returned as `Text("{a,b,c}")` — server sends arrays with `Type::TEXT` OID | `tests/src/common.rs` |
| B-tree read operations held exclusive write lock — `search()` / `range_scan()` changed to shared lock | `crates/btree/src/file.rs` |
| Transaction manager cloned entire committed-XID set on every scan | `crates/txn/src/manager.rs` |
| SSI read-set acquired one lock per visible tuple — replaced with single batch flush | `crates/txn/src/manager.rs` |
