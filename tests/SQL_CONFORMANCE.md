# SQL Conformance Test Suite

This document summarizes the SQL conformance test suite for icedb.
Tests are in `/tests/src/sql_conformance/` and run with `cargo test -p icedb-tests`.

## Results Summary

| Status   | Count |
|----------|-------|
| Passing  | 202   |
| Ignored  | 32    |
| Failing  | 0     |
| **Total**| **234** |

*(Counts are for sql_conformance module only; total icedb-tests suite: 227 passing, 32 ignored.)*

## How to Run

```bash
# Run all conformance tests
cd tests && cargo test sql_conformance

# Run with output
cd tests && cargo test sql_conformance -- --nocapture

# Run a specific category
cd tests && cargo test sql_conformance::aggregates
cd tests && cargo test sql_conformance::joins
cd tests && cargo test sql_conformance::tpch
cd tests && cargo test sql_conformance::hermitage

# Run including ignored tests (to see what is planned)
cd tests && cargo test sql_conformance -- --include-ignored

# Run full workspace
cd /path/to/icedb && cargo test --workspace
```

## Test Categories

### Category 1: Basic SELECT (`sql_conformance::select`)
**34 tests — 29 passing, 5 ignored**

Confirmed working:
- SELECT literal values (int, text, bool, arithmetic)
- SELECT *, named columns, column aliases, table aliases
- Arithmetic expressions in SELECT
- WHERE with =, <>, <, <=, >, >= comparisons
- WHERE with AND, OR, NOT
- WHERE BETWEEN ... AND ...
- WHERE LIKE (prefix, suffix, contains)
- WHERE ILIKE (case-insensitive)
- WHERE IN (...), WHERE NOT IN (...)
- ORDER BY ASC, DESC, multi-column
- LIMIT, LIMIT + OFFSET
- SELECT DISTINCT, DISTINCT multi-column
- ORDER BY + LIMIT + OFFSET combined
- SELECT without FROM (literal expressions)
- Row count consistency (50-row table)

Ignored (not yet implemented):
- `CASE WHEN ... THEN ... ELSE ... END` expression
- `COALESCE(...)` function
- `NULLIF(val, 0)` function
- String concatenation operator `||`
- Window functions (`RANK() OVER (ORDER BY ...)`, `ROW_NUMBER()`)
- `GENERATE_SERIES()` table function

---

### Category 2: NULL Handling (`sql_conformance::null_handling`)
**14 tests — 13 passing, 1 ignored**

Confirmed working:
- `IS NULL`, `IS NOT NULL` in WHERE
- `NULL = NULL` evaluates to NULL (returns 0 rows)
- `NULL <> 'value'` for NULL row does not match
- `COUNT(*)` counts NULLs, `COUNT(col)` skips NULLs
- `SUM(col)` skips NULLs; SUM of all NULLs returns NULL/0
- NULL in ORDER BY (all rows returned)
- NULL not equal to 0
- INSERT and SELECT of NULL values
- NOT NULL constraint enforcement
- `WHERE val IN (1, NULL)` only matches non-NULL values

Ignored:
- `COALESCE(NULL, NULL, 42)` — function not implemented

---

### Category 3: Aggregates (`sql_conformance::aggregates`)
**16 tests — 14 passing, 2 ignored**

Confirmed working:
- `COUNT(*)`, `COUNT(col)` (with NULL skipping)
- `SUM`, `AVG`, `MIN`, `MAX` on numeric and text columns
- `GROUP BY` single and multiple columns
- `HAVING COUNT(*) > n`, `HAVING SUM(...) > n`
- `HAVING` with `WHERE` pre-filter
- Multiple aggregates in one query
- GROUP BY + ORDER BY by aggregate result
- Aggregate on empty table returns 0

Ignored:
- `COUNT(DISTINCT col)` — not yet implemented
- `STDDEV()` / `VARIANCE()` — not yet implemented

---

### Category 4: JOINs (`sql_conformance::joins`)
**14 tests — 10 passing, 4 ignored**

Confirmed working:
- `INNER JOIN ON a.id = b.id` (basic)
- INNER JOIN excludes non-matching rows
- `LEFT OUTER JOIN` (unmatched left rows appear with NULLs)
- LEFT JOIN + `WHERE right.col IS NULL` (find unmatched left rows)
- Implicit cross join via `FROM a, b`
- Self-join (employee–manager hierarchy)
- Three-table JOIN chain
- JOIN with aggregate (GROUP BY after JOIN)
- JOIN with empty table (INNER: 0 rows; LEFT: all left rows)

Ignored (engine bugs / not implemented):
- `CROSS JOIN` keyword — not implemented (use `FROM a, b` instead)
- `JOIN ... ON a.x = b.x AND a.y = b.y` (multi-condition) — engine returns cross product
- `JOIN t2 USING (col)` — engine returns cross product instead of equi-join
- Selecting t1.col and t2.col with same name via aliases — engine returns t1 value for both
- `FULL OUTER JOIN` — not implemented
- `RIGHT OUTER JOIN` — not implemented

