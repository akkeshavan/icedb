# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**icedb** is a production-grade, PostgreSQL-compatible RDBMS built in Rust. It implements the full PostgreSQL wire protocol (v3.0), ACID transactions via WAL + MVCC, a page-based storage engine, persistent B+ tree indexes, and cross-language drivers (Rust, Python, JS/TS).

## Workspace Structure

This is a Cargo workspace. Each major subsystem is its own crate:

```
icedb/
├── crates/
│   ├── storage/        # Page layout, buffer manager, heap files
│   ├── btree/          # Persistent B+ tree index
│   ├── wal/            # Write-Ahead Log
│   ├── txn/            # Transaction manager, MVCC, snapshot isolation
│   ├── catalog/        # System catalogs (pg_class, pg_attribute, pg_authid)
│   ├── sql/            # Parser (sqlparser-rs), planner, optimizer, executor
│   ├── network/        # PostgreSQL wire protocol (pgwire crate)
│   ├── auth/           # SCRAM-SHA-256, RBAC
│   ├── server/         # Top-level server binary wiring all crates
│   └── cli/            # isql CLI (rustyline)
├── drivers/
│   ├── rust/           # Native Rust client crate
│   ├── python/         # PyO3/Maturin Python bindings
│   └── nodejs/         # NAPI-RS Node.js bindings
├── docs/
│   └── Specs-RDBMS-rust.md
├── tests/              # Integration and ACID compliance tests
└── Cargo.toml          # Workspace manifest
```

## Build & Development Commands

```bash
# Build all crates
cargo build --workspace

# Build release (optimized)
cargo build --workspace --release

# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p storage
cargo test -p btree
cargo test -p txn

# Run a single test by name
cargo test -p storage page_header_layout
cargo test -p txn -- mvcc_visibility --nocapture

# Lint (must pass before committing)
cargo clippy --workspace --all-targets -- -D warnings

# Format check
cargo fmt --check

# Format in-place
cargo fmt --all

# Run the server
cargo run -p server -- --port 5432 --data-dir ./data

# Run the CLI
cargo run -p cli -- --data-dir ./data -U icedb
```

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `pgwire` | PostgreSQL wire protocol server-side implementation |
| `sqlparser` | SQL parsing (PostgreSQL dialect) |
| `tokio` | Async runtime for the network layer |
| `bytes` | Zero-copy byte buffer manipulation |
| `pyo3` + `maturin` | Python bindings for drivers |
| `napi` + `napi-derive` | Node.js bindings for drivers |
| `arrow` | Apache Arrow for zero-copy columnar data transfer |
| `rustyline` | Line editing and history for the CLI |
| `sha2` + `hmac` | SCRAM-SHA-256 authentication |

## Architecture: Data Flow

```
Client (psql / app driver)
        │  TCP (PostgreSQL Wire Protocol v3.0)
        ▼
  network/ crate  ←→  auth/ (SCRAM-SHA-256, RBAC)
        │
        ▼
  sql/ crate
    ├── Parser (sqlparser-rs → AST)
    ├── Semantic Analyzer (validates against catalog/)
    ├── Planner / Cost-Based Optimizer (uses pg_statistic)
    └── Executor (Volcano/iterator model)
           │  calls
           ▼
  txn/ crate (MVCC snapshot, visibility rules, lock manager)
           │
           ▼
  storage/ crate (buffer manager, heap files)
  btree/   crate (persistent B+ tree index pages)
           │
           ▼
  wal/ crate (WAL writer, checkpointing, recovery)
           │
           ▼
       Disk (8 kB pages)
```

## Core Data Model

