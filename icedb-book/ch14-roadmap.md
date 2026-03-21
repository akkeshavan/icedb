# Chapter 14 — Roadmap & Known Limitations

This chapter documents every significant feature that icedb does **not yet implement**, explains *why* it is absent, and describes what would be required to add it. The goal is transparency: if you are evaluating icedb for a production workload, or contributing to the project, this chapter tells you exactly where the boundaries are and what is left to build.

---

## How to read this chapter

Features are grouped by the kind of work required to implement them:

| Category | Meaning |
|---|---|
| **Missing SQL** | The engine exists but the SQL surface area is incomplete |
| **Infrastructure gap** | A whole subsystem needs to be built before the feature is possible |
| **Design decision** | Deliberately deferred or out of scope for the current phase |

Current test count: **325 passing, 0 failing, 0 ignored.**

---

## Missing SQL features

These features require changes to the SQL planner, executor, or catalog only — no new infrastructure is needed.

### PL/pgSQL stored procedures

**What is missing:** `CREATE FUNCTION … LANGUAGE plpgsql` with procedural control flow — `IF`, `LOOP`, `RAISE`, variables, exception handling.

**What is implemented:** `LANGUAGE SQL` functions (a single `SELECT` expression, parameters substituted as `$1`/`$2` literals, single scalar return value).

**Why it is not done:** PL/pgSQL is a full interpreted language. Implementing it requires a bytecode interpreter or AST evaluator for the PL/pgSQL grammar, a separate parser pass (sqlparser-rs does not parse PL/pgSQL bodies), and a per-call execution context with local variable slots. It is a substantial self-contained project, not a small extension. A practical path is to implement a minimal subset — `IF/ELSE`, simple `LOOP`, `RETURN`, and `RAISE NOTICE` — before attempting the full spec.

**Effort:** Large (2–4 weeks). Not blocked by any other item.

---

### `WITH RECURSIVE … SEARCH` and `CYCLE` clauses

**What is missing:**
```sql
WITH RECURSIVE tree(id, parent) AS (
  SELECT id, parent FROM nodes WHERE id = 1
  UNION ALL
  SELECT n.id, n.parent FROM nodes n JOIN tree t ON n.parent = t.id
)
SEARCH DEPTH FIRST BY id SET ordercol
CYCLE id SET is_cycle USING path
```

**What is implemented:** `WITH RECURSIVE` with `UNION ALL` — base case + recursive step, with a 1 000-iteration safety limit. Column aliases on the CTE signature work. `UNION ALL` termination works correctly.

**Why it is not done:** The `SEARCH` and `CYCLE` clauses require:
1. Carrying an ordering column across iterations (`SEARCH DEPTH FIRST`)
2. Detecting duplicate rows by primary key and stopping (`CYCLE`)
3. Materialising the traversal path as an array column (`USING path`)

These are additive to the current recursive CTE executor (`exec_recursive_cte` in `crates/sql/src/executor.rs`) and do not require new infrastructure. They were deferred because they are rarely needed in practice and have no blocking dependencies.

**Effort:** Medium (2–3 days).

---

### `LATERAL` joins

**What is missing:**
```sql
SELECT o.id, top_item.name
FROM orders o,
     LATERAL (SELECT name FROM items WHERE order_id = o.id ORDER BY price DESC LIMIT 1) top_item;
```

**What is implemented:** Correlated subqueries in `WHERE` (`EXISTS`, `IN (subquery)`, scalar subquery). These are evaluated row-by-row via `exec_plan_correlated`.

**Why it is not done:** `LATERAL` extends the correlated-subquery mechanism to the `FROM` clause. The planner must mark a `TableScan` or subquery as `LATERAL`, and the executor must pass the current outer row into the lateral input for every iteration. The join executor (`exec_join`) would need a new `JoinType::Lateral` path.

The correlated evaluation infrastructure already exists; the main work is in the planner's `plan_table_factor` (detecting `LATERAL` in the AST) and wiring the outer row through `exec_plan_with_ctes` / `exec_plan_correlated`. It was deferred because it is rarely needed and the `exec_plan_correlated` path is already complex.

