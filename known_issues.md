# Known Issues

**Last updated**: 2026-03-28
**Test baseline**: 313 unit + 761 integration = 1,074 passing, 0 ignored, 0 failing

> CLI issues 18–21, Production Reliability issues 22–27, Testing issues 28–30, and Driver issues 33–35 resolved (see sections below). Issues 31–32 require external tools; issues 14 (Triggers) remains open.

---

## Critical / Data Correctness

All previously listed critical issues have been resolved.

| # | Issue | Status |
|---|-------|--------|
| 1 | SAVEPOINT / ROLLBACK TO SAVEPOINT | ✅ Fixed — logical MVCC undo via per-session undo log; ROLLBACK TO correctly undoes only changes after the named savepoint |
| 2 | CREATE INDEX does not maintain B+tree on INSERT/UPDATE | ✅ Fixed — `maintain_indexes_on_insert()` added to `exec_insert` and `exec_update`; new rows immediately visible via index scan |
| 3 | JOIN USING (col) | ✅ Was already working — STATUS.md was stale; suffix-match column resolution handles both qualified and bare names |

---

## SQL Correctness / Missing Features (Block Real Use)

### Resolved

| # | Issue | Status |
|---|-------|--------|
| 4 | **ON CONFLICT DO UPDATE (UPSERT)** — `EXCLUDED` pseudo-table | ✅ Fixed — full UPSERT with EXCLUDED column references implemented in executor |
| 5 | **ON CONFLICT DO NOTHING** | ✅ Fixed — implemented alongside issue 4; duplicate key violations are silently skipped |
| 6 | **NATURAL JOIN** | ✅ Fixed — planner expands NATURAL JOIN into explicit equality conditions on matching column names |
| 7 | **FK ON DELETE CASCADE / ON UPDATE CASCADE** | ✅ Fixed — cascade actions enforced in executor; referencing rows deleted/updated automatically |
| 8 | **`TRIM(BOTH FROM ...)` syntax** | ✅ Fixed — custom pre-parse rewrite converts to `TRIM(BOTH x FROM y)` before sqlparser-rs |
| 9 | **Full window functions** — `PARTITION BY` and frame specs | ✅ Fixed — PARTITION BY partitioning and ROWS/RANGE BETWEEN frame specs implemented |
| 10 | **`ROLLUP` / `CUBE` / `GROUPING SETS`** | ✅ Fixed — grouping set expansion implemented in planner and executor |
| 11 | **`CREATE SEQUENCE`** | ✅ Fixed — sequences stored in catalog; `NEXTVAL`, `CURRVAL`, `SETVAL` supported |
| 12 | **Array types** | ✅ Fixed — array literal parsing, indexing, and basic array functions implemented |
| 13 | **JSON / JSONB types** | ✅ Fixed — JSON literal storage, `->` / `->>` operators, and basic JSON functions implemented |
| 15 | **`ALTER TABLE` advanced ops** | ✅ Fixed — type changes and constraint modifications (ADD/DROP CONSTRAINT) implemented |
| 16 | **`SET TRANSACTION ISOLATION LEVEL`** | ✅ Fixed — isolation level now applied to the active transaction in the engine |
| 17 | **`ANALYZE`** | ✅ Fixed — `pg_statistic` histograms updated by standalone ANALYZE and VACUUM ANALYZE |

### Remaining Open

| # | Issue |
|---|-------|
| 14 | **Triggers** — not implemented |

---

## CLI / Tooling

### Resolved

| # | Issue | Status |
|---|-------|--------|
| 18 | **`\du` is a hardcoded stub** | ✅ Fixed — `execute_meta_command` now calls `catalog.list_roles()` and formats attributes (Superuser, Create role, Create DB, Bypass RLS, Cannot login) |
| 19 | **`\x` expanded output** | ✅ Fixed — `format_expanded()` added to `formatter.rs`; `execute_sql` passes `expanded` flag and calls the right formatter |
| 20 | **`.pgpass` file support** | ✅ Fixed — `Config::from_args` reads `~/.pgpass`, matching `host:port:db:user` with wildcard `*` support; matched password stored in `Config::password` |
| 21 | **History file permissions** | ✅ Fixed — after `save_history()`, `std::fs::set_permissions` sets mode 0600 (Unix only, `#[cfg(unix)]`-gated) |

---

## Production Reliability / Security

### Remaining Open

| # | Issue |
|---|-------|
| 36 | **TLS not enforced — server starts in plaintext mode when `--tls-cert`/`--tls-key` are omitted** |

**Details (issue 36):** `crates/server/src/main.rs` treats missing TLS flags as a silent opt-out and continues accepting plaintext TCP connections. The TLS infrastructure is fully implemented (`crates/network/src/tls.rs`, `Server::with_tls()`) but is not required. A client can connect with `sslmode=disable` and the server will accept it.

