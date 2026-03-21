# Chapter 5: Transactions & ACID Guarantees

ACID — Atomicity, Consistency, Isolation, Durability — is the contractual promise a relational database makes to its users. This chapter explains exactly how icedb delivers each guarantee, with concrete examples and code you can run.

## What ACID Means in icedb

### Atomicity

A transaction either completes entirely or has no visible effect at all. There is no partial commit.

icedb achieves atomicity through the WAL and the MVCC tuple lifecycle. When a transaction begins, it receives a unique transaction ID (XID, a `u32`). Every tuple the transaction inserts has `t_xmin` set to that XID. Every tuple the transaction deletes has `t_xmax` set to that XID.

If the transaction calls `ROLLBACK`, the WAL records an `Abort` record for that XID. All tuple versions with `t_xmin` equal to an aborted XID are invisible to every snapshot. All tuple versions with `t_xmax` equal to an aborted XID are treated as still live. From the perspective of every observer, the aborted transaction never happened.

If the server crashes mid-transaction (before a `Commit` WAL record), the transaction is effectively aborted. On restart, the WAL recovery scans for `Commit` and `Abort` records. Any XID without a `Commit` record is treated as aborted. Its tuple versions are invisible.

**Example: rollback leaves no trace**

```sql
BEGIN;
INSERT INTO accounts (id, balance) VALUES (99, 500.00);
-- Decide not to proceed
ROLLBACK;

-- The row was never committed; it is not visible
SELECT * FROM accounts WHERE id = 99;
-- (0 rows)
```

### Consistency

Consistency means the database moves from one valid state to another. Validity is defined by constraints.

icedb currently enforces `NOT NULL` constraints at insert time. The planner checks each inserted value against the column's `not_null` flag before the tuple is written.

```sql
CREATE TABLE accounts (id INT NOT NULL, balance FLOAT NOT NULL);

INSERT INTO accounts VALUES (1, NULL);
-- ERROR: null value in column "balance" violates not-null constraint
```

icedb also enforces arithmetic consistency: integer arithmetic that would overflow the column's range returns an error (`SQLSTATE 22012` for division by zero, `SQLSTATE 22003` for overflow) rather than silently wrapping around. This means a computation like `SELECT 2147483647 + 1` raises an error instead of producing a negative number that violates application-level invariants. Float arithmetic follows IEEE 754 (infinity and NaN are possible).

`UNIQUE` and `PRIMARY KEY` constraints are enforced at INSERT time — a duplicate value in a UNIQUE or PRIMARY KEY column raises SQLSTATE `23000`. `FOREIGN KEY` constraints are recorded in the catalog but not yet enforced (see Chapter 4 for the full list of limitations).

### Isolation

Isolation means concurrent transactions do not interfere with each other in unexpected ways. icedb uses **Multi-Version Concurrency Control (MVCC)** — the same approach used by PostgreSQL and most modern databases.

The key insight behind MVCC: instead of locking rows and making concurrent readers wait, icedb keeps multiple versions of each row. Readers see the version that was current at the time their snapshot was taken. Writers create new versions without touching the versions that readers are looking at. Readers never block writers. Writers never block readers.

Write-write conflicts (two transactions modifying the same row) are handled by two-phase locking: the second writer blocks until the first commits or rolls back.

### Durability

Durability means committed data survives crashes and power failures.

icedb implements the **WAL write-ahead rule**: before any modified data page is written to disk, the WAL record describing that modification must be fsynced. On a commit, the `Commit` WAL record is fsynced before the commit function returns to the caller.

This means: if `COMMIT` returned successfully, the data is on disk. Even if the server process is killed with `SIGKILL` one millisecond after the commit returns, the data can be recovered by replaying the WAL on next startup.

---

## MVCC Explained: How Tuple Versioning Works

Every row stored in icedb carries a header with four MVCC fields:

| Field | Type | Meaning |
|-------|------|---------|
| `t_xmin` | u32 (XID) | Transaction that created this version |
| `t_xmax` | u32 (XID) | Transaction that deleted this version (0 if live) |
| `t_cid` | u32 | Command ID within the transaction |
| `t_ctid` | (page: u32, slot: u16) | Pointer to newer version of this row (for UPDATE chains) |

A row version is visible to transaction X if and only if both of the following hold:

**Condition 1 — the row was created by a visible writer:**
- `t_xmin == X` (this transaction wrote the row — read-own-writes), OR
- `t_xmin` identifies a committed transaction that committed before X's snapshot was taken.