**Effort:** Medium (3–5 days).

---

### Column-level `GRANT` / `REVOKE`

**What is missing:**
```sql
GRANT SELECT (name, email) ON users TO analyst;
REVOKE INSERT (salary) ON employees FROM intern;
```

**What is implemented:** Table-level ACLs (`SELECT`, `INSERT`, `UPDATE`, `DELETE`, `ALL` on a whole table). Stored as JSON in `data_dir/acls/<schema>_<table>.acl`.

**Why it is not done:** Column-level privileges require the ACL model to carry per-column grant sets. The executor's privilege check (in `engine.rs::check_privileges`) would need to inspect each projected column and verify the caller has the required column-level right. The catalog schema (`TableAcl`) needs a `col_grants: HashMap<String, Vec<AclPrivilege>>` field. This is straightforward but low-value for most use cases; most applications control column visibility through views rather than column grants.

**Effort:** Small (1 day).

---

### `VACUUM ANALYZE` — statistics collection

**What is missing:** When `VACUUM ANALYZE` is run, icedb performs the vacuum pass but does not update `pg_statistic`. The cost-based optimizer therefore cannot use real selectivity estimates; it only uses the index registry.

**What is implemented:** `VACUUM` (marks dead tuple slots, updates `pd_prune_xid`). `ANALYZE` is accepted without error but is a no-op for statistics. The optimizer rewrite rule (Filter → IndexScan for equality predicates on indexed columns) is in place and functional.

**Why it is not done:** Populating `pg_statistic` requires scanning all rows for each column, computing the null fraction, number of distinct values, and most-common-value histograms, then persisting them. The storage structures (`pg_statistic` virtual table, histogram encoding) exist partially in the codebase but the scan-and-persist pass was not wired into `exec_vacuum`. This is a high-value gap because the optimizer will make poor join-order decisions on tables without statistics.

**Effort:** Small-medium (2–3 days). Highest-priority item in this section.

---

### Table partitioning

**What is missing:** `CREATE TABLE orders PARTITION BY RANGE (created_at)` and partition pruning during query planning.

**Why it is not done:** Partitioning requires the catalog to track a parent-table / partition-table relationship, the planner to enumerate partitions and prune them based on the query predicate, and all DML paths to route writes to the correct partition. This is a significant planner and catalog overhaul. It is not a prerequisite for any other planned feature.

**Effort:** Large (3–6 weeks).

---

### Tablespaces

**What is missing:** `CREATE TABLESPACE`, `ALTER TABLE … SET TABLESPACE`, directory-per-tablespace storage.

**Why it is not done:** Currently all heap and index files live under a single `data_dir`. Adding tablespace support requires the catalog to record a `reltablespace` OID per relation and the storage layer to resolve file paths through that mapping. It is purely an operational feature with no query-semantics impact. Deferred indefinitely as a low-priority item.

**Effort:** Medium (1 week).

---

## Infrastructure gaps

These features cannot be added with planner or executor changes alone — they require new subsystems.

### PostgreSQL wire protocol (network layer)

**What is missing:** Accepting TCP connections from `psql`, DBeaver, pgAdmin, JDBC, `psycopg2`, and any other PostgreSQL client. Currently the engine runs only in embedded mode (in-process via the Rust API or the `nkv-psql` CLI which calls the engine directly).

**What is needed:**
1. **`pgwire` crate integration** — the `crates/network` stub exists but is not wired. `pgwire` handles framing, message parsing, and the `BackendMessage`/`FrontendMessage` codec.
2. **Startup handshake** — SSL negotiation (optional), `StartupMessage`, `AuthenticationRequest`, `ParameterStatus`, `BackendKeyData`, `ReadyForQuery`.
3. **SCRAM-SHA-256** — multi-step challenge/response using the `sha2` + `hmac` crates. The `crates/auth` stub is present.
4. **Simple Query protocol** — receive `Q` message, plan+execute, send `RowDescription` + zero or more `DataRow` messages + `CommandComplete` + `ReadyForQuery`.
5. **Extended Query protocol** — `Parse` → `Bind` → `Execute` → `Sync` with server-side prepared statement cache (already implemented in `QueryEngine::prepared_statements`).
6. **Error protocol** — `ErrorResponse` with SQLSTATE codes (the `SqlError::sqlstate()` method already returns correct codes).
7. **Type OID mapping** — every `Value` variant must map to a PostgreSQL type OID in the `RowDescription` message.

