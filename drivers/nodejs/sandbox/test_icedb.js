"use strict";
/**
 * Sandbox tests for the icedb Node.js driver.
 *
 * Tests:
 *   1.  Basic CRUD
 *   2.  Explicit begin / commit
 *   3.  Explicit begin / rollback
 *   4.  Null values
 *   5.  Aggregate queries (SUM / COUNT / MAX / MIN)
 *   6.  execute() rows_affected count
 *   7.  Uncommitted write not visible to a second connection on the same dir
 *   8.  begin / rollback / begin again (connection remains usable)
 *   9.  SQL error does not break the connection
 *   10. PK constraint violation: connection stays usable
 *   11. Large result set (1,000 rows)
 *   12. Multiple transactions on the same connection
 *   13. Concurrent connections preserve balance invariant
 */

const icedb  = require("icedb");
const os     = require("os");
const fs     = require("fs");
const path   = require("path");
const { Worker, isMainThread, workerData, parentPort } = require("worker_threads");

let failures = 0;
const tests = [];

function test(name, fn) { tests.push({ name, fn }); }

function assertEqual(a, b, msg = "") {
    if (a !== b) throw new Error(`Expected ${JSON.stringify(b)}, got ${JSON.stringify(a)}. ${msg}`);
}
function assertTrue(cond, msg = "") {
    if (!cond) throw new Error(msg);
}
function makeTempDir() {
    return fs.mkdtempSync(path.join(os.tmpdir(), "icedb-test-"));
}
function removeDir(d) {
    fs.rmSync(d, { recursive: true, force: true });
}

// ── Tests ────────────────────────────────────────────────────────────────────

test("basic_crud", () => {
    const dir = makeTempDir();
    try {
        const conn = icedb.connect(dir);
        conn.execute("CREATE TABLE t (id INT PRIMARY KEY, val TEXT)");
        conn.execute("INSERT INTO t VALUES (1, 'hello')");
        conn.execute("INSERT INTO t VALUES (2, 'world')");

        const rows = conn.query("SELECT id, val FROM t ORDER BY id");
        assertEqual(rows.length, 2);
        assertEqual(rows[0].values[0], "1");
        assertEqual(rows[0].values[1], "hello");

        conn.execute("UPDATE t SET val = 'updated' WHERE id = 1");
        const updated = conn.query("SELECT val FROM t WHERE id = 1");
        assertEqual(updated[0].values[0], "updated");

        conn.execute("DELETE FROM t WHERE id = 2");
        const count = conn.query("SELECT COUNT(*) AS n FROM t");
        assertEqual(count[0].values[0], "1");
    } finally { removeDir(dir); }
});

test("explicit_commit", () => {
    const dir = makeTempDir();
    try {
        const conn = icedb.connect(dir);
        conn.execute("CREATE TABLE acct (id INT PRIMARY KEY, balance INT)");
        conn.execute("INSERT INTO acct VALUES (1, 1000)");

        conn.begin();
        conn.execute("UPDATE acct SET balance = balance - 100 WHERE id = 1");
        conn.commit();

        const rows = conn.query("SELECT balance FROM acct WHERE id = 1");
        assertEqual(rows[0].values[0], "900");
    } finally { removeDir(dir); }
});

test("explicit_rollback", () => {
    const dir = makeTempDir();
    try {
        const conn = icedb.connect(dir);
        conn.execute("CREATE TABLE acct (id INT PRIMARY KEY, balance INT)");
        conn.execute("INSERT INTO acct VALUES (1, 1000)");

        conn.begin();
        conn.execute("UPDATE acct SET balance = 0 WHERE id = 1");
        conn.rollback();

        const rows = conn.query("SELECT balance FROM acct WHERE id = 1");
        assertEqual(rows[0].values[0], "1000", "balance must be unchanged after rollback");
    } finally { removeDir(dir); }
});

