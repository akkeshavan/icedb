# IceDB Implementation Status

**Last updated**: 2026-03-19
**Total tests**: 968 passing, 4 ignored, 0 failing
**Build**: `cargo build --workspace` — clean, zero warnings

---

## Quick-start for a new session

```bash
# Verify everything still compiles and passes
cd /path/to/icedb
cargo build --workspace
cargo test --workspace          # 253 unit tests
cd tests && cargo test          # 715 integration tests
```

---

## Phase-by-phase status

### Phase 1 — Storage Foundation ✅ COMPLETE
All storage primitives implemented and tested.

| Item | Status |
|------|--------|
| 8 kB page layout (`PageHeader`, `pd_lsn`, `pd_lower/upper`, etc.) | ✅ |
| Page checksum (CRC32C) | ✅ |
| Slotted-page item identifier array | ✅ |
| Tuple header with MVCC fields (`t_xmin`, `t_xmax`, `t_cid`, `t_ctid`, null bitmap) | ✅ |
| Heap file CRUD by TID | ✅ |
| `BufferPool` with fixed-size frame array | ✅ |
| Clock-sweep eviction algorithm | ✅ |
| Pin/unpin, dirty marking, background flush thread | ✅ |
| Unit tests (4) | ✅ |

### Phase 2 — Write-Ahead Log & Crash Recovery ✅ COMPLETE

| Item | Status |
|------|--------|
| LSN type (`u64`, monotonic) | ✅ |
| WAL record format (LSN, type, page ref, before/after image) | ✅ |
| Sequential segment files, append-only writer | ✅ |
| `fsync` on transaction commit | ✅ |
| Checkpoint (flush dirty pages → write checkpoint record) | ✅ |
| Redo recovery from last checkpoint LSN on startup | ✅ |
| `pd_lsn` stamped on every page write | ✅ |
| Unit tests (13) | ✅ |

### Phase 3 — Transaction Manager & MVCC ✅ COMPLETE

| Item | Status |
|------|--------|
| Transaction ID allocator (global atomic `u64`) | ✅ |
| `Snapshot` struct: `xmin`, `xmax`, `in_progress` set | ✅ |
| `is_visible(tuple_header, snapshot)` | ✅ |
| `BEGIN` / `COMMIT` / `ROLLBACK` state machine | ✅ |
| Read Committed (per-statement snapshot) | ✅ |
| Repeatable Read (per-transaction snapshot) | ✅ |
| Two-phase locking for write-write conflicts | ✅ |
| Dead tuple tracking / VACUUM hook | ✅ |
| Serializable Snapshot Isolation (SSI) | ✅ |
| Unit tests (10) — snapshot visibility, MVCC, isolation levels | ✅ |

### Phase 4 — Persistent B+ Tree Index ✅ COMPLETE

| Item | Status |
|------|--------|
| Internal node page format (keys + child pointers) | ✅ |
| Leaf node page format (keys + TIDs) with sibling pointers | ✅ |
| Metapage (page 0): root pointer, tree height | ✅ |
| Search root-to-leaf with shared latches | ✅ |
| Insert with leaf split, key promotion, latch crabbing | ✅ |
| Delete with merge/redistribute | ✅ |
| Range scan via sibling chain | ✅ |
| WAL-logged structural modification operations (SMOs) | ✅ |
| SMO recovery replay | ✅ |
| Unit tests (10) | ✅ |

### Phase 5 — System Catalog ✅ COMPLETE

| Item | Status |
|------|--------|
| `pg_class` — table/index registry | ✅ |
| `pg_attribute` — column definitions and types | ✅ |
| `pg_authid` — roles with privilege flags | ✅ |
| `pg_statistic` — column histograms and MCVs | ✅ |
| `CREATE TABLE`, `DROP TABLE`, `CREATE INDEX` | ✅ |
| NOT NULL, UNIQUE, PRIMARY KEY, FOREIGN KEY (tracking) | ✅ |
| `information_schema.tables/columns` | ✅ |
| `pg_roles` view | ✅ |
| Unit tests (11) | ✅ |

### Phase 6 — SQL Engine ✅ COMPLETE (with known gaps — see below)