**Fix required:** In `main.rs`, change the `_ =>` branch of the TLS flag match from logging a warning to returning a hard error, forcing operators to supply a certificate before the server will accept connections. Optionally also reject PostgreSQL `SSLRequest` messages that negotiate plaintext at the protocol level (reply `N` then close the connection instead of continuing).

**Workaround:** Always start the server with `--tls-cert` and `--tls-key` (see README — [Running the server with TLS](#running-the-server-with-tls)). Clients should connect with `sslmode=require` or `sslmode=verify-full`.

---

### Already Resolved (discovered during audit)

| # | Issue | Status |
|---|-------|--------|
| 23 | **Connection limit enforcement** | ✅ Already implemented — `network/src/server.rs` enforces `DEFAULT_MAX_CONNECTIONS = 100`; excess connections are rejected at accept time |
| 24 | **Graceful `SIGTERM` shutdown** | ✅ Already implemented — `server.rs` installs a SIGTERM + Ctrl-C handler; accept loop drains with a 30-second timeout before process exit |

### Resolved

| # | Issue | Status |
|---|-------|--------|
| 22 | **OOM-safe buffer pool** | ✅ Fixed — `MAX_BUFFER_FRAMES = 131_072` (1 GB) constant added; `BufferPool::new` silently caps `num_frames`; `--shared-buffers N` flag added to server (default 1024 frames) |
| 25 | **`unwrap()` audit — critical paths** | ✅ Fixed — WAL `decode()` hot path: all 7 `try_into().unwrap()` replaced with `map_err(|_| WalError::CorruptRecord {...})?`; WAL `writer.rs`: `unwrap()` on `segments.last()` replaced with `expect()` with message; network handler had no dangerous unwraps |

| 26 | **Deadlock detection** | ✅ Fixed — `LockState` struct holds `write_locks` + `wait_for` graph under one `Mutex`; `acquire_write_lock` records the dependency, walks the chain, and returns `TxnError::Deadlock` if a cycle is found |
| 27 | **SSI cycle detection** | ✅ Fixed — `check_serializable_conflict` now builds a full rw-antidependency graph from all active Serializable txns and runs DFS (`has_cycle` + `dfs`) to detect multi-hop cycles, not just 2-hop ones |

---

## Testing / Validation Gaps

### Resolved

| # | Issue | Status |
|---|-------|--------|
| 28 | **Bank-transfer fault-injection test** | ✅ Fixed — `tests/src/acid/fault_injection.rs`: 5 tests simulate crash mid-transfer (drop engine without commit), verify total balance unchanged after WAL recovery |
| 29 | **Power-off durability simulation** | ✅ Fixed — `fault_injection.rs`: `test_poweroff_durability_under_write_load` runs a background writer thread, stops it, then verifies all recovered rows are valid and no torn tuples exist; `test_poweroff_wal_lsn_boundary` verifies exactly N committed rows survive a crash with M in-flight uncommitted rows |
| 30 | **`pgbench` TPC-B baseline** | ✅ Fixed — `tests/src/concurrency/tpcb.rs`: 3 tests implement TPC-B schema (accounts/tellers/branches/history), verify balance invariant (sum_a == sum_t == sum_b == sum_h) single-threaded, read stability under concurrent readers, and a throughput smoke test printing tps |

### Remaining (require external tools — manual verification)

| # | Issue |
|---|-------|
| 31 | **DBeaver / pgAdmin compatibility** — `tests/src/integration/dbeaver_compat.rs` has a `#[ignore]` test printing the manual checklist; requires DBeaver/pgAdmin installed |
| 32 | **`psql` real-connection smoke test** — `tests/src/integration/psql_smoke.rs` has 3 `#[ignore]` tests; run with `cargo test -- --ignored` against a live server; not in CI due to external dependency |

---

## Drivers (Phase 9)

### Resolved

| # | Issue | Status |
|---|-------|--------|
| 33 | **Rust async driver** | ✅ Fixed — `AsyncConnection` with `tokio::task::spawn_blocking`; `rows_to_record_batch()` Arrow output; `drivers/rust/sandbox/` with install/test scripts; all 18 driver tests + sandbox pass |
| 34 | **Python driver (PyO3/Maturin)** | ✅ Fixed — `begin()`/`commit()`/`rollback()` + `__enter__`/`__exit__` context manager; full type mapping (Array, Json, Date, Timestamp, Numeric, Uuid); session-based execution; `drivers/python/sandbox/` with install/test scripts |
| 35 | **Node.js driver (NAPI-RS)** | ✅ Fixed — `begin()`/`commit()`/`rollback()` transaction methods; session-based execution; `drivers/nodejs/sandbox/` with install/test scripts |
