# Chapter 10: Architecture Deep Dive

This chapter traces the complete path from a TCP connection through every layer of icedb to disk, explaining every design decision and data structure along the way. It is aimed at contributors and at users who want to understand why icedb behaves the way it does.

**In this chapter:**
- The full layer stack (wire protocol → auth → SQL engine → txn → storage → WAL → disk)
- Wire protocol startup sequence and query handling
- SQL planner: LogicalPlan variants
- Transaction manager: snapshots, MVCC visibility, two-phase locking, SSI status
- System catalog and in-memory caches
- B+ tree index internals
- WAL record format, segment rotation, checkpointing
- Storage engine: page layout, tuple header byte layout, buffer pool
- MVCC timeline diagram and WAL write-ahead sequence diagram
- Recovery procedure

## The Stack, Top to Bottom

```
┌─────────────────────────────────────────────────────┐
│  Client: psql / JDBC / libpq / application driver   │
└─────────────────────────┬───────────────────────────┘
                          │ TCP   PostgreSQL Wire Protocol v3.0
┌─────────────────────────▼───────────────────────────┐
│  network/   (crate: network)                        │
│  pgwire crate; Simple + Extended Query protocol     │
│  IceDbHandler, IceDbStartupHandler                  │
└────────┬────────────────────────────────────────────┘
         │                              ▲
         ├──► auth/  ◄──────────────────┘
         │   SCRAM-SHA-256 / cleartext password check
         │   RBAC privilege enforcement via catalog
         ▼
┌─────────────────────────────────────────────────────┐
│  sql/   (crate: sql)                                │
│  Parser: sqlparser-rs (PostgreSQL dialect)          │
│  Planner: AST → LogicalPlan                         │
│  Executor: Volcano/iterator model                   │
└─────────────────────────┬───────────────────────────┘
                          │
         ┌────────────────┼────────────────┐
         ▼                ▼                ▼
┌────────────────┐ ┌────────────┐ ┌─────────────────┐
│ txn/           │ │ catalog/   │ │ btree/          │
│ XID allocator  │ │ pg_class   │ │ 8 kB page nodes │
│ Snapshot       │ │ pg_attr    │ │ latch crabbing  │
│ MVCC visibility│ │ pg_authid  │ │ sibling chain   │
│ Lock manager   │ │ pg_ns      │ │ WAL-logged SMOs │
│ SSI tracking   │ │ OID alloc  │ └────────┬────────┘
└────────┬───────┘ └────────────┘          │
         │                                 │
         ▼                                 ▼
┌─────────────────────────────────────────────────────┐
│  storage/   (crate: storage)                        │
│  8 kB slotted pages; PageHeader; ItemId array       │
│  TupleHeader (t_xmin, t_xmax, t_cid, t_ctid)       │
│  HeapFile (sequential pages on disk)                │
│  BufferPool (fixed frames, Clock eviction)          │
└─────────────────────────┬───────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────┐
│  wal/   (crate: wal)                                │
│  WalWriter: append-only, segment-based (16 MiB)    │
│  WalRecord: LSN, type, page_no, data               │
│  fsync on COMMIT; checkpoint.ctl                   │
│  RecoveryManager: redo-only from last checkpoint   │
└─────────────────────────┬───────────────────────────┘
                          │
                          ▼
                     Disk (8 kB pages)
          *.heap  *.btree  *.wal  checkpoint.ctl
```

---

## Layer 1: Wire Protocol (network/)

