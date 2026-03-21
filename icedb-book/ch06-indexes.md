# Chapter 6: Indexes & Query Performance

## Why Indexes Matter

Without an index, every query that filters rows must read every page of the table — a full sequential scan. For a table with one million rows spread across 125,000 pages (at roughly 8 rows per 8 kB page), finding a single row requires reading 125,000 pages. With a B+ tree index on the filter column, finding that same row requires reading only the index path from root to leaf — typically 2–4 pages regardless of table size.

For low-selectivity filters (queries that match most of the table) a sequential scan can still be faster because it reads pages sequentially and skips index overhead. The query planner in icedb uses index scan when an equality predicate matches an indexed column; otherwise it falls back to sequential scan.

## The B+ Tree Index

icedb uses a persistent B+ tree as its only index type. Every index node — both internal nodes and leaf nodes — occupies exactly one 8 kB page on disk. This keeps the index in sync with the storage page model: every read or write to the index goes through exactly the same page I/O infrastructure as heap reads.

**Page layout of a B+ tree node:**

```
 0        8       10      12      14      16      18      20      24
 ├────────┼───────┼───────┼───────┼───────┼───────┼───────┼───────┤
 │ pd_lsn │cs_sum │ flags │pd_low │pd_up  │pd_sp  │version│prune  │
 │ 8 bytes│ 2     │ 2     │ 2     │ 2     │ 2     │ 2     │ 4     │
 └────────┴───────┴───────┴───────┴───────┴───────┴───────┴───────┘
 │              node entries (keys + TIDs or child page numbers)   │
 └─────────────────────────────────────────────────────────────────┘
 │  pd_special: left_sibling (u32) + right_sibling (u32) — 8 bytes│
 └─────────────────────────────────────────────────────────────────┘
```

**Internal nodes** store separator keys and child page numbers. Traversal descends from the root by comparing the search key against the separator keys and following the appropriate child pointer.

**Leaf nodes** store the actual indexed keys alongside **TIDs** (Tuple IDs: a pair of page number and slot number). A TID points to the exact location of the corresponding row in the heap file. After finding a key in the index, the executor fetches the heap page and slot directly.

**Sibling pointers** (`left_sibling` and `right_sibling` stored in `pd_special`) link all leaf nodes in key order, forming a doubly-linked list. Range scans walk this chain without returning to the root for each step.

**Page 0 of every index file** is the metapage. It records:
- `root_page`: page number of the current root node
- `height`: depth of the tree (number of levels)
- `num_entries`: total count of indexed entries
- `next_page`: next page number to allocate when splitting

## Creating an Index

```sql
CREATE INDEX ON table_name (column_name);
```

This scans every existing row in the table, encodes the index key (the column value serialized as bytes), and inserts each (key, TID) pair into the B+ tree. The index file appears in the data directory immediately.

```sql
CREATE TABLE products (
    id    INT NOT NULL,
    name  TEXT NOT NULL,
    price FLOAT,
    sku   TEXT
);

-- Insert some data
INSERT INTO products VALUES (1, 'Widget', 9.99, 'WGT-001');
INSERT INTO products VALUES (2, 'Gadget', 19.99, 'GDG-002');
INSERT INTO products VALUES (3, 'Doohickey', 4.99, 'DOO-003');

-- Create an index on price
CREATE INDEX ON products (price);

-- Create an index on SKU for exact lookups
CREATE INDEX ON products (sku);
```

Index files are named `idx_<table_oid>_<column_name>.btree`. For a table with OID 16384, `CREATE INDEX ON products (sku)` creates `idx_16384_sku.btree`.

### Multi-Column (Composite) Indexes

You can create an index on more than one column by listing the columns in order:

```sql
-- Multi-column index (composite index)
CREATE INDEX idx_books_author_price ON books(author_id, price);
```

The index is registered under the full column specification. Note: the query planner currently only uses indexes for single-column equality lookups on the leading column of the index. It does not yet perform index-only scans or range-based index selection for composite indexes. A composite index on `(author_id, price)` will be used for an equality predicate on `author_id` alone, but not for predicates that involve only `price` or that use range operators (`<`, `>`, `BETWEEN`) on either column.

## How the Query Planner Chooses an Index

The planner checks for an index when it encounters an equality predicate (`=`) on a column. If an index exists for that column, the planner generates an `IndexScan` plan node; otherwise it generates a `TableScan` (sequential scan).

```sql
-- Uses the index on sku:
SELECT * FROM products WHERE sku = 'WGT-001';

-- Uses the index on price (equality):
SELECT * FROM products WHERE price = 9.99;

-- Sequential scan — no index, or non-equality predicate:
SELECT * FROM products WHERE price < 15.00;
SELECT * FROM products WHERE name LIKE 'Widget%';
```

The planner does not currently perform cost estimation for index vs. sequential scan. Index selection is based solely on whether an index exists for the predicate column and whether the predicate is an equality.