| Item | Status |
|------|--------|
| `sqlparser-rs` (PostgreSQL dialect) | ✅ |
| Semantic analyzer (resolves names against catalog) | ✅ |
| Logical plan: Scan, Filter, Project, Join, Aggregate, Sort, Limit | ✅ |
| Cost-based optimizer: equality-predicate IndexScan rewrite | ✅ |
| Physical operators: SeqScan, IndexScan, HashJoin, MergeJoin, NestedLoop, Sort, HashAgg | ✅ |
| Volcano iterator model | ✅ |
| DML: INSERT, UPDATE, DELETE with MVCC | ✅ |
| EXPLAIN / EXPLAIN ANALYZE | ✅ |
| Subqueries (correlated and uncorrelated) | ✅ |
| CTEs (WITH ... AS) | ✅ |
| LATERAL joins | ✅ |
| UNION / UNION ALL / INTERSECT / INTERSECT ALL / EXCEPT / EXCEPT ALL | ✅ |
| GROUP BY / HAVING / ORDER BY / LIMIT / OFFSET | ✅ |
| Window functions (basic — OVER with ORDER BY) | ✅ |
| Aggregate functions: COUNT, SUM, AVG, MIN, MAX | ✅ |
| String functions: UPPER, LOWER, LENGTH, TRIM, LTRIM, RTRIM, SUBSTRING, POSITION, REPLACE | ✅ |
| LIKE / ILIKE / NOT LIKE | ✅ |
| Modulo operator `%` | ✅ |
| IS NULL / IS NOT NULL / IS UNKNOWN / IS NOT UNKNOWN | ✅ |
| CASE / WHEN / THEN / ELSE | ✅ |
| Type casts: `::INT`, `::FLOAT8`, `::BOOLEAN`, `::TEXT`, `::DATE`, `::TIMESTAMP`, `::UUID`, `::NUMERIC` | ✅ |
| Text → numeric casts including `'Infinity'::FLOAT8`, `'NaN'::FLOAT8` | ✅ |
| SAVEPOINT / ROLLBACK TO SAVEPOINT / RELEASE SAVEPOINT | ✅ (partial — aborts full txn, no page-level undo) |
| SET TRANSACTION ISOLATION LEVEL | ✅ (accepted, treated as no-op) |
| SERIAL / DEFAULT values | ✅ |
| CHECK constraints | ✅ |
| FOREIGN KEY constraints | ✅ |
| GRANT / REVOKE | ✅ |
| VACUUM ANALYZE | ✅ |
| UPSERT (ON CONFLICT DO UPDATE) | ❌ not implemented |
| ALTER TABLE | ❌ not implemented |
| Window functions (full — PARTITION BY, frame specs) | ⚠️ basic only |
| NATURAL JOIN | ❌ not implemented |
| ROLLUP / CUBE / GROUPING SETS | ❌ not implemented |
| Procedural language (PL/pgSQL) | ❌ not implemented |
| CREATE SEQUENCE | ❌ not implemented |
| Array types | ❌ not implemented |
| JSON / JSONB types | ❌ not implemented |
| Triggers | ❌ not implemented |
| Table inheritance | ❌ not implemented |
| FK constraint cascades (ON DELETE CASCADE etc.) | ❌ not implemented |
| TRIM(BOTH FROM ...) syntax | ❌ blocked by sqlparser-rs 0.53 parser limitation |
| Unit tests in `sql` crate (73) | ✅ |
| Unit tests in `network` crate (109 in-process SQL conformance) | ✅ |

### Phase 7 — Network Layer & PostgreSQL Wire Protocol ✅ COMPLETE

| Item | Status |
|------|--------|
| `pgwire` crate integration | ✅ |
| Startup handshake and parameter exchange | ✅ |
| SCRAM-SHA-256 multi-step handshake | ✅ |
| RBAC enforcement | ✅ |
| Simple Query protocol (Q message → rows → CommandComplete) | ✅ |
| Extended Query protocol (Parse → Bind → Execute → Sync + plan cache) | ✅ |
| ErrorResponse formatting (severity, SQLSTATE code, message) | ✅ |
| Multi-statement batches, transaction control over wire | ✅ |
| Unit tests (7 in `server` crate) | ✅ |
| `psql` real-connection smoke test | ⚠️ implemented but not automated |

### Phase 8 — CLI (nkv-psql) ✅ COMPLETE

| Item | Status |
|------|--------|
| `rustyline` REPL with persistent history | ✅ |
| `--host`, `--port`, `--user`, `--dbname` flags | ✅ |
| SQL keyword + table name auto-completion | ✅ |
| ASCII table result rendering (`tabled`) | ✅ |
| Meta-commands: `\d`, `\dt`, `\du`, `\l`, `\q`, `\i <file>`, `\e` | ✅ |
| `\timing` and `\x` (expanded output) | ✅ |
| `\dump <path>` / `\restore <path>` — logical backup/restore | ✅ |
| `.pgpass` file support | ❌ not implemented |
| Unit tests (10) | ✅ |