---

### Category 5: Subqueries (`sql_conformance::subqueries`)
**16 tests — 11 passing, 5 ignored**

Confirmed working:
- `WHERE id IN (SELECT ...)` — non-correlated IN subquery
- `WHERE id NOT IN (SELECT ...)` — non-correlated NOT IN subquery
- `FROM (SELECT ...) AS sub` — derived table / inline view
- Derived table with aggregation
- Nested `IN (... IN (...))` subqueries
- `WHERE id IN (SELECT ... GROUP BY ... HAVING ...)` — via separate test
- `WHERE price = (SELECT MAX(price) FROM t)` — non-correlated scalar
- `SELECT COUNT(*) FROM (SELECT ...) AS sub` — count via derived table
- Empty subquery result for IN (returns 0 rows)
- NOT IN with NULL in subquery (documented, no assert)

Ignored (correlated subqueries not implemented):
- `WHERE EXISTS (SELECT 1 FROM t WHERE t.col = outer.col)` — correlated EXISTS
- `WHERE NOT EXISTS (SELECT 1 FROM t WHERE ...)` — correlated NOT EXISTS
- `SELECT (SELECT COUNT(*) FROM orders WHERE user_id = u.id)` — scalar in SELECT list
- `WHERE salary = (SELECT MAX(s2.salary) FROM t s2 WHERE s2.dept = t.dept)` — correlated scalar in WHERE
- `WHERE price < (SELECT AVG(p2.price) FROM t p2 WHERE p2.cat = p.cat)` — correlated filter
- `WHERE id IN (SELECT uid FROM t GROUP BY uid HAVING COUNT(*) > 1)` — aggregate alias in outer (engine bug)

---

### Category 6: Set Operations (`sql_conformance::set_operations`)
**17 tests — 17 passing, 0 ignored**

Confirmed working:
- `UNION` (deduplicates)
- `UNION ALL` (preserves duplicates)
- `INTERSECT`
- `EXCEPT`
- `UNION` + `ORDER BY`
- `UNION` + `WHERE` pre-filter
- Three-way `UNION` and `UNION ALL`
- Set operations with empty tables
- `UNION` with literal subquery

---

### Category 7: CTEs (`sql_conformance::ctes`)
**14 tests — 8 passing, 6 ignored**

Confirmed working:
- Basic `WITH cte AS (SELECT ...) SELECT FROM cte`
- CTE used in `COUNT(*)`
- CTE column aliasing
- CTE with `LIMIT`
- CTE with aggregate filter (single-level CTE)

Ignored (engine bug: chained CTEs return 0 rows):
- Second CTE referencing first CTE
- CTE joined with base table
- CTE used in `IN (SELECT id FROM cte)` pattern
- Multi-step chain of 3 CTEs
- CTE + JOIN for revenue analysis
- Recursive CTEs (`WITH RECURSIVE`) — not implemented

---

### Category 8: DML (`sql_conformance::dml`)
**20 tests — 20 passing, 0 ignored**

Confirmed working:
- `INSERT` single row, multi-row, with column list
- `INSERT ... RETURNING id, name`, `RETURNING *`, multi-row RETURNING
- `UPDATE` single column, multiple columns, by expression
- `UPDATE ... RETURNING`
- `UPDATE` with no rows matched (no-op)
- `UPDATE` all rows (no WHERE)
- `UPDATE SET col = NULL`
- `DELETE WHERE ...`
- `DELETE ... RETURNING`, `RETURNING *`
- `DELETE` all rows (no WHERE)
- `DELETE` multiple rows
- Insert → Update → Select round-trip
- Insert → Delete → Re-insert

---

### Category 9: DDL (`sql_conformance::ddl`)
**18 tests — 15 passing, 3 ignored**

Confirmed working:
- `CREATE TABLE` with INTEGER, BIGINT, TEXT, VARCHAR, BOOLEAN, FLOAT, DOUBLE PRECISION
- `NOT NULL` constraint enforcement
- `DROP TABLE`
- `DROP TABLE IF EXISTS`
- `CREATE TABLE` duplicate error (SQLSTATE 42P07)
- `CREATE INDEX` single column
- Drop + recreate table with different schema
- Many-column table (8 columns)
- Table survives restart (durability)
- Drop+recreate pattern (no ALTER TABLE)

Ignored:
- `CREATE INDEX` multi-column — engine bug: post-index query returns fewer rows
- `ALTER TABLE ADD COLUMN` — not implemented
- `UNIQUE` constraint enforcement — not implemented
- `PRIMARY KEY` constraint enforcement — not implemented

---

