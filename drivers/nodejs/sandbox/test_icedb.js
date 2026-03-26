"use strict";
/**
 * Sandbox tests for the icedb Node.js driver.
 *
 * Tests:
 *   1. Basic CRUD (INSERT / SELECT / UPDATE / DELETE)
 *   2. Explicit transaction: begin / commit
 *   3. Explicit transaction: begin / rollback (data must not persist)
 *   4. Null values and type coercion
 *   5. Aggregate queries
 */

const icedb = require("icedb");
const os = require("os");
const fs = require("fs");
const path = require("path");

let failures = 0;
const tests = [];

function test(name, fn) {
  tests.push({ name, fn });
}

function assertEqual(a, b, msg = "") {
  if (a !== b) {
    throw new Error(`Expected ${JSON.stringify(b)}, got ${JSON.stringify(a)}. ${msg}`);
  }
}

function makeTempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "icedb-test-"));
}

function removeDir(dir) {
  fs.rmSync(dir, { recursive: true, force: true });
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
    assertEqual(rows[1].values[0], "2");
    assertEqual(rows[1].values[1], "world");

    conn.execute("UPDATE t SET val = 'updated' WHERE id = 1");
    const updated = conn.query("SELECT val FROM t WHERE id = 1");
    assertEqual(updated[0].values[0], "updated");

    conn.execute("DELETE FROM t WHERE id = 2");
    const count = conn.query("SELECT COUNT(*) AS n FROM t");
    assertEqual(count[0].values[0], "1");
  } finally {
    removeDir(dir);
  }
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
  } finally {
    removeDir(dir);
  }
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
  } finally {
    removeDir(dir);
  }
});

test("null_values", () => {
  const dir = makeTempDir();
  try {
    const conn = icedb.connect(dir);
    conn.execute("CREATE TABLE misc (id INT, note TEXT)");
    conn.execute("INSERT INTO misc VALUES (1, NULL)");

    const rows = conn.query("SELECT id, note FROM misc");
    assertEqual(rows.length, 1);
    assertEqual(rows[0].values[0], "1");
    assertEqual(rows[0].values[1], null, "NULL column must map to null");
  } finally {
    removeDir(dir);
  }
});

test("aggregate_sum", () => {
  const dir = makeTempDir();
  try {
    const conn = icedb.connect(dir);
    conn.execute("CREATE TABLE nums (v INT)");
    conn.execute("INSERT INTO nums VALUES (10)");
    conn.execute("INSERT INTO nums VALUES (20)");
    conn.execute("INSERT INTO nums VALUES (30)");

    const rows = conn.query("SELECT SUM(v) AS total FROM nums");
    assertEqual(rows[0].values[0], "60");
  } finally {
    removeDir(dir);
  }
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