### Phase 9 — Cross-Language Drivers ⚠️ STUB ONLY
Directories exist under `drivers/` but no implementation beyond stubs.

| Item | Status |
|------|--------|
| `drivers/rust` — async connection pool, Arrow output | ❌ stub |
| `drivers/python` — PyO3/Maturin bindings | ❌ stub |
| `drivers/nodejs` — NAPI-RS bindings, TypeScript types | ❌ stub |

### Phase 10 — ACID Verification & Production Hardening ⚠️ PARTIAL

| Item | Status |
|------|--------|
| Hermitage isolation suite (G0, G1a, G1b, G1c, P4, G2-item) | ✅ all pass |
| ACID integration tests (atomicity, consistency, isolation, durability) | ✅ 19 tests |
| Concurrency multi-thread tests | ✅ 6 tests |
| VACUUM / autovacuum daemon (60s sweep, 5-min threshold) | ✅ |
| ANALYZE / pg_statistic histogram update | ❌ not implemented |
| Bank-transfer fault-injection (SIGKILL mid-transfer) | ❌ not automated |
| Power-off durability simulation | ❌ not automated |
| `pgbench` TPC-B baseline | ❌ not run |
| DBeaver / pgAdmin connection compatibility | ❌ not verified |
| Connection limit enforcement | ❌ not implemented |
| Graceful shutdown (SIGTERM) | ❌ not implemented |
| OOM-safe buffer pool (fixed allocation, no dynamic growth) | ❌ not implemented |

---

## Test suite summary

| Workspace | Location | Tests | Ignored | Failed |
|-----------|----------|-------|---------|--------|
| Unit tests | `crates/` (root workspace) | **253** | 0 | 0 |
| Integration tests | `tests/` (separate workspace) | **715** | 4 | 0 |
| **Total** | | **968** | **4** | **0** |

### Unit test breakdown (253 total)

| Crate | Tests | Coverage |
|-------|-------|----------|
| `auth` | 6 | Password hashing, SCRAM-SHA-256, role lookup |
| `btree` | 10 | B+ tree insert/search/delete, range scan, splits |
| `catalog` | 11 | Table/column/index registration, ACL, stats |
| `txn` | 10 | Snapshot visibility, MVCC, isolation state machine |
| `storage` | 4 | Page layout, slotted-page, buffer pool eviction |
| `sql` | 73 | Parser, planner, optimizer, executor unit tests |
| `network` (in-process) | 109 | SQL conformance via in-process engine (no TCP) |
| `wal` | 13 | Record format, segment rotation, recovery |
| `cli` | 10 | CLI flag parsing, meta-command dispatch |
| `server` | 7 | Server startup, TLS config, connection limit |

### Integration test breakdown (715 total)

**`sql_conformance` — 711 tests across 24 modules** (from PostgreSQL regression suite, Hermitage, TPC-H)

| Module | Tests | Source |
|--------|-------|--------|
| `joins` | 50 | `join.sql` |
| `set_operations` | 48 | `union.sql` |
| `select` | 43 | `select.sql` |
| `aggregates` | 42 | `aggregates.sql` |
| `subqueries` | 41 | `subselect.sql` |
| `dml` | 41 | `insert.sql`, `update.sql`, `delete.sql` |
| `null_handling` | 37 | PostgreSQL NULL semantics |
| `transactions` | 34 | `transactions.sql` |
| `string_functions` | 34 | `strings.sql` |
| `int_types` | 31 | `int4.sql`, `int8.sql` |
| `boolean_type` | 31 | `boolean.sql` |
| `ctes` | 30 | `with.sql` |
| `new_features` | 28 | IceDB-specific (UUID, SERIAL, CHECK, FK, GRANT, UPSERT) |
| `limit_offset` | 26 | `limit.sql` |
| `case_expr` | 23 | `case.sql` |
| `float_types` | 21 | `float8.sql` |
| `ddl` | 20 | PostgreSQL DDL regression |
| `catalog_views` | 20 | `information_schema`, `pg_class`, COPY, PREPARE |
| `error_handling` | 18 | PostgreSQL SQLSTATE codes |
| `date_type` | 17 | `date.sql` |
| `advanced_features` | 17 | LISTEN/NOTIFY, CREATE FUNCTION, cost-based optimizer, pg_dump |
| `timestamp_type` | 15 | `timestamp.sql` |
| `tpch` | 14 | TPC-H Q1, Q3, Q5, Q6, Q10 (simplified schema) |
| `hermitage` | 12 | Hermitage isolation anomaly suite |