test("null_values", () => {
    const dir = makeTempDir();
    try {
        const conn = icedb.connect(dir);
        conn.execute("CREATE TABLE misc (id INT, note TEXT)");
        conn.execute("INSERT INTO misc VALUES (1, NULL)");
        conn.execute("INSERT INTO misc VALUES (2, 'present')");

        const rows = conn.query("SELECT id, note FROM misc ORDER BY id");
        assertEqual(rows.length, 2);
        assertEqual(rows[0].values[1], null, "NULL column must map to null");
        assertEqual(rows[1].values[1], "present");
    } finally { removeDir(dir); }
});

test("aggregate_queries", () => {
    const dir = makeTempDir();
    try {
        const conn = icedb.connect(dir);
        conn.execute("CREATE TABLE nums (v INT)");
        for (const v of [10, 20, 30, 40]) conn.execute(`INSERT INTO nums VALUES (${v})`);

        assertEqual(conn.query("SELECT SUM(v) AS s FROM nums")[0].values[0], "100");
        assertEqual(conn.query("SELECT COUNT(*) AS n FROM nums")[0].values[0], "4");
        assertEqual(conn.query("SELECT MAX(v) AS m FROM nums")[0].values[0], "40");
        assertEqual(conn.query("SELECT MIN(v) AS m FROM nums")[0].values[0], "10");
    } finally { removeDir(dir); }
});

test("execute_rows_affected", () => {
    const dir = makeTempDir();
    try {
        const conn = icedb.connect(dir);
        conn.execute("CREATE TABLE t (id INT, val INT)");
        for (let i = 1; i <= 5; i++) conn.execute(`INSERT INTO t VALUES (${i}, ${i * 10})`);

        const n = conn.execute("UPDATE t SET val = 0 WHERE val > 20");
        assertEqual(n, 3, "UPDATE must return 3 rows affected");

        const n2 = conn.execute("DELETE FROM t WHERE val = 0");
        assertEqual(n2, 3, "DELETE must return 3 rows affected");
    } finally { removeDir(dir); }
});

test("uncommitted_not_visible_to_other_connection", () => {
    const dir = makeTempDir();
    try {
        const conn1 = icedb.connect(dir);
        const conn2 = icedb.connect(dir);

        conn1.execute("CREATE TABLE t (id INT, val INT)");
        conn1.execute("INSERT INTO t VALUES (1, 100)");

        conn1.begin();
        conn1.execute("UPDATE t SET val = 999 WHERE id = 1");

        const rows = conn2.query("SELECT val FROM t WHERE id = 1");
        assertEqual(rows[0].values[0], "100",
            "conn2 must not see conn1's uncommitted write");

        conn1.rollback();
    } finally { removeDir(dir); }
});

test("rollback_then_reuse_connection", () => {
    const dir = makeTempDir();
    try {
        const conn = icedb.connect(dir);
        conn.execute("CREATE TABLE t (id INT PRIMARY KEY)");

        conn.begin();
        conn.execute("INSERT INTO t VALUES (1)");
        conn.rollback();

        // Connection must be reusable after rollback
        conn.execute("INSERT INTO t VALUES (2)");
        const rows = conn.query("SELECT id FROM t");
        assertEqual(rows.length, 1, "Only the committed row must exist");
        assertEqual(rows[0].values[0], "2");
    } finally { removeDir(dir); }
});

test("sql_error_connection_still_usable", () => {
    const dir = makeTempDir();
    try {
        const conn = icedb.connect(dir);
        conn.execute("CREATE TABLE t (id INT PRIMARY KEY)");

        try {
            conn.query("THIS IS NOT SQL");
        } catch (_) { /* expected */ }

        // Connection must still be usable
        conn.execute("INSERT INTO t VALUES (1)");
        const rows = conn.query("SELECT id FROM t");
        assertEqual(rows.length, 1);
    } finally { removeDir(dir); }
});

