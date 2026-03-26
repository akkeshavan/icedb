"""
Sandbox tests for the icedb Python driver.

Tests:
  1. Basic CRUD (INSERT / SELECT / UPDATE / DELETE)
  2. Explicit transaction: begin / commit
  3. Explicit transaction: begin / rollback (data must not persist)
  4. Context manager: commit on success
  5. Context manager: rollback on exception
  6. Multiple connections to the same data directory
"""

import os
import sys
import tempfile
import icedb


def assert_eq(a, b, msg=""):
    if a != b:
        raise AssertionError(f"Expected {b!r}, got {a!r}. {msg}")


def test_basic_crud():
    with tempfile.TemporaryDirectory() as d:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE t (id INT PRIMARY KEY, val TEXT)")

        conn.execute_dml("INSERT INTO t VALUES (1, 'hello')")
        conn.execute_dml("INSERT INTO t VALUES (2, 'world')")

        rows = conn.execute("SELECT id, val FROM t ORDER BY id")
        assert_eq(len(rows), 2)
        assert_eq(rows[0]["id"], 1)
        assert_eq(rows[0]["val"], "hello")
        assert_eq(rows[1]["id"], 2)
        assert_eq(rows[1]["val"], "world")

        conn.execute_dml("UPDATE t SET val = 'updated' WHERE id = 1")
        rows = conn.execute("SELECT val FROM t WHERE id = 1")
        assert_eq(rows[0]["val"], "updated")

        conn.execute_dml("DELETE FROM t WHERE id = 2")
        rows = conn.execute("SELECT COUNT(*) AS n FROM t")
        assert_eq(rows[0]["n"], 1)

    print("  [PASS] test_basic_crud")


def test_explicit_commit():
    with tempfile.TemporaryDirectory() as d:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE acct (id INT PRIMARY KEY, balance INT)")
        conn.execute_dml("INSERT INTO acct VALUES (1, 1000)")

        conn.begin()
        conn.execute_dml("UPDATE acct SET balance = balance - 100 WHERE id = 1")
        conn.commit()

        rows = conn.execute("SELECT balance FROM acct WHERE id = 1")
        assert_eq(rows[0]["balance"], 900)

    print("  [PASS] test_explicit_commit")


def test_explicit_rollback():
    with tempfile.TemporaryDirectory() as d:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE acct (id INT PRIMARY KEY, balance INT)")
        conn.execute_dml("INSERT INTO acct VALUES (1, 1000)")

        conn.begin()
        conn.execute_dml("UPDATE acct SET balance = 0 WHERE id = 1")
        conn.rollback()

        rows = conn.execute("SELECT balance FROM acct WHERE id = 1")
        assert_eq(rows[0]["balance"], 1000, "balance must be unchanged after rollback")

    print("  [PASS] test_explicit_rollback")


def test_context_manager_commit():
    with tempfile.TemporaryDirectory() as d:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE events (id INT PRIMARY KEY, name TEXT)")

        with conn:
            conn.execute_dml("INSERT INTO events VALUES (1, 'signup')")
            conn.execute_dml("INSERT INTO events VALUES (2, 'login')")

        rows = conn.execute("SELECT COUNT(*) AS n FROM events")
        assert_eq(rows[0]["n"], 2)

    print("  [PASS] test_context_manager_commit")


def test_context_manager_rollback_on_exception():
    with tempfile.TemporaryDirectory() as d:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE events (id INT PRIMARY KEY, name TEXT)")

        try:
            with conn:
                conn.execute_dml("INSERT INTO events VALUES (1, 'signup')")
                raise RuntimeError("simulated error")
        except RuntimeError:
            pass

        rows = conn.execute("SELECT COUNT(*) AS n FROM events")
        assert_eq(rows[0]["n"], 0, "rows must be rolled back after exception in context manager")

    print("  [PASS] test_context_manager_rollback_on_exception")


def test_null_and_types():
    with tempfile.TemporaryDirectory() as d:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE misc (id INT, flag BOOL, score FLOAT8, note TEXT)")
        conn.execute_dml("INSERT INTO misc VALUES (1, TRUE, 3.14, NULL)")

        rows = conn.execute("SELECT id, flag, score, note FROM misc")
        assert_eq(len(rows), 1)
        assert_eq(rows[0]["id"], 1)
        assert_eq(rows[0]["flag"], True)
        assert abs(rows[0]["score"] - 3.14) < 1e-9
        assert rows[0]["note"] is None

    print("  [PASS] test_null_and_types")


if __name__ == "__main__":
    tests = [
        test_basic_crud,
        test_explicit_commit,
        test_explicit_rollback,
        test_context_manager_commit,
        test_context_manager_rollback_on_exception,
        test_null_and_types,
    ]
    failures = 0
    for t in tests:
        try:
            t()
        except Exception as e:
            print(f"  [FAIL] {t.__name__}: {e}")
            failures += 1

    if failures:
        print(f"\n{failures}/{len(tests)} tests FAILED")
        sys.exit(1)
    else:
        print(f"\n{len(tests)}/{len(tests)} tests passed")