- **Page size**: 8 kB. Every table row and index node lives in an 8 kB page.
- **Page header** (24 bytes): `pd_lsn`, `pd_checksum`, `pd_flags`, `pd_lower`, `pd_upper`, `pd_special`, `pd_version`, `pd_prune_xid`. Slotted layout — item pointers grow down from `pd_lower`, tuple data grows up from `pd_upper`.
- **Tuple header**: `t_xmin`, `t_xmax`, `t_cid`, `t_ctid` (version chain pointer), `t_infomask`, optional null bitmap. Data starts at `t_hoff` (8-byte aligned).
- **MVCC visibility**: A tuple version is visible if `xmin` committed before the snapshot and `xmax` is absent/aborted/future; also visible if `xmin` equals the current transaction's own XID and `xmax` is 0 (read-own-writes).
- **WAL rule**: No data page hits disk before its WAL record is fsynced.
- **Buffer manager**: Clock/Second-Chance eviction. Pinned pages cannot be evicted. Dirty pages flushed by background writer.
- **B+ tree**: Each node = one 8 kB page. Leaf nodes hold TIDs (page, offset). `pd_special` stores left/right sibling pointers. Structural modifications (splits/merges) are WAL-logged.

## Isolation Levels

| Level | Behavior |
|-------|----------|
| Read Committed | Per-statement snapshot |
| Repeatable Read | Per-transaction snapshot |
| Serializable | SSI — detect dependency cycles |

## Implementation Phases

### Phase 1 — Storage Foundation
**Goal**: Raw page I/O, buffer pool, heap file CRUD. No SQL, no network.

- [ ] Cargo workspace scaffold with all crate stubs
- [ ] `storage`: `PageHeader` struct with exact byte layout (24-byte header)
- [ ] `storage`: Page checksum (FNV or CRC32C)
- [ ] `storage`: Slotted-page item identifier array (`pd_lower`/`pd_upper`)
- [ ] `storage`: Tuple header struct with MVCC fields (`t_xmin`, `t_xmax`, `t_cid`, `t_ctid`, `t_infomask`, null bitmap)
- [ ] `storage`: Heap file — insert, read, and delete tuples by TID
- [ ] `storage`: `BufferPool` with fixed-size frame array
- [ ] `storage`: Clock-sweep eviction algorithm
- [ ] `storage`: Pin/unpin, dirty marking, background flush thread
- [ ] Unit tests: page layout byte offsets, checksum validation, insert/read/eviction

**Gate**: `cargo test -p storage` passes 100%.

---

### Phase 2 — Write-Ahead Log & Crash Recovery
**Goal**: Durable WAL with redo-only recovery. Checkpointing.

- [ ] `wal`: LSN type (`u64`, monotonically increasing)
- [ ] `wal`: WAL record format (LSN, type, page ref, before/after image)
- [ ] `wal`: Sequential segment files, append-only writer
- [ ] `wal`: `fsync` on transaction commit
- [ ] `wal`: Checkpoint: flush all dirty buffer pages then write checkpoint record
- [ ] `wal`: Redo recovery: replay from last checkpoint LSN on startup
- [ ] `storage`: Stamp `pd_lsn` on every page write; validate during recovery
- [ ] Integration test: write records, crash-simulate (kill writer mid-batch), recover and verify state

**Gate**: `cargo test -p wal` and recovery integration test pass 100%.

---

### Phase 3 — Transaction Manager & MVCC
**Goal**: Full ACID transaction lifecycle with snapshot isolation.

- [ ] `txn`: Transaction ID (`u64`) allocator (global atomic)
- [ ] `txn`: `Snapshot` struct: `xmin`, `xmax`, `in_progress` set
- [ ] `txn`: Visibility function: `is_visible(tuple_header, snapshot) -> bool`
- [ ] `txn`: `BEGIN`, `COMMIT`, `ROLLBACK` state machine
- [ ] `txn`: Read Committed isolation (per-statement snapshot)
- [ ] `txn`: Repeatable Read isolation (per-transaction snapshot)
- [ ] `txn`: Two-phase locking for write-write conflicts
- [ ] `txn`: Dead tuple tracking; hook for future VACUUM
- [ ] `txn`: Serializable Snapshot Isolation (SSI) with rw-antidependency tracking
- [ ] Tests: dirty read prevention, non-repeatable read prevention, phantom prevention, bank-transfer atomicity test

