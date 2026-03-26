# Known Issues

**Last updated**: 2026-03-26
**Test baseline**: 313 unit + 761 integration = 1,074 passing, 0 ignored, 0 failing

> CLI issues 18–21 resolved in the same session (see CLI / Tooling section below).

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

| # | Issue |
|---|-------|
| 22 | **OOM-safe buffer pool** — buffer pool can grow without bound under load; no fixed allocation ceiling |
| 23 | **Connection limit enforcement** — no max-connections cap; server accepts unlimited clients |
| 24 | **Graceful `SIGTERM` shutdown** — no signal handler; process exits abruptly, may corrupt in-flight writes |
| 25 | **`unwrap()` audit** — 1,171+ `unwrap()` calls workspace-wide; any unexpected state panics the thread |
| 26 | **Deadlock detection / timeout** — two-phase locking implemented but no deadlock detector; waiting transactions can hang forever |
| 27 | **SSI cycle detection** — SSI rw-antidependency tracking code exists but cycle detection is incomplete |

---

## Testing / Validation Gaps

| # | Issue |
|---|-------|
| 28 | **Bank-transfer fault-injection test** — SIGKILL mid-transfer not automated |
| 29 | **Power-off durability simulation** — abrupt kill under write load and recovery verification not automated |
| 30 | **`pgbench` TPC-B baseline** — never run; no throughput/concurrency baseline established |
| 31 | **DBeaver / pgAdmin compatibility** — not verified |
| 32 | **`psql` real-connection smoke test** — implemented but not automated in CI |

---

## Drivers (Phase 9 — All Stubs)

| # | Issue |
|---|-------|
| 33 | **Rust async driver** — stub only; no connection pool, no Arrow output |
| 34 | **Python driver (PyO3/Maturin)** — stub only |
| 35 | **Node.js driver (NAPI-RS)** — stub only |
