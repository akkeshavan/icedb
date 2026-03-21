# IceDB Test Architecture

## Overview

IceDB has two independent test workspaces:

| Workspace | Location | Passing | Ignored | Failed |
|-----------|----------|---------|---------|--------|
| Unit tests | `crates/` (root workspace) | **253** | 0 | 0 |
| Integration tests | `tests/` (separate workspace) | **715** | 4 | 0 |
| **Total** | | **968** | **4** | **0** |

The `tests/` directory is a **separate Cargo workspace** (`[workspace]` in its own `Cargo.toml`). Running `cargo test --workspace` from the project root only covers the 253 unit tests. Both suites must be run to get the full count.

---

## Running the Tests

```bash
# Run everything (recommended — covers both workspaces)
cd /path/to/icedb
cargo test --workspace && cd tests && cargo test

# Unit tests only
cargo test --workspace

# Integration tests only
cd tests && cargo test

# Specific integration module
cd tests && cargo test sql_conformance::select
cd tests && cargo test sql_conformance::joins
cd tests && cargo test sql_conformance::aggregates
cd tests && cargo test sql_conformance::tpch
cd tests && cargo test sql_conformance::hermitage
cd tests && cargo test sql_conformance::transactions
cd tests && cargo test acid
cd tests && cargo test concurrency

# Verbose output (see SQL statements and results)
cd tests && cargo test -- --nocapture

# Single test by name
cd tests && cargo test test_hermitage_g1a_aborted_read -- --nocapture

# Run ignored tests (to inspect unimplemented features)
cd tests && cargo test -- --include-ignored

# Control parallelism (useful for concurrency tests)
cd tests && cargo test -- --test-threads=4
```

---

## Unit Tests (`cargo test --workspace`)

These live alongside source code in each crate's `src/` directory under `#[cfg(test)]` blocks.

| Crate | Tests | What's covered |
|-------|-------|----------------|
| `auth` | 6 | Password hashing, SCRAM-SHA-256 verification, role lookup |
| `btree` | 10 | B+ tree insert/search/delete, range scan, page splits |
| `catalog` | 11 | Table/column/index registration, ACL enforcement, stats persistence |
| `txn` | 10 | Snapshot visibility, MVCC, isolation level state machine |
| `storage` | 4 | Page layout, slotted-page item pointers, buffer pool eviction |
| `sql` | 73 | Parser round-trips, planner, optimizer, executor unit tests |
| `network` (integration) | 109 | SQL conformance via in-process engine (no TCP) |
| `wal` | 13 | WAL record format, segment rotation, recovery |
| `cli` | 10 | CLI flag parsing, meta-command dispatch |
| `server` | 7 | Server startup, TLS config, connection limit wiring |

---

## Integration Tests (`cd tests && cargo test`)

Tests spin up an in-process `QueryEngine` backed by a `TempDir` — no TCP, no server process. All SQL goes directly through `QueryEngine::execute()` or `QueryEngine::execute_session()`.

### Test Helpers (`tests/src/common.rs`)

```rust
make_engine(dir.path())             // create in-process QueryEngine
exec(&engine, sql)                  // run SQL, panic on error → ExecutionResult
exec_err(&engine, sql)              // run SQL, panic if it succeeds → SqlError
count_rows(&engine, sql)            // return row count as usize
query_int(&engine, sql)             // return first col of first row as i64
exec_session(&engine, sid, sql)     // session-aware exec (supports BEGIN/SAVEPOINT)
exec_session_err(&engine, sid, sql) // session-aware, expect error
```

`exec_session` routes through `QueryEngine::execute_session()`, which maintains per-session transaction state across calls — required for `BEGIN`/`SAVEPOINT`/`ROLLBACK` to work correctly.

---

### `sql_conformance` — 711 tests across 24 modules

#### Origin of test cases

Test cases were derived from three authoritative sources:

**1. PostgreSQL regression test suite**
The following files from `src/test/regress/sql/` in the [PostgreSQL source tree](https://github.com/postgres/postgres) were fetched and ported test-by-test into Rust `#[test]` functions. Each SQL statement was translated to use icedb's in-process engine API, with expected output taken from the corresponding `.out` files.

| PostgreSQL file | IceDB module | Tests |
|----------------|--------------|-------|
| `select.sql` | `select` | 43 |
| `aggregates.sql` | `aggregates` | 42 |
| `join.sql` | `joins` | 50 |
| `subselect.sql` | `subqueries` | 41 |
| `union.sql` | `set_operations` | 48 |
| `with.sql` | `ctes` | 30 |
| `insert.sql` | `dml` | 41 |
| `update.sql` | `dml` | (included above) |
| `delete.sql` | `dml` | (included above) |
| `transactions.sql` | `transactions` | 34 |
| `boolean.sql` | `boolean_type` | 31 |
| `case.sql` | `case_expr` | 23 |
| `int4.sql` + `int8.sql` | `int_types` | 31 |
| `float8.sql` | `float_types` | 21 |
| `strings.sql` | `string_functions` | 34 |
| `date.sql` | `date_type` | 17 |
| `timestamp.sql` | `timestamp_type` | 15 |
| `limit.sql` | `limit_offset` | 26 |
| `null.sql` (semantics) | `null_handling` | 37 |
| `errors.sql` (SQLSTATE codes) | `error_handling` | 18 |

**2. Hermitage isolation test suite**
The canonical anomaly tests from [github.com/ept/hermitage](https://github.com/ept/hermitage) — G0, G1a, G1b, G1c, P4, G2-item — were ported verbatim into `hermitage.rs`, using `engine.execute_in_txn(xid, sql)` to simulate concurrent transactions.

**3. TPC-H benchmark**
A simplified TPC-H schema (6 tables, ~50 rows) was constructed to run the spirit of Q1, Q3, Q5, Q6, Q10 without date-arithmetic functions.

**4. IceDB-specific features**
Two additional modules cover features beyond the PostgreSQL regression suite:

| Module | Tests | What's covered |
|--------|-------|----------------|
| `new_features` | 28 | DATE/TIMESTAMP/UUID/NUMERIC types, SERIAL, DEFAULT, CHECK, FK, GRANT/REVOKE, VACUUM ANALYZE, UPSERT |
| `advanced_features` | 17 | LISTEN/NOTIFY, CREATE FUNCTION, cost-based optimizer, pg_dump/restore |
| `catalog_views` | 20 | `information_schema.tables/columns`, `pg_class`, `pg_authid`, COPY, PREPARE/EXECUTE |

---

#### Per-module breakdown

| Module | Tests | Source |
|--------|-------|--------|
| `joins` | 50 | `join.sql` |
| `set_operations` | 48 | `union.sql` |
| `select` | 43 | `select.sql` |
| `aggregates` | 42 | `aggregates.sql` |
| `subqueries` | 41 | `subselect.sql` |
| `dml` | 41 | `insert.sql`, `update.sql`, `delete.sql` |
| `null_handling` | 37 | NULL semantics from PostgreSQL docs |
| `transactions` | 34 | `transactions.sql` |
| `string_functions` | 34 | `strings.sql` |
| `int_types` | 31 | `int4.sql`, `int8.sql` |
| `boolean_type` | 31 | `boolean.sql` |
| `ctes` | 30 | `with.sql` |
| `new_features` | 28 | IceDB-specific |
| `limit_offset` | 26 | `limit.sql` |
| `case_expr` | 23 | `case.sql` |
| `float_types` | 21 | `float8.sql` |
| `ddl` | 20 | PostgreSQL DDL regression tests |
| `catalog_views` | 20 | `information_schema` tests |
| `error_handling` | 18 | PostgreSQL error code specification |
| `date_type` | 17 | `date.sql` |
| `advanced_features` | 17 | IceDB tier-2 features |
| `timestamp_type` | 15 | `timestamp.sql` |
| `tpch` | 14 | TPC-H benchmark |
| `hermitage` | 12 | Hermitage isolation suite |

---

### `acid` — 19 tests

| Sub-module | Tests | What's covered |
|------------|-------|----------------|
| `atomicity` | 4 | All-or-nothing commits, partial update rollback, abort on error |
| `consistency` | 5 | Schema constraints, concurrent inserts, table lifecycle |
| `isolation` | 6 | Dirty reads, write skew, lost updates, snapshot isolation levels |
| `durability` | 4 | Engine restart, aborted-not-persisted, crash simulation |

### `concurrency` — 6 tests

Multi-threaded tests using `std::thread::spawn` and `Arc<QueryEngine>` to run truly concurrent transactions, verifying that isolation anomalies do not occur.

| Module | Tests | What's covered |
|--------|-------|----------------|
| `hermitage` | 6 | G0 (dirty write), G1a (aborted read), G1b (intermediate read), P4 (lost update), concurrent snapshot read |

### `tutorial_validation` — 1 test

Runs every SQL example from the icedb book's tutorial chapter in sequence to verify documentation stays in sync with the engine.

---

## What Was Included vs. Excluded from PostgreSQL's Suite

### Included

All PostgreSQL test files where icedb implements the feature. Tests that reference the large pre-loaded tables (`onek`, `tenk1`, `aggtest`) — which require external data files loaded via `COPY FROM` — were skipped in favour of equivalent self-contained tests with inline `INSERT` data.

### Excluded (feature not implemented)

| PostgreSQL test file | Reason excluded |
|----------------------|-----------------|
| `alter_table.sql` | `ALTER TABLE` not implemented |
| `window.sql` | Window functions not implemented |
| `foreign_key.sql` | FK constraint cascade not implemented |
| `plpgsql.sql` | Procedural language not implemented |
| `sequence.sql` | `CREATE SEQUENCE` not implemented |
| `arrays.sql` | Array types not implemented |
| `json.sql` | JSON/JSONB types not implemented |
| `triggers.sql` | Triggers not implemented |
| `rules.sql` | Rule system not implemented |
| `inherit.sql` | Table inheritance not implemented |
| `rangefuncs.sql` | Table-returning functions not implemented |
| `groupingsets.sql` | `ROLLUP`/`CUBE` not implemented |

---

## The 4 Ignored Tests

| Test | Reason |
|------|--------|
| `dml::test_insert_on_conflict_do_nothing` | `ON CONFLICT DO NOTHING` (upsert) not implemented |
| `dml::test_insert_on_conflict_do_update` | `ON CONFLICT DO UPDATE` (upsert) not implemented |
| `joins::test_join_natural_join` | `NATURAL JOIN` not implemented |
| `string_functions::test_string_trim_both_explicit` | `TRIM(BOTH FROM ...)` syntax not parsed by sqlparser-rs 0.53 |

---

## Engine Fixes Made During Test Porting

Writing comprehensive tests against PostgreSQL's regression suite revealed and fixed the following engine bugs:

| Fix | Files changed |
|-----|---------------|
| `%` modulo operator missing | `plan.rs`, `planner.rs`, `executor.rs` |
| `'Infinity'::FLOAT8` / `'NaN'::FLOAT8` text cast | `value.rs` |
| `'true'::BOOLEAN` text cast | `value.rs` |
| `Text → INT4 / INT8` cast | `value.rs` |
| `IS UNKNOWN` predicate (= `IS NULL` for booleans) | `planner.rs` |
| `POSITION(needle IN haystack)` mapped to `strpos()` | `planner.rs` |
| `FETCH FIRST N ROWS ONLY` syntax ignored | `planner.rs` |
| `UNION` without `ALL` wasn't deduplicating | `executor.rs` |
| `EXCEPT ALL` removed all copies instead of one | `executor.rs` |
| `SAVEPOINT` / `ROLLBACK TO` / `RELEASE` not implemented | `engine.rs` |
| `SET TRANSACTION ISOLATION LEVEL` not accepted | `engine.rs` |
| SQL-text `BEGIN`/`ROLLBACK` not session-aware | `engine.rs`, test helpers |