test("pk_constraint_violation_connection_still_usable", () => {
    const dir = makeTempDir();
    try {
        const conn = icedb.connect(dir);
        conn.execute("CREATE TABLE t (id INT PRIMARY KEY, val TEXT)");
        conn.execute("INSERT INTO t VALUES (1, 'first')");

        try {
            conn.execute("INSERT INTO t VALUES (1, 'duplicate')");
        } catch (_) { /* expected */ }

        // Original row intact; connection usable
        const rows = conn.query("SELECT val FROM t WHERE id = 1");
        assertEqual(rows.length, 1);
        assertEqual(rows[0].values[0], "first");
    } finally { removeDir(dir); }
});

test("large_result_set", () => {
    const dir = makeTempDir();
    try {
        const conn = icedb.connect(dir);
        conn.execute("CREATE TABLE big (id INT, val INT)");
        for (let i = 1; i <= 1000; i++) conn.execute(`INSERT INTO big VALUES (${i}, ${i * 2})`);

        const rows = conn.query("SELECT id, val FROM big ORDER BY id");
        assertEqual(rows.length, 1000);
        assertEqual(rows[0].values[0], "1");
        assertEqual(rows[999].values[0], "1000");

        const expectedSum = (1000 * 1001 / 2) * 2;
        const actualSum = rows.reduce((s, r) => s + parseInt(r.values[1]), 0);
        assertEqual(actualSum, expectedSum, `SUM(val) must be ${expectedSum}`);
    } finally { removeDir(dir); }
});

test("multiple_transactions_same_connection", () => {
    const dir = makeTempDir();
    try {
        const conn = icedb.connect(dir);
        conn.execute("CREATE TABLE t (id INT PRIMARY KEY)");

        // First transaction: commit
        conn.begin();
        conn.execute("INSERT INTO t VALUES (1)");
        conn.commit();

        // Second transaction: rollback
        conn.begin();
        conn.execute("INSERT INTO t VALUES (2)");
        conn.rollback();

        // Third transaction: commit
        conn.begin();
        conn.execute("INSERT INTO t VALUES (3)");
        conn.commit();

        const rows = conn.query("SELECT id FROM t ORDER BY id");
        assertEqual(rows.length, 2, "Only id=1 and id=3 must be committed");
        assertEqual(rows[0].values[0], "1");
        assertEqual(rows[1].values[0], "3");
    } finally { removeDir(dir); }
});

test("concurrent_connections_balance_invariant", () => {
    // Synchronous simulation using a single thread with multiple connections
    const dir = makeTempDir();
    try {
        const setup = icedb.connect(dir);
        const N = 6;
        const initial = 1000;
        setup.execute("CREATE TABLE acct (id INT PRIMARY KEY, balance INT)");
        for (let i = 1; i <= N; i++) setup.execute(`INSERT INTO acct VALUES (${i}, ${initial})`);

        // Run 60 transfers using separate connections (simulates concurrent workers)
        for (let i = 0; i < 60; i++) {
            const from = (i % N) + 1;
            const to   = (from % N) + 1;
            const conn = icedb.connect(dir);

            for (let attempt = 0; attempt < 10; attempt++) {
                try {
                    conn.begin();
                    conn.execute(`UPDATE acct SET balance = balance - 5 WHERE id = ${from}`);
                    conn.execute(`UPDATE acct SET balance = balance + 5 WHERE id = ${to}`);
                    conn.commit();
                    break;
                } catch (_) {
                    try { conn.rollback(); } catch (_2) {}
                }
            }
        }

        const rows = setup.query("SELECT SUM(balance) AS total FROM acct");
        const total = parseInt(rows[0].values[0]);
        assertEqual(total, N * initial,
            `Balance invariant violated: expected ${N * initial}, got ${total}`);
    } finally { removeDir(dir); }
});

// ── Runner ───────────────────────────────────────────────────────────────────

for (const { name, fn } of tests) {
    try {
        fn();
        console.log(`  [PASS] ${name}`);
    } catch (e) {
        console.error(`  [FAIL] ${name}: ${e.message}`);
        failures++;
    }
}

const total = tests.length;
if (failures > 0) {
    console.error(`\n${failures}/${total} tests FAILED`);
    process.exit(1);
} else {
    console.log(`\n${total}/${total} tests passed`);
}