**Why it is not done:** The wire protocol is a self-contained milestone (Phase 7 in the roadmap). The SQL engine, catalog, and transaction manager are complete and stable; wiring the network layer is the next major step toward a standalone server. It was not prioritised during the current development phase, which focused on SQL correctness and completeness.

**Effort:** Large (2–3 weeks). This is the **highest-priority gap** for making icedb usable as a drop-in server.

**Impact:** Without this, the following are also impossible:
- `psql` interactive sessions against a running server
- DBeaver / pgAdmin / TablePlus GUI tools
- JDBC and ODBC drivers
- `pgbench` TPC-B benchmarks
- Streaming replication (which rides on the wire protocol's replication slot messages)

---

### SSL / TLS

**What is missing:** Encrypted connections between clients and the server.

**What is needed:** TLS termination on the listener socket, `SSLRequest` message handling in the startup sequence, and a certificate/key configuration option. Typically handled by wrapping the TCP stream with `tokio-rustls` or `tokio-native-tls` before handing it to `pgwire`.

**Blocked by:** Wire protocol.

**Effort:** Small (1–2 days) once the wire protocol is in place.

---

### Physical and logical replication

**What is missing:** Streaming WAL to a standby (`pg_basebackup`, `wal_receiver`), and logical replication slots (`CREATE PUBLICATION`, `CREATE SUBSCRIPTION`).

**What is needed:**
- **Physical replication:** A WAL sender process that streams WAL segments over the replication protocol (a PostgreSQL extension of the wire protocol). The standby replays WAL using the existing `wal::recovery` path. Requires WAL segments to be network-accessible and the `primary_conninfo` / `recovery.conf` equivalent.
- **Logical replication:** Decoding WAL records back to row-level changes (INSERT/UPDATE/DELETE), publishing them as a logical replication stream. Requires a WAL logical decoding layer that the current WAL writer does not produce.

**Blocked by:** Wire protocol (physical replication rides on the replication protocol extension).

**Effort:** Very large (4–8 weeks per type). Logical replication is significantly harder than physical.

---

### Connection limit enforcement and graceful shutdown

**What is missing:** `max_connections` setting, per-role connection limits, graceful `SIGTERM` shutdown (drain in-flight transactions, then exit).

**What is needed:** A connection counter in the server accept loop, checked on each new connection, and a shutdown signal handler that sets a flag to stop accepting new connections and waits for active transactions to drain.

**Blocked by:** Wire protocol (there are no TCP connections yet).

**Effort:** Small (1 day) once the wire protocol is in place.

---

### OOM-safe buffer pool

**What is missing:** The buffer pool currently uses a `Vec<Frame>` with a fixed compile-time size. Under heavy load with many concurrent transactions, the in-memory page cache can grow unbounded if the clock-sweep eviction is not aggressive enough.

**What is needed:** A hard cap on the total number of frames (configurable via `--shared-buffers`), with the eviction loop blocking new page fetches when all frames are pinned. This prevents out-of-memory kills under sustained write load.

**Effort:** Small (1–2 days). Does not require the wire protocol.

---

### `pg_dump` / `pg_restore` parity

**What is implemented:** `\dump path` and `\restore path` in the CLI generate a SQL script (CREATE TABLE + INSERT per row) and replay it. This covers schema and data.

**What is NOT covered:**
- Indexes (must be recreated manually with `CREATE INDEX`)
- Sequences (SERIAL counters reset to 1 after restore)
- Role definitions and ACL grants
- Arbitrary schema names (only `public` is dumped)
- Binary format / parallel restore (only plain SQL is produced)

**Why:** The dump format was designed for simplicity. A full `pg_dump`-compatible tool would need to introspect the catalog more deeply and generate the correct SQL ordering (dependencies between objects).

**Effort:** Medium (1 week) to reach parity with `pg_dump --schema-only` and `--data-only` modes.

---

## Intentionally out of scope

The following are deliberately not planned for the current release series. They represent either fundamental architectural changes or features that conflict with icedb's design goals.

### Multi-database support

PostgreSQL supports multiple databases within one server (each with its own catalog). icedb currently has a single database rooted at `data_dir`. Supporting multiple databases would require the catalog to be multi-tenanted and the wire protocol startup to include a database-selection handshake. Deferred to a future major version.

### Parallel query execution

PostgreSQL's parallel query uses worker processes that partition a SeqScan across CPU cores. icedb's executor is single-threaded and synchronous per query. Adding parallelism requires the executor to produce a parallel-safe plan tree and coordinate worker threads safely with the buffer pool. Deferred — single-threaded correctness is the priority.

### JIT compilation

PostgreSQL can JIT-compile tuple expressions to native code via LLVM. icedb's expression evaluator is a tree-walking interpreter. JIT is a performance optimisation, not a correctness requirement. Not planned.

### Triggers

`BEFORE`/`AFTER` row and statement triggers require a trigger registry in the catalog, a dispatch mechanism in the executor's DML paths, and PL/pgSQL support for trigger function bodies. Deferred until PL/pgSQL is implemented.

### Foreign Data Wrappers (FDW)

`CREATE FOREIGN TABLE`, `CREATE SERVER`, and `IMPORT FOREIGN SCHEMA` allow querying external data sources through a pluggable interface. Not planned for the current version.

---

## Priority order for contributors

If you want to contribute, here is the recommended implementation order based on user impact:

| Priority | Feature | Estimated effort | Notes |
|---|---|---|---|
| 1 | Wire protocol (pgwire integration) | 2–3 weeks | Unlocks psql, DBeaver, JDBC, pgbench |
| 2 | SSL/TLS | 1–2 days | Depends on wire protocol |
| 3 | `VACUUM ANALYZE` statistics | 2–3 days | Improves optimizer immediately |
| 4 | Connection limits + graceful shutdown | 1 day | Depends on wire protocol |
| 5 | OOM-safe buffer pool | 1–2 days | Independent |
| 6 | `LATERAL` joins | 3–5 days | Independent |
| 7 | `WITH RECURSIVE SEARCH/CYCLE` | 2–3 days | Independent |
| 8 | PL/pgSQL (minimal subset) | 2–4 weeks | Independent |
| 9 | Column-level `GRANT`/`REVOKE` | 1 day | Independent |
| 10 | pg_dump parity | 1 week | Independent |
| 11 | Physical replication | 4–6 weeks | Depends on wire protocol |
| 12 | Partitioning | 3–6 weeks | Independent |

---

## Changelog

| Version | Change |
|---|---|
| Current | Chapter added. Wire protocol, PL/pgSQL, LATERAL, SEARCH/CYCLE, column-level ACLs, replication, partitioning, tablespaces documented as not yet implemented. |
| — | `GENERATE_SERIES`, `STDDEV`/`VARIANCE`, `ALTER TABLE ADD COLUMN`, `INSERT INTO … SELECT`, `UPDATE … FROM`, `DELETE … USING`, window frames, `LEAD`/`LAG`/`FIRST_VALUE`/`LAST_VALUE`, `CAST`, `BETWEEN`, `IN (list)`, `CREATE SCHEMA`, `ON DELETE SET NULL/DEFAULT`, named indexes, `current_user`/`version()` system functions, `SHOW`/`SET`, `LISTEN`/`NOTIFY`, `COPY FROM`/`COPY TO`, `PREPARE`/`EXECUTE`, `information_schema`, `pg_catalog` views, autovacuum, cost-based optimizer, `pg_dump`/`\restore` all moved to implemented. |