**Gate**: `cargo test -p txn` passes including all isolation anomaly tests.

---

### Phase 4 — Persistent B+ Tree Index
**Goal**: Crash-safe B+ tree stored as 8 kB pages, WAL-logged.

- [ ] `btree`: Internal node page format (keys + child page pointers)
- [ ] `btree`: Leaf node page format (keys + TIDs) with left/right sibling pointers in `pd_special`
- [ ] `btree`: Metapage (page 0): root pointer, tree height
- [ ] `btree`: Search (root-to-leaf with shared latches)
- [ ] `btree`: Insert with leaf split and key promotion (latch crabbing)
- [ ] `btree`: Delete with merge/redistribute
- [ ] `btree`: Range scan using sibling chain
- [ ] `btree`: WAL-log all SMOs (structural modification operations)
- [ ] `btree`: Recovery replays SMOs correctly
- [ ] Tests: correctness under random inserts/deletes, range scans, recovery after mid-split crash

**Gate**: `cargo test -p btree` passes 100%.

---

### Phase 5 — System Catalog & Schema Management
**Goal**: In-engine system tables for metadata (pg_class, pg_attribute, pg_authid, pg_statistic).

- [ ] `catalog`: Bootstrap system tables as heap files with known OIDs
- [ ] `catalog`: `pg_class` — table/index registry
- [ ] `catalog`: `pg_attribute` — column definitions and types
- [ ] `catalog`: `pg_authid` — roles with privilege flags; `pg_roles` view
- [ ] `catalog`: `pg_statistic` — column histograms and MCVs
- [ ] `catalog`: DDL execution: `CREATE TABLE`, `DROP TABLE`, `CREATE INDEX`
- [ ] `catalog`: Constraint tracking: NOT NULL, UNIQUE, PRIMARY KEY, FOREIGN KEY
- [ ] `catalog`: Schema namespace support
- [ ] Tests: catalog round-trips, constraint enforcement

**Gate**: `cargo test -p catalog` passes 100%.

---

### Phase 6 — SQL Engine (Parser → Executor)
**Goal**: End-to-end SQL query processing using the Volcano model.

- [x] `sql`: Integrate `sqlparser-rs` with PostgreSQL dialect
- [x] `sql`: Semantic analyzer — resolve table/column names against catalog
- [x] `sql`: Logical plan nodes: Scan, Filter, Project, Join, Aggregate, Sort, Limit
- [x] `sql`: Cost-based optimizer: Filter(TableScan) → IndexScan rewrite on equality predicates; statistics-driven selectivity estimates are planned
- [x] `sql`: Physical plan operators: `SeqScan`, `IndexScan`, `HashJoin`, `MergeJoin`, `NestedLoop`, `Sort`, `HashAgg`
- [x] `sql`: Volcano iterator model: each operator implements `next() -> Option<Tuple>`
- [x] `sql`: DML: `INSERT`, `UPDATE`, `DELETE` with MVCC tuple versioning
- [x] `sql`: `EXPLAIN` / `EXPLAIN ANALYZE`
- [x] `sql`: Subqueries, CTEs, window functions (basic)
- [ ] Tests: TPC-H query subset, join correctness, aggregate correctness

**Gate**: `cargo test -p sql` passes 100%.

---

### Phase 7 — Network Layer & PostgreSQL Wire Protocol
**Goal**: Accept real psql connections; run SQL end-to-end over the wire.

- [ ] `network`: Integrate `pgwire` crate as the protocol server
- [ ] `network`: Startup handshake and parameter exchange
- [ ] `auth`: SCRAM-SHA-256 multi-step handshake
- [ ] `auth`: RBAC enforcement — check role privileges before executing statements
- [ ] `network`: Simple Query protocol (`Q` message → result rows → `CommandComplete`)
- [ ] `network`: Extended Query protocol: `Parse` → `Bind` → `Execute` → `Sync` with plan caching
- [ ] `network`: Error response formatting (`ErrorResponse` with severity, code, message)
- [ ] `network`: Multi-statement batches; transaction control over wire
- [ ] Integration test: connect with real `psql`, run CRUD, verify results