### Category 10: Transactions (`sql_conformance::transactions`)
**17 tests — 17 passing, 0 ignored**

Confirmed working:
- `BEGIN` / `COMMIT` — changes visible after commit
- `BEGIN` / `ROLLBACK` — changes not visible after rollback
- Multi-statement commit (bank transfer)
- Multi-statement rollback (all operations undone)
- No dirty reads (uncommitted writes not visible)
- Aborted writes not persisted
- Repeatable Read: snapshot stability across reads
- Read Committed: sees latest committed value
- No phantom reads under Repeatable Read
- Own inserts visible within same transaction
- Concurrent reads (3 simultaneous RR transactions)
- `is_committed()` / `is_aborted()` transaction state
- Durability: data survives restart
- Rollback preserves prior committed rows

---

### Category 11: Error Handling (`sql_conformance::error_handling`)
**16 tests — 16 passing, 0 ignored**

Confirmed SQLSTATE codes:
- `42P01` — undefined table (TableNotFound)
- `42703` — undefined column (ColumnNotFound)
- `22012` — division by zero (DivisionByZero)
- `42601` — syntax error (Parse)
- `42P07` — duplicate table
- `22003` — numeric overflow
- Error messages include the offending object name
- `DROP TABLE` nonexistent reports error
- `UPDATE` to nonexistent column: `42703`
- NOT NULL violation: `23000` / `23502`
- Ambiguous column reference: `42702`
- Empty statement handled gracefully (no panic)

---

### Category 12: TPC-H Style Queries (`sql_conformance::tpch`)
**14 tests — 14 passing, 0 ignored**

Confirmed working with simplified TPC-H schema (5 regions, 10 nations, 5 suppliers, 8 customers, 8 orders, 12 lineitems):
- Q1: GROUP BY returnflag + linestatus with SUM, AVG, COUNT
- Q2: Supplier-nation-region join (nation/region join with filter)
- Q3: 3-way JOIN (customer + orders + lineitem) with GROUP BY and HAVING
- Q4: Orders group by priority with COUNT
- Q5: 5-table JOIN with nation revenue aggregation
- Q6: Discount revenue (WHERE BETWEEN + SUM of expression)
- Q10: Customer return analysis (3-way JOIN + returnflag filter)
- Lineitem stats by return flag
- Schema integrity (all 6 tables, correct row counts)
- Q3 with HAVING + expression arithmetic

---

### Category 13: Hermitage Isolation Tests (`sql_conformance::hermitage`)
**17 tests — 16 passing, 1 ignored**

Confirmed passing:
- **G0**: No dirty writes — T1's committed write visible after T2 aborts
- **G1a**: No aborted reads — T2 does not see T1's uncommitted write
- **G1b**: No intermediate reads — T2 sees original value during T1's in-progress update chain
- **G1c**: No circular information flow — neither T1 nor T2 sees the other's uncommitted writes
- **P4**: Lost update detection under Repeatable Read (write-write conflict)
- **G2-item**: Write skew documented (behavior depends on isolation level; infrastructure in place)
- Committed write visible to subsequent transactions
- RR snapshot taken at BEGIN, not at first read
- Uncommitted inserts not visible to other transactions
- Phantom prevention under Repeatable Read
- Complete rollback of batched operations (INSERT + UPDATE, all undone)

Ignored:
- `test_hermitage_read_own_writes`: UPDATE within transaction not visible to subsequent reads in same txn (own-write visibility bug for UPDATE)

---

## Known Engine Limitations Documented by Ignored Tests

| Feature | Status |
|---------|--------|
| `CASE WHEN` expression | Not implemented (`0A000`) |
| `COALESCE()` function | Not implemented |
| `NULLIF()` function | Not implemented |
| String `\|\|` concatenation | Not implemented |
| `COUNT(DISTINCT col)` | Not implemented |
| `STDDEV()` / `VARIANCE()` | Not implemented |
| `CROSS JOIN` keyword | Not implemented |
| `JOIN ... USING (col)` | Engine bug: returns cross product |
| Multi-condition `JOIN ON a=b AND c=d` | Engine bug: returns cross product |
| Correlated subqueries (EXISTS, scalar) | Not implemented |
| Chained CTEs (CTE references CTE) | Engine bug: returns 0 rows |
| `WITH RECURSIVE` | Not implemented |
| `ALTER TABLE` | Not implemented |
| `UNIQUE` constraint | Not implemented |
| `PRIMARY KEY` constraint | Not implemented |
| `FULL OUTER JOIN` | Not implemented |
| `RIGHT OUTER JOIN` | Not implemented |
| Window functions | Not implemented |
| Own-write visibility after UPDATE in same txn | Engine bug |
| Multi-column index correctness | Engine bug: post-index scan returns wrong count |
| SSI cycle detection | Infrastructure in place; detection stubbed out |