**Condition 2 — the row has not been deleted by a visible deleter:**
- `t_xmax == 0` (the row has not been deleted), OR
- `t_xmax == X` (this transaction deleted it — the row is NOT visible to X itself; the old version is gone from this transaction's perspective), OR rather **NOT** (`t_xmax == X`), OR
- `t_xmax` identifies a transaction that was not yet committed when X's snapshot was taken, OR
- `t_xmax` identifies an aborted transaction.

More precisely: condition 2 is satisfied when `t_xmax` is absent (0), aborted, or not yet committed at snapshot time — and the special case is that if `t_xmax == X`, the row was deleted by the current transaction and is invisible to it.

**Read-own-writes** means a transaction always sees the rows it has written or modified, even before it commits. This is essential for correct behavior within a single transaction: inserting a row and then selecting it in the same transaction returns the new row without requiring a COMMIT.

```sql
BEGIN;
INSERT INTO accounts VALUES (99, 500);
SELECT balance FROM accounts WHERE id = 99;  -- returns 500 (read-own-write)
ROLLBACK;
SELECT balance FROM accounts WHERE id = 99;  -- returns nothing (rolled back)
```

Let's trace through a complete scenario.

**Step 1: Initial state — no rows**

The `accounts` table is empty.

**Step 2: Transaction A inserts a row**

```sql
-- Transaction A: XID = 100
BEGIN;
INSERT INTO accounts (id, balance) VALUES (1, 1000.00);
COMMIT;
```

After commit, the tuple on disk looks like:

```
t_xmin = 100 (committed)
t_xmax = 0   (not deleted)
data:  id=1, balance=1000.00
```

**Step 3: Transaction B reads**

Transaction B starts after A commits. Its snapshot captures xmax (the next XID to be allocated) as 101. Since XID 100 < 101 and is committed, the row is visible. B sees `(1, 1000.00)`.

**Step 4: Transaction C updates the balance**

```sql
-- Transaction C: XID = 102
BEGIN;
UPDATE accounts SET balance = 800.00 WHERE id = 1;
```

An UPDATE is implemented as two operations:
1. Set `t_xmax = 102` on the old tuple version.
2. Insert a new tuple version with `t_xmin = 102`, `t_xmax = 0`, `balance = 800.00`.

While C is still active (not yet committed), the heap contains two versions of the row:

```
Version 1: t_xmin=100, t_xmax=102(active), balance=1000.00
Version 2: t_xmin=102(active), t_xmax=0,   balance=800.00
```

**Step 5: Concurrent readers see different values**

- A snapshot taken before C began (xmax ≤ 102) sees Version 1 as live and Version 2 as invisible (its `t_xmin` is in the active set). It reads `balance = 1000.00`.
- A snapshot taken after C commits (xmax > 102) sees Version 1 with `t_xmax = 102 (committed)` — so Version 1 is deleted. It sees Version 2 as the live version. It reads `balance = 800.00`.

This is the fundamental MVCC guarantee: readers and writers work on independent views of the data simultaneously without blocking each other.

---

## Isolation Levels

icedb supports three isolation levels. They differ in when a transaction takes its snapshot.

| Isolation Level | Dirty Read | Non-Repeatable Read | Phantom Read |
|-----------------|------------|---------------------|--------------|
| Read Committed | Prevented | Possible | Possible |
| Repeatable Read | Prevented | Prevented | Prevented |
| Serializable | Prevented | Prevented | Prevented |

### Read Committed

The default isolation level. A new snapshot is taken for each individual SQL statement. This means:

- If another transaction commits between two SELECT statements in the same transaction, the second SELECT sees the new data.
- This is the "read what's committed right now" behavior.

**Example: non-repeatable read at Read Committed**

```sql
-- Session 1
BEGIN TRANSACTION ISOLATION LEVEL READ COMMITTED;
SELECT balance FROM accounts WHERE id = 1;
-- Returns: 1000.00

-- (Session 2 now runs: UPDATE accounts SET balance = 800 WHERE id = 1; COMMIT;)

SELECT balance FROM accounts WHERE id = 1;
-- Returns: 800.00  ← different from first read; non-repeatable
COMMIT;
```

The second `SELECT` takes a fresh snapshot and sees Session 2's committed update.

### Repeatable Read

A single snapshot is taken at `BEGIN` time. All statements in the transaction use this fixed snapshot, regardless of what other transactions commit in the meantime.

```sql
-- Session 1
BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ;
SELECT balance FROM accounts WHERE id = 1;
-- Returns: 1000.00

-- (Session 2: UPDATE accounts SET balance = 800 WHERE id = 1; COMMIT;)

SELECT balance FROM accounts WHERE id = 1;
-- Returns: 1000.00  ← same as first read; repeatable!
COMMIT;
```

Phantom reads (new rows matching the WHERE clause appearing between two reads) are also prevented at Repeatable Read in icedb's MVCC implementation, because the fixed snapshot does not include rows committed after `BEGIN`.

### Serializable

Identical snapshot behavior to Repeatable Read, with additional tracking of read/write sets to detect rw-antidependency cycles. The SSI infrastructure (recording which rows each transaction read and wrote) is in place. Full cycle detection (which would cause `SERIALIZATION FAILURE` errors for conflicting concurrent transactions) is in development; the current implementation logs the read/write sets but does not yet abort transactions for detected cycles.

When SSI is fully implemented, a serialization failure will return SQLSTATE 40001. Application code using Serializable isolation should be written to retry on this error (see Chapter 12 for a retry example).

```sql
BEGIN TRANSACTION ISOLATION LEVEL SERIALIZABLE;
-- ... queries ...
COMMIT;
```

---

## Using Transactions in Practice

### The Bank Transfer Example

The classic atomicity test: transfer money between accounts. Either both debits and credits succeed, or neither takes effect.

```sql
CREATE TABLE accounts (
    id      INT NOT NULL,
    name    TEXT NOT NULL,
    balance FLOAT NOT NULL
);

INSERT INTO accounts VALUES (1, 'Alice', 1000.00);
INSERT INTO accounts VALUES (2, 'Bob',   500.00);
```

Transfer $300 from Alice to Bob:

```sql
BEGIN;
UPDATE accounts SET balance = balance - 300.00 WHERE id = 1;
UPDATE accounts SET balance = balance + 300.00 WHERE id = 2;
COMMIT;
```

After commit:

```sql
SELECT name, balance FROM accounts ORDER BY id;
```

```
 name  | balance
-------+---------
 Alice |   700.0
 Bob   |   800.0
```

The sum of balances is still 1500.00 — the consistency invariant is preserved.

**What happens step by step:**

1. `BEGIN` allocates a new XID (say, 200) and records it as active.
2. For Repeatable Read or Serializable, a snapshot is captured immediately.
3. The first `UPDATE` soft-deletes Alice's old tuple (sets `t_xmax = 200`) and inserts a new tuple with `balance = 700.00`, `t_xmin = 200`.
4. Before the page is written to disk, a `PageImage` WAL record is written containing the full modified page bytes.
5. The same WAL-first process happens for Bob's row.
6. `COMMIT` writes a `Commit` WAL record with `xid = 200` and calls `fsync`.
7. XID 200 is moved from the active set to the committed set.
8. Lock manager releases all locks held by XID 200.

If the server crashes between steps 5 and 6, neither account change is committed. On restart, WAL replay replays the `PageImage` records (both pages are written as they appeared mid-transaction), but since there is no `Commit` record for XID 200, both updates are invisible — their `t_xmin` is in the aborted set.

**What if the transfer fails mid-way?**

```sql
BEGIN;
UPDATE accounts SET balance = balance - 300.00 WHERE id = 1;
-- Suppose the application crashes here, or explicitly rolls back
ROLLBACK;
```

Alice's old tuple (balance 1000.00) had `t_xmax` set to XID 200 transiently. After `ROLLBACK`, XID 200 is in the aborted set. The old tuple's `t_xmax` points to an aborted XID, so MVCC visibility rules treat it as still live. Alice's balance reads as 1000.00. Bob is untouched.

---

## What Happens on Crash

On startup, icedb runs WAL recovery automatically. The process:

1. Read `checkpoint.ctl` from the data directory. This file contains the LSN of the last checkpoint (the last point where all dirty pages were flushed to disk).
2. Open a `WalReader` starting at that checkpoint LSN.
3. Replay all `PageImage` WAL records: for each record, overwrite the corresponding heap or index page on disk with the page image stored in the WAL record.
4. Skip `Commit`, `Abort`, and `Checkpoint` records (no physical redo needed — they update transaction state, which is rebuilt from the commit/abort records by `TransactionManager::new_with_wal_recovery`).
5. After all records are replayed, the database is in the state it would have been in had it shut down cleanly just before the crash.

The WAL reader scans forward through segment files in order (named `0000000000000001.wal`, `0000000000000002.wal`, etc.). It stops at the first truncated or corrupt record — partial writes from a crash are simply discarded.

After physical recovery, `TransactionManager::new_with_wal_recovery` scans the WAL a second time, collecting all `Commit` and `Abort` records, to rebuild the committed/aborted XID sets. This ensures visibility rules work correctly for any tuples that survived the crash.

**Recovery is fully automatic.** Users never need to run a recovery tool. If the server exits uncleanly, the next startup handles it. Log output during recovery looks like:

```
INFO  Starting WAL recovery from LSN 42
INFO  WAL recovery finished; last replayed LSN = 87
```

If recovery encounters a genuinely corrupt WAL record (not just a partial write), it logs an error and stops at that point. In practice, partial writes are the only form of WAL damage that occurs under normal crash conditions.