**Other integration suites**

| Suite | Tests | Coverage |
|-------|-------|----------|
| `acid` | 19 | Atomicity, consistency, isolation, durability |
| `concurrency` | 6 | Multi-threaded G0, G1a, G1b, P4, snapshot read |
| `tutorial_validation` | 1 | Every SQL example from the tutorial chapter |

### The 4 ignored tests

| Test | Reason |
|------|--------|
| `dml::test_insert_on_conflict_do_nothing` | `ON CONFLICT DO NOTHING` not implemented |
| `dml::test_insert_on_conflict_do_update` | `ON CONFLICT DO UPDATE` not implemented |
| `joins::test_join_natural_join` | `NATURAL JOIN` not implemented |
| `string_functions::test_string_trim_both_explicit` | `TRIM(BOTH FROM ...)` not parsed by sqlparser-rs 0.53 |

---

## PostgreSQL features NOT implemented

These were explicitly excluded because the feature doesn't exist in icedb yet:

| Feature | PostgreSQL test file |
|---------|---------------------|
| `ALTER TABLE` | `alter_table.sql` |
| Window functions (full frame specs, PARTITION BY) | `window.sql` |
| FK constraint cascades | `foreign_key.sql` |
| PL/pgSQL procedural language | `plpgsql.sql` |
| `CREATE SEQUENCE` | `sequence.sql` |
| Array types | `arrays.sql` |
| JSON / JSONB | `json.sql` |
| Triggers | `triggers.sql` |
| Rule system | `rules.sql` |
| Table inheritance | `inherit.sql` |
| Table-returning functions | `rangefuncs.sql` |
| `ROLLUP` / `CUBE` / `GROUPING SETS` | `groupingsets.sql` |

---

## Key architectural decisions made

- **SAVEPOINT partial rollback**: True page-level undo is not implemented. `ROLLBACK TO SAVEPOINT` aborts the entire transaction and starts a new one. Tests reflect this actual behaviour.
- **SET TRANSACTION**: Accepted and parsed; treated as no-op (isolation level is set per-engine, not per-statement).
- **`tests/` is a separate Cargo workspace**: `cargo test --workspace` from root only covers the 253 unit tests. Both workspaces must be run to get the full 968. See `tests/TEST-ARCHITECTURE.md`.
- **No external data files**: PostgreSQL regression tests that require large pre-loaded tables (`onek`, `tenk1`, `aggtest`) via `COPY FROM` were replaced with self-contained tests using inline `INSERT` data.

---

## Highest-priority next steps

These are the gaps most likely to matter for a "production-ready" claim, roughly in priority order:

1. **ON CONFLICT (UPSERT)** — blocked 2 integration tests; needed for practical use
2. **ALTER TABLE** — essential for schema evolution, currently completely absent
3. **OOM-safe buffer pool** — current implementation can grow without bound under load
4. **Connection limit + graceful SIGTERM shutdown** — needed for production deployment
5. **ANALYZE** — without it, `pg_statistic` histograms are never updated; cost-based optimizer degrades over time
6. **Fault-injection tests** — bank-transfer SIGKILL and power-off recovery are untested automatically
7. **Cross-language drivers** — Python and Node.js stubs need actual implementation
8. **NATURAL JOIN** — simple to add; unblocks 1 ignored test
9. **Full window functions** (PARTITION BY, frame specs) — currently basic OVER/ORDER BY only
10. **pgbench / DBeaver compatibility** — validates real-world PostgreSQL client compatibility

---

## Files to read at session start

| File | Purpose |
|------|---------|
| `CLAUDE.md` | Build commands, architecture overview, phase definitions |
| `STATUS.md` (this file) | Current implementation state and priorities |
| `tests/TEST-ARCHITECTURE.md` | Test suite structure, run instructions, per-module breakdown |
| `crates/sql/src/executor.rs` | Core query execution (largest, most-changed file) |
| `crates/sql/src/planner.rs` | AST → logical plan conversion |
| `crates/sql/src/plan.rs` | LogicalPlan and Expr enums |
| `crates/sql/src/engine.rs` | QueryEngine, session management, SAVEPOINT |
| `crates/sql/src/value.rs` | Value type, cast_to(), arithmetic |