## Range Scans via the Sibling Chain

Even though the planner does not route range queries through the index automatically, the B+ tree API supports efficient range scans via the `range_scan(start_key, end_key)` method. This:

1. Traverses from root to the leaf page that would contain `start_key`.
2. Scans forward through leaf nodes via `right_sibling` pointers, collecting all entries with `key <= end_key`.
3. Stops as soon as a key exceeds `end_key`.

The cost of a range scan is proportional to the number of matching index entries plus one root-to-leaf traversal. For large tables with small result sets, this is dramatically cheaper than a full sequential scan.

Future planner enhancements will route range predicates through index range scans automatically.

## When Not to Use an Index

Indexes are not always faster. Avoid creating an index when:

- **The table is small** (fewer than a few hundred rows). The overhead of opening the index file, traversing the tree, and then fetching heap pages by TID often exceeds the cost of simply scanning the small table sequentially.

- **The column has low selectivity**. A column like `country` on a global users table might have only 50 distinct values. A query `WHERE country = 'US'` might match 30% of the table. The index would find 300,000 TIDs, and fetching each corresponding heap page individually could cause more I/O than a sequential scan (which reads pages in order and benefits from read-ahead).

- **The workload is write-heavy**. Every INSERT, UPDATE, and DELETE must also update all indexes on the table. A table with 10 indexes has 10× the write amplification per row. For append-only or bulk-load workloads, creating indexes after the load is complete is more efficient.

- **The column is updated frequently**. Each UPDATE to an indexed column deletes the old index entry and inserts a new one. High-velocity updates to an indexed column amplify WAL volume.

## Viewing Indexes

Use `\d tablename` in the CLI to see a table's columns. The current `\d` output shows column definitions only (name, type, nullable) — it does not yet list indexes alongside columns. Index registration is tracked in the internal index registry. A `\d` output that lists indexes alongside columns is planned for a future CLI update.

## Index Maintenance: What Happens on INSERT, UPDATE, and DELETE

When a row is inserted into a table with one or more indexes, the executor writes the row to the heap and then calls `BTree::insert` for each index, passing the encoded column value as the key and the new row's TID as the value.

When a row is deleted, the executor calls `BTree::delete` for each index, removing the entry whose key matches the deleted row's column value.

When a row is updated on an indexed column, the old index entry is deleted and a new entry is inserted with the new key value. If the updated column is not part of any index, the index is not touched.

All index modifications are WAL-logged: `write_node` writes a `PageImage` WAL record for each modified B+ tree page before flushing it to disk. This means index state is as durable as heap state and recovers correctly from crashes.

## Worked Example: Query Performance With and Without an Index

Create a table with a large number of rows and measure query behavior:

```sql
CREATE TABLE events (
    id        INT NOT NULL,
    user_id   INT NOT NULL,
    event_type TEXT NOT NULL,
    score     FLOAT
);
```

Insert 10,000 rows using repeated inserts (in a real benchmark you would script this):

```sql
INSERT INTO events VALUES (1, 42, 'click', 1.0);
INSERT INTO events VALUES (2, 17, 'view', 0.5);
-- ... 9,998 more rows
```

Query without an index — full sequential scan of all pages:

```sql
\timing
SELECT * FROM events WHERE user_id = 42;
-- Time: 35.2 ms (example; actual depends on data size and hardware)
```

Create an index on `user_id`:

```sql
CREATE INDEX ON events (user_id);
```

Run the same query with the index available:

```sql
SELECT * FROM events WHERE user_id = 42;
-- Time: 0.8 ms (example; index traversal + targeted heap fetches)
```

The index traversal is O(log N) for the tree height (typically 2–4 pages for 10,000 entries) plus one heap fetch per matching row. The sequential scan is O(N/rows_per_page) page reads.

The difference grows as the table grows. At 1,000,000 rows, the sequential scan reads approximately 125,000 pages. An index lookup for a single user ID reads roughly 3 index pages plus the number of matching heap pages — orders of magnitude less I/O.

## Index File Internals

The B+ tree splits when a leaf node fills up. Leaf pages hold approximately 60–100 entries depending on key size (an 8 kB page can hold many (key, TID) pairs for short INT keys). When a leaf overflows:

1. The leaf is split into two leaves. The median key is promoted to the parent internal node.
2. Sibling pointers are updated: the new right leaf points back to the left leaf, and the old right sibling (if any) updates its `left_sibling` pointer.
3. All modified pages are WAL-logged as `PageImage` records before being written to disk.
4. The metapage is updated with the new `next_page` value.

If the parent internal node is also full, the split propagates upward. If the root splits, a new root internal node is created and the tree height increases by one. All of this is transparent to callers of the public API.

Merging of underfull nodes after DELETE is implemented: when a leaf becomes empty after deletion, it is logically removed. The current implementation does a simple key removal from the leaf array without full merge-and-redistribute when the node is merely half-full. Full merge logic will be added in a future release.