**Gate**: `psql -h localhost -U icedb` connects and executes all basic SQL (icedb is PostgreSQL wire-protocol compatible); `cargo test -p network` passes.

---

### Phase 8 — CLI (isql)
**Goal**: A psql-compatible interactive terminal.

- [ ] `cli`: `rustyline`-based REPL with persistent history
- [ ] `cli`: Connection flags: `--host`, `--port`, `--user`, `--dbname`; env vars `PGHOST`, `PGPORT`, `PGUSER`
- [ ] `cli`: `.pgpass` file support for passwords (not yet implemented)
- [ ] `cli`: SQL keyword + table name auto-completion
- [ ] `cli`: Result rendering with `tabled` (ASCII table)
- [ ] `cli`: Meta-commands: `\d`, `\dt`, `\du`, `\l`, `\q`, `\i <file>`, `\e`
- [ ] `cli`: `\timing` and `\x` (expanded output)
- [x] `cli`: `\dump path` and `\restore path` — logical backup and restore via SQL statements
- [ ] Tests: CLI flag parsing, meta-command dispatch

**Gate**: `cargo test -p cli` passes; manual smoke test against running server.

---

### Phase 9 — Cross-Language Drivers
**Goal**: Native drivers for Rust, Python, and Node.js with zero-copy Arrow output.

- [ ] `drivers/rust`: Connection pool, async query API using `tokio`
- [ ] `drivers/rust`: Apache Arrow `RecordBatch` output for large result sets
- [ ] `drivers/python`: PyO3 + Maturin build; type mapping (Rust → Python primitives)
- [ ] `drivers/python`: `asyncio` integration via `tokio`
- [ ] `drivers/nodejs`: NAPI-RS bindings; TypeScript type definitions
- [ ] `drivers/nodejs`: Promise-based async API
- [ ] Tests: round-trip query tests in Python and Node.js; Arrow zero-copy benchmark

**Gate**: Python `pip install` and Node.js `npm install` work; driver integration tests pass.

---

### Phase 10 — ACID Verification & Production Hardening
**Goal**: Pass all ACID compliance tests and PostgreSQL compatibility benchmarks.

- [ ] Atomicity: Bank-transfer fault-injection test (SIGKILL mid-transfer, verify balance invariant)
- [ ] Consistency: Schema constraint stress test (UNIQUE, NOT NULL, FK under concurrent load)
- [x] Isolation: Hermitage anomaly suite (G1a, G1b, G1c, P4, G2-item, G2, etc.) — MVCC read-own-writes fix landed; full suite passes at documented levels
- [ ] Durability: Power-off simulation — abrupt kill under heavy write load, verify recovery to last committed LSN
- [ ] `pgbench` TPC-B baseline: measure tps at varying concurrency levels
- [ ] DBeaver / pgAdmin connection compatibility check
- [x] VACUUM: reclaim dead tuple space, update `pd_prune_xid`; autovacuum daemon implemented (`QueryEngine::start_autovacuum`, 60s sweep, 5 min threshold)
- [ ] `ANALYZE`: update `pg_statistic` histograms
- [ ] Connection limit enforcement, graceful shutdown (`SIGTERM`)
- [ ] OOM-safe buffer pool (fixed allocation, no dynamic growth)

**Gate**: All Hermitage tests pass at documented isolation levels; bank-transfer and power-off tests pass; pgbench runs to completion.

## Testing Philosophy

- Each phase has a hard gate: **all tests for that phase must pass before work on the next phase begins.**
- Integration tests live in `tests/` and spin up a real server process.
- Use `cargo test --workspace` for a full sweep before any phase gate sign-off.
- Fault-injection tests use `SIGKILL` on the server process and file-level corruption to simulate crashes.
- The Hermitage suite is the authoritative isolation verification — run it against the live server via `psql`.