The `network` crate wraps the [`pgwire`](https://crates.io/crates/pgwire) library, which handles low-level PostgreSQL message framing, startup negotiation, and the Simple/Extended Query protocols.

**Startup sequence:**

1. Client connects via TCP.
2. Client sends a `Startup` message containing the username and requested database.
3. `IceDbStartupHandler::on_startup` runs. It requests a cleartext password by sending `Authentication::CleartextPassword`.
4. Client sends the password.
5. `IceDbStartupHandler` calls `authenticator.authenticate(username, password)` (see Layer 2).
6. On success, `pgwire::api::auth::finish_authentication` sends `AuthenticationOK`, parameter status messages (server version: `"16.0 (icedb)"` as of this writing), and `ReadyForQuery`.

**Simple Query protocol (`Q` message):**

1. Client sends a `Q` message with a SQL string.
2. `IceDbHandler::do_query` calls `engine.execute(sql)`.
3. The result is streamed back as `DataRow` messages followed by `CommandComplete`.

**Extended Query protocol (`Parse` / `Bind` / `Execute` / `Sync`):**

1. `Parse`: client sends a SQL string with `$1`, `$2`, ... placeholders. `IceDbQueryParser` stores the SQL string as-is.
2. `Bind`: client sends parameter values. `substitute_params` replaces `$1`, `$2`, ... with quoted literal values.
3. `Execute`: `IceDbHandler::do_query` on the portal calls `engine.execute(substituted_sql)`.
4. `Sync`: triggers `ReadyForQuery`.

Parameter substitution is done by text replacement in the current implementation. Proper parameterized query support (avoiding string substitution) is a planned improvement.

**Handler factory:**

`IceDbHandlerFactory` implements `PgWireHandlerFactory`, providing separate handler instances for startup, simple queries, extended queries, and the no-op COPY handler. Each incoming TCP connection spawns a Tokio task calling `pgwire::tokio::process_socket`.

---

## Layer 2: Authentication (auth/)

Authentication has two parts: password verification and RBAC enforcement.

**Password verification** (`Authenticator::authenticate`):

1. Look up the role by username in `pg_authid`.
2. Check `rolcanlogin` — reject if false.
3. Read `rolpassword` (the stored SCRAM-SHA-256 verifier).
4. Call `scram::verify_password(stored_verifier, provided_password)`.
5. `verify_password` parses the verifier, re-derives the StoredKey using PBKDF2-SHA-256 with the stored salt and iteration count, and compares byte-by-byte.

**RBAC enforcement** (`QueryEngine::check_privileges`):

1. After the planner produces a `LogicalPlan`, `check_privileges` inspects the plan type.
2. Superuser (`rolsuper = true`) bypasses all checks.
3. `CREATE TABLE`, `DROP TABLE`, `CREATE INDEX` require `rolcreatedb`.
4. `CREATE ROLE` requires `rolcreaterole`.
5. `SELECT`, `INSERT`, `UPDATE`, `DELETE` require `rolcanlogin`.

---

## Layer 3: SQL Engine (sql/)

The SQL engine has three components: parser, planner, and executor.

### Parser

`sqlparser-rs` with the PostgreSQL dialect parses SQL strings into an AST. The `Parser::parse` wrapper in the `sql` crate takes a `&str` and returns `Vec<Statement>`. Multi-statement inputs are supported (each statement is a separate element).

### Planner

`Planner::plan_statement` converts a `Statement` AST node into a `LogicalPlan`. The `LogicalPlan` enum represents every SQL construct. The following shows the actual variants from `crates/sql/src/plan.rs`:

```rust
enum LogicalPlan {
    // Reads
    TableScan { table_name, alias, schema, filter },
    IndexScan { table_name, schema, index_column, eq_value, range_start, range_end, filter },
    // Relational operators
    Filter { input, predicate },
    Project { input, columns, distinct },
    Join { left, right, join_type, condition, using_columns },
    Aggregate { input, group_by, aggregates, having },
    Sort { input, keys },
    Limit { input, limit, offset },
    SetOp { op, all, left, right },   // UNION / INTERSECT / EXCEPT
    Cte { ctes, inner },              // WITH ... AS (...)
    Values { schema, rows },          // materialized subquery or CTE result
    // DML
    Insert { table_name, schema, columns, source, returning },
    Update { table_name, schema, assignments, filter, returning },
    Delete { table_name, schema, filter, returning },
    // DDL
    CreateTable { schema_name, table_name, columns, if_not_exists },
    DropTable { schema_name, table_name, if_exists },
    CreateIndex { schema_name, table_name, column_name },
    CreateRole { rolname, rolsuper, rolcanlogin, password },
}
```

Note: table names are resolved to schema + table at the planner level, but the plan variants themselves carry `table_name` (not `schema_name`) for most read/write nodes; DDL nodes carry both.

The planner:
1. Resolves table and column names against the catalog.
2. Passes the initial plan through the `Optimizer` (see below).
3. Converts `sqlparser-rs` expression types to the internal `Expr` enum.
4. Constructs the plan tree bottom-up (inner relations before outer).

### Cost-Based Optimizer

The `Optimizer` struct (`crates/sql/src/optimizer.rs`) post-processes the logical plan produced by the planner. It currently implements one rewrite rule:

**Filter → IndexScan pushdown:** When the plan contains a `Filter` node wrapping a `TableScan`, and the filter predicate is an equality check (`column = value`) on a column that has a B+ tree index, the optimizer rewrites the subtree to an `IndexScan`. This eliminates the full sequential scan and the separate filter step in a single pass.

```
Before optimization:
  Filter(predicate: customer_id = 42)
    TableScan(orders)

After optimization:
  IndexScan(orders, index_column: customer_id, eq_value: 42)
```

The `index_registry` in the catalog manager is consulted to check whether an index exists for the (table, column) pair. If no index exists, the `Filter(TableScan)` structure is preserved unchanged.

**Planned optimizer rules (not yet implemented):**
- Join reordering based on estimated cardinality
- Selectivity estimation from `pg_statistic` histograms (MCV and range buckets)
- Predicate pushdown through joins
- Aggregate early termination on indexed GROUP BY columns

### Executor

`Executor::execute` pattern-matches on the `LogicalPlan` and runs the appropriate method. The overall model is **Volcano/iterator**: each plan node conceptually produces a stream of rows. The executor materializes results into `Vec<Row>` before returning, making the model "pull all at once" rather than true streaming (true streaming to the wire protocol is a future improvement).

**Table scan execution:**

1. Open the heap file for the table OID from the catalog.
2. Call `txn_manager.scan_visible_tuples(xid, &mut heap)`.
3. For each visible tuple, decode the row's column values using the table schema.
4. Apply the filter predicate (if any).
5. Apply the projection (column selection).

**Index scan execution:**

1. Open the B+ tree for the indexed column.
2. Call `btree.search(encoded_key)` to get the TID.
3. Fetch that specific tuple from the heap.
4. Apply the filter predicate.

**JOIN execution:**

The current executor implements a **nested-loop join**: for each row in the left relation, scan the entire right relation and emit pairs that satisfy the join condition. This is O(N×M) in the worst case. Hash join and merge join are planned.

**AGGREGATE execution:**

1. Execute the input plan and collect all rows.
2. Group rows by the GROUP BY expressions.
3. For each group, compute aggregate functions (SUM, COUNT, AVG, MIN, MAX).
4. Return one output row per group.

**DML execution (INSERT, UPDATE, DELETE):**

All DML goes through the transaction manager, which enforces the WAL write-ahead rule on every tuple write.

---

## Layer 4: Transaction Manager (txn/)

The `TransactionManager` is the concurrency control heart of icedb. It maintains:

- **active**: `HashMap<Xid, Transaction>` — all currently active transactions
- **committed**: `HashSet<Xid>` — all committed XIDs
- **aborted**: `HashSet<Xid>` — all aborted XIDs
- **lock_manager**: `LockManager` — two-phase locking for write-write conflicts
- **wal_writer**: `Arc<WalWriter>` — for logging COMMIT and ABORT records

**Transaction lifecycle:**

```
begin()  → allocate XID via global atomic; insert into active map
             for RepRd/Ser: take snapshot at BEGIN time
             returns Xid

execute  → scan_visible_tuples() or insert/delete/update_tuple()
             visibility checks use current snapshot

commit() → check_serializable_conflict() (SSI)
             append_and_flush(Commit WAL record)
             move XID from active → committed
             release all locks

abort()  → append_and_flush(Abort WAL record)
             move XID from active → aborted
             release all locks
```

**Snapshot:**

A snapshot is a `Snapshot { xmin, xmax, in_progress: HashSet<Xid> }`:

- `xmin`: minimum XID of any concurrent active transaction (exclusive — transactions with XID < xmin are either committed or aborted)
- `xmax`: next XID to be allocated (transactions with XID >= xmax don't exist yet)
- `in_progress`: XIDs of transactions that were active when the snapshot was taken

**Visibility rule** (`is_tuple_visible`):

```
is_visible(header, snapshot, committed_set) → bool

A tuple is visible if:
  (xmin == current_txn_xid) OR (xmin committed AND xmin < snapshot.xmax AND xmin NOT in_progress)
  AND
  (xmax == 0) OR (xmax is active or aborted or future)
```

In plain English: the creating transaction committed before this snapshot was taken, and the deleting transaction (if any) was not yet committed when this snapshot was taken.

**Two-phase locking:**

Write-write conflicts are prevented by `LockManager::acquire_write_lock(xid, tid)`. Before modifying a tuple, the transaction manager tries to acquire an exclusive write lock on the TID. If another active transaction holds the lock, the current transaction blocks (or returns a conflict error). Locks are released on commit or abort.

**SSI:**

For Serializable transactions, `read_set` and `write_set` record every (page_no, slot) that was read or written. On commit, `check_serializable_conflict` logs the set sizes for diagnostics and then returns `Ok(())` — it is currently a stub. Full rw-antidependency cycle detection (which would return `Err(TxnError::SerializationFailure)` and surface as SQLSTATE 40001 to the client) is in development.

---

## Layer 5: System Catalog (catalog/)

The catalog stores schema metadata in four heap files:

| Heap file | Contents |
|-----------|----------|
| `catalog_pg_class.heap` | One row per table or index: OID, name, namespace, kind, column count |
| `catalog_pg_attribute.heap` | One row per column: table OID, column name, type OID, attnum, not_null |
| `catalog_pg_authid.heap` | One row per role: OID, name, privilege flags, password verifier |
| `catalog_pg_namespace.heap` | One row per schema: OID, name, owner OID |

These are ordinary icedb heap files using the same slotted page format and tuple headers as user tables. MVCC applies to catalog writes — DDL statements are transactional.

**In-memory caches:**

```
schema_cache: RwLock<HashMap<u32 (OID), TableSchema>>
name_cache:   RwLock<HashMap<(ns_oid, table_name), OID>>
ns_cache:     RwLock<HashMap<schema_name, ns_oid>>
index_registry: RwLock<HashMap<(table_oid, col_name), PathBuf>>
```

Cache writes are protected by `RwLock`. Cache misses fall back to disk scans. On startup, `load_from_disk` populates the caches from the heap files.

**Virtual catalog views (`information_schema` and `pg_catalog`):**

The `information_schema` and `pg_catalog` views are implemented as virtual table scans — they have no heap files on disk. Instead, when the executor encounters a scan of a virtual catalog table (e.g., `information_schema.tables` or `pg_catalog.pg_class`), it calls the appropriate method on `CatalogManager` to generate rows from the in-memory caches at query time. No storage I/O occurs. This means catalog queries are always consistent with the current in-memory state and are very fast, but the rows are not visible to MVCC or VACUUM.

**Bootstrap:**

On first startup (when `catalog_pg_class.heap` does not exist), `bootstrap()` runs:
1. Inserts the `public` and `pg_catalog` namespace rows.
2. Registers the four system tables themselves in `pg_class`.
3. Inserts column definitions for each system table in `pg_attribute`.
4. Creates the `icedb` superuser role in `pg_authid` (no password required).
5. Commits the bootstrap transaction.

---

## Layer 6: B+ Tree Index (btree/)

The B+ tree is stored as a file of 8 kB pages. Page 0 is the metapage. Page 1 is the initial root (a leaf node). As the tree grows, new pages are allocated sequentially.

**Node types:**

```
Internal node:
  node_type: Internal
  internal_entries: Vec<InternalEntry { key: Vec<u8>, child_page: u32 }>
  left_sibling, right_sibling: u32

Leaf node:
  node_type: Leaf
  leaf_entries: Vec<LeafEntry { key: Vec<u8>, tid: TID }>
  left_sibling, right_sibling: u32
```

**Search (O(log N)):**

Root → compare key with internal node separator keys → descend to child → repeat until leaf → binary search in leaf for exact key.

**Insert with split:**

1. Find leaf via root-to-leaf traversal.
2. Insert into leaf.
3. If leaf is full: split into two halves. Median key promoted to parent.
4. If parent is full: split parent. Promote again. Continue up the path.
5. If root splits: create a new root with two children. Tree height increases.

All modified pages are WAL-logged (`PageImage` records) before being written to disk. The metapage is updated last.

**Range scan:**

1. Traverse from root to the leftmost leaf containing `start_key`.
2. Walk forward through the sibling chain (`right_sibling` pointers).
3. Collect entries with `key <= end_key`.
4. Stop when a key exceeds `end_key` or the sibling pointer is 0 (end of leaves).

---

## Layer 7: Write-Ahead Log (wal/)

**WAL records** are encoded as:

```
total_len:  u32  (little-endian; length of entire record including this field)
lsn:        u64  (monotonically increasing; assigned by WalWriter)
prev_lsn:   u64  (LSN of previous record; 0 for first)
xid:        u32  (transaction ID)
record_type: u8  (Commit=1, Abort=2, Insert=3, Delete=4, Update=5, PageImage=6, Checkpoint=7)
page_no:    u32  (heap page number, or index page with encoding)
data_len:   u32  (length of payload)
data:       [u8]  (payload bytes; for PageImage, the full 8 kB page)
```

**WalWriter** is thread-safe via `Mutex<Inner>`. All writes go through `append_inner`, which:
1. Rotates to a new segment file if the current file exceeds 16 MiB.
2. Assigns the next LSN (monotonically from `current_lsn + 1`).
3. Encodes the record and writes it to the current file.
4. Updates `current_lsn` and `prev_lsn`.

`append()` buffers in the OS page cache. `append_and_flush()` calls `fsync` after writing. Commit and Abort records always use `append_and_flush` — durability requires the record to be on disk before the commit returns.

**Segment rotation:**

When `stream_position() >= segment_size` (default 16 MiB), the writer opens a new file named `{segment_number:016}.wal`. Segment numbers start at 1 and increment. On startup, the writer scans all existing segments to find the highest LSN so new records continue the sequence.

**Checkpointing:**

A checkpoint flushes all dirty buffer pool pages to disk and then writes a `Checkpoint` WAL record. The checkpoint LSN is written to `checkpoint.ctl` (8 bytes, little-endian `u64`). Recovery starts from this LSN, so WAL segments before the checkpoint LSN can be archived or deleted.

---

## Layer 8: Storage Engine (storage/)

**Page layout:**

```
Byte offset  Size  Field        Description
──────────────────────────────────────────────────────────────
0            8     pd_lsn       LSN of last WAL record touching this page
8            2     pd_checksum  FNV-1a checksum (computed over page with this field zeroed)
10           2     pd_flags     PD_HAS_CHECKSUM (0x0001) and other flags
12           2     pd_lower     End of item pointer array (first free byte after last ItemId)
14           2     pd_upper     Start of tuple data (last free byte before first tuple)
16           2     pd_special   Start of special space (for B+ tree: sibling pointers)
18           2     pd_version   Page format version (currently 1)
20           4     pd_prune_xid Oldest XID whose dead tuples could be pruned (for VACUUM)
24           …     ItemId array Grows downward from pd_lower; each entry is 4 bytes
…            …     Free space   Between pd_lower and pd_upper
…            …     Tuple data   Grows upward toward pd_upper; newest tuples nearest pd_upper
```

ASCII art of a page with two tuples inserted:

```
 Offset 0                                        Offset 8192
 ┌────────┬──┬──┬──┬──┬──┬──┬──┬──────────────────────────────┐
 │ Header │  Item ID 0  │  Item ID 1  │    Free Space    │T1│T0│
 │ 24 B   │  lp0        │  lp1        │                  │  │  │
 └────────┴─────────────┴─────────────┴──────────────────┴──┴──┘
          ↑pd_lower                                  pd_upper↑
```

`pd_lower` starts at 24 (right after the header). Each inserted tuple advances `pd_lower` by 4 bytes (one ItemId) and decreases `pd_upper` by `len(tuple_bytes)`. Free space = `pd_upper - pd_lower`.

**Tuple header:**

```
Byte offset  Size  Field          Description
0            4     t_xmin         XID of creating transaction (u32)
4            4     t_xmax         XID of deleting transaction (u32; 0 = live)
8            4     t_cid          Command ID within the creating transaction (u32)
12           4     t_ctid_page    Page number of newer version of this row (u32)
16           2     t_ctid_slot    Slot number of newer version of this row (u16)
18           2     t_infomask     Visibility/status flags (u16; see HEAP_* constants)
20           1     t_hoff         Offset to start of user data (always 24)
21           1     t_bits         Null bitmap (1 byte; supports up to 8 columns)
── total: 22 bytes header; 2 bytes padding to reach t_hoff=24 ──
```

User data starts at `t_hoff` (byte offset 24, after 2 bytes of alignment padding appended after the 22-byte header). Column values are serialized sequentially starting at offset 24.

Note: `t_xmin` and `t_xmax` are 32-bit (`u32`) transaction IDs, not 64-bit. XIDs are allocated from an atomic counter and fit in u32 for the current implementation.

**FNV-1a checksum:**

`compute_checksum()` computes a 32-bit FNV-1a hash over all 8192 bytes of the page, treating the two checksum bytes (at offset 8–9) as zero. The result is XOR-folded into 16 bits: `(hash ^ (hash >> 16)) as u16`. This detects single-bit and multi-bit corruption with high probability.

**HeapFile:**

A `HeapFile` is a plain file of 8 kB pages, named `<oid>.heap`. Methods:
- `read_page(page_no) -> Page`: read 8 kB at offset `page_no * 8192`
- `write_page(page_no, page)`: write 8 kB at that offset
- `allocate_page() -> u32`: extend the file by one empty page
- `num_pages() -> u32`: file size / 8192

**BufferPool:**

The `BufferPool` struct (with clock-sweep/Second-Chance eviction, pin/unpin, dirty marking, and flush) is fully implemented and tested in `crates/storage/src/buffer.rs`. However, the executor's hot path for user tables currently calls `HeapFile` read/write methods directly rather than routing through the buffer pool. Wiring the buffer pool into the executor's table scan and DML path is the next storage integration milestone.

---

## MVCC Visibility: A Timeline Diagram

```
Time →

Txn A (XID=100)    BEGIN ──INSERT (id=1, balance=1000)──────────── COMMIT
                                  │
                         t_xmin=100, t_xmax=0

Txn B (XID=101)              BEGIN ──READ id=1 ─────────────────── COMMIT
                                  │
                         Snapshot: xmin=100, xmax=102, in_progress={}
                         XID 100 < 102 and committed → row VISIBLE
                         Reads: balance = 1000

Txn C (XID=102)                        BEGIN ─UPDATE id=1 set balance=800─ COMMIT
                                               │           │
                                  old: t_xmax=102    new: t_xmin=102, t_xmax=0

Txn D (XID=103)                                      BEGIN ── READ id=1 ── COMMIT
                                                            │
                                                   Snapshot: xmin=102, xmax=104
                                                   XID 102 < 104 and committed
                                                   Old version: t_xmax=102 committed → DELETED
                                                   New version: t_xmin=102 committed → VISIBLE
                                                   Reads: balance = 800
```

---

## WAL Write-Ahead Rule: Sequence Diagram

```
Application thread                WAL                    Heap file

insert_tuple(xid, heap, data)
   │
   ├── find_and_prepare_insert()     ←── page modified in MEMORY (not disk yet)
   │
   ├── wal.append(PageImage, page_bytes)   ──write to WAL file──►  WAL segment
   │
   └── heap.write_page(page_no, page)  ──write to heap file──►  .heap file

on COMMIT:
   ├── wal.append_and_flush(Commit, ...)   ──write + fsync──►  WAL segment (on disk)
   │
   └── returns Ok(())   ← DATA IS NOW DURABLE
```

The critical invariant: the WAL record always reaches disk **before** the data page. Even if the data page write is lost in a crash, the WAL record exists and can be replayed to reconstruct the page.

---

## Recovery Procedure

```
Startup
   │
   ├── Read checkpoint.ctl → get checkpoint LSN (or INVALID_LSN if no checkpoint)
   │
   ├── WalReader::open(log_dir, checkpoint_lsn)
   │      ↓
   │   Scans .wal segment files in order
   │   Seeks to checkpoint_lsn position
   │
   ├── For each WAL record:
   │      PageImage → overwrite heap page at page_no with record.data (8 kB)
   │      Commit / Abort / Checkpoint → skip (no physical redo)
   │
   ├── TransactionManager::new_with_wal_recovery
   │      Re-scans all WAL records for Commit/Abort records
   │      Rebuilds committed/aborted XID sets
   │      Sets next_xid to max_seen_xid + 1
   │
   └── Database is ready; open for connections
```

The recovery is **redo-only**. There is no undo phase. Uncommitted transactions are invisible because their XIDs are not in the committed set — MVCC visibility rules exclude them without any physical undo of their writes.

This is correct because every data page modification is logged as a complete `PageImage` record (the full 8 kB page after the modification). Replaying the record to the heap file exactly reconstructs the state at the time of the write, including the MVCC header fields (`t_xmin`, `t_xmax`). Visibility rules then filter out uncommitted tuples correctly.

---

## Testing Architecture

icedb has 2520 automated tests spread across three independent test workspaces. The most important structural property is that every integration test runs in **three modes**.

### Three-mode test execution

```
┌──────────────────────────────────────────────────────────────────┐
│  test_foo_body(b: &Backend)                                      │
│  ┌───────────────────────────────────────────────────────────┐   │
│  │  exec(b, "CREATE TABLE t (id INT)")                       │   │
│  │  exec(b, "INSERT INTO t VALUES (1)")                      │   │
│  │  assert_eq!(count_rows(b, "SELECT * FROM t"), 1)          │   │
│  └─────────────────────┬─────────────────────────────────────┘   │
└────────────────────────┼─────────────────────────────────────────┘
                         │ crate::net_tests!(test_foo)
              ┌──────────┼──────────┐
              ▼          ▼          ▼
        test_foo    test_foo_net  test_foo_net_tls
        Embedded    Plain TCP     TLS (sslmode=require)
        (direct     (icedb-server (icedb-server
        Rust call)   subprocess)   subprocess + TLS)
```

The `Backend` enum abstracts over the two transport layers:

```rust
enum Backend {
    Embedded(Arc<QueryEngine>),   // direct Rust method call
    Network(Mutex<PgClient>),     // PostgreSQL wire protocol v3.0
}
```

When a test uses `Backend::Network`, SQL is serialized into a `Q` (Simple Query) message, sent over TCP (optionally TLS), and the response is parsed from `RowDescription` + `DataRow` + `CommandComplete` messages. Column types are reconstructed from the PostgreSQL OIDs in `RowDescription`, and errors are mapped from SQLSTATE codes to the same `SqlError` variants the embedded engine produces.

### Test workspaces

| Workspace | Command | Tests | Modes |
|-----------|---------|-------|-------|
| Root crates | `cargo test --workspace` | 313 unit | embedded only |
| `tests/` | `cargo test --manifest-path tests/Cargo.toml` | 2204 integration | embedded + plain TCP + TLS |
| `sandbox/ch03/` | `cargo test --manifest-path sandbox/ch03/Cargo.toml` | 3 ch03 | embedded + plain TCP + TLS |

### Network test infrastructure

Two long-lived `icedb-server` subprocesses are started once per test-binary invocation via `OnceLock<NetServer>` — one plain TCP, one TLS. Each network test connects to its own isolated database (named after the test function and provisioned with `DROP DATABASE IF EXISTS` + `CREATE DATABASE` before the test body runs). The `#[serial]` attribute from the `serial_test` crate sequences network tests within each server's slot while allowing full parallelism between embedded tests and between the two server instances.

Server processes redirect `stdout`/`stderr` to `Stdio::null()` so they do not inherit the test binary's output pipe, which would otherwise prevent `cargo test | tail -N` style invocations from terminating.

### What the three modes cover

| Concern | Embedded | Plain TCP | TLS |
|---------|----------|-----------|-----|
| SQL correctness | ✓ | ✓ | ✓ |
| Wire protocol framing | — | ✓ | ✓ |
| Type OID encoding/decoding | — | ✓ | ✓ |
| SQLSTATE error propagation | — | ✓ | ✓ |
| TLS handshake and encryption | — | — | ✓ |
| Concurrent session isolation | ✓ | ✓ | ✓ |
| Catalog introspection APIs | ✓ | — | — |
| Dump/restore internals | ✓ | — | — |

Tests that use internal Rust APIs not accessible over the wire (dump/restore, catalog listener, vacuum tracking) run embedded-only via `if b.is_network() { return; }` guards.

Full details are in [`tests/TEST-ARCHITECTURE.md`](../tests/TEST-ARCHITECTURE.md).
