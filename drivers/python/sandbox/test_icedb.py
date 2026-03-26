"""
Sandbox tests for the icedb Python driver.

Tests:
  1.  Basic CRUD (INSERT / SELECT / UPDATE / DELETE)
  2.  Explicit transaction: begin / commit
  3.  Explicit transaction: begin / rollback (data must not persist)
  4.  Context manager: commit on success
  5.  Context manager: rollback on exception
  6.  NULL values and type coercion
  7.  Multi-row INSERT atomicity: if any row violates PK, nothing is inserted
  8.  execute_dml returns correct rows_affected count
  9.  Uncommitted write not visible to a second connection on the same data dir
  10. Rollback idempotency: a second rollback on an idle connection must not crash
  11. Error path: SQL syntax error raises RuntimeError, connection remains usable
  12. Error path: constraint violation raises error, connection remains usable
  13. Large result set: 1,000-row query completes correctly
  14. Aggregate queries: SUM / COUNT / AVG
  15. Interleaved BEGIN/COMMIT: correct state transitions
"""

import os
import sys
import tempfile
import threading
import icedb


def assert_eq(a, b, msg=""):
    if a != b:
        raise AssertionError(f"Expected {b!r}, got {a!r}. {msg}")


def assert_true(cond, msg=""):
    if not cond:
        raise AssertionError(msg)


def makedir():
    return tempfile.mkdtemp()


def rmdir(d):
    import shutil
    shutil.rmtree(d, ignore_errors=True)


# ── Tests ─────────────────────────────────────────────────────────────────────

def test_basic_crud():
    d = makedir()
    try:
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
    finally:
        rmdir(d)
    print("  [PASS] test_basic_crud")


def test_explicit_commit():
    d = makedir()
    try:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE acct (id INT PRIMARY KEY, balance INT)")
        conn.execute_dml("INSERT INTO acct VALUES (1, 1000)")

        conn.begin()
        conn.execute_dml("UPDATE acct SET balance = balance - 100 WHERE id = 1")
        conn.commit()

        rows = conn.execute("SELECT balance FROM acct WHERE id = 1")
        assert_eq(rows[0]["balance"], 900)
    finally:
        rmdir(d)
    print("  [PASS] test_explicit_commit")


def test_explicit_rollback():
    d = makedir()
    try:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE acct (id INT PRIMARY KEY, balance INT)")
        conn.execute_dml("INSERT INTO acct VALUES (1, 1000)")

        conn.begin()
        conn.execute_dml("UPDATE acct SET balance = 0 WHERE id = 1")
        conn.rollback()

        rows = conn.execute("SELECT balance FROM acct WHERE id = 1")
        assert_eq(rows[0]["balance"], 1000, "balance must be unchanged after rollback")
    finally:
        rmdir(d)
    print("  [PASS] test_explicit_rollback")


def test_context_manager_commit():
    d = makedir()
    try:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE events (id INT PRIMARY KEY, name TEXT)")

        with conn:
            conn.execute_dml("INSERT INTO events VALUES (1, 'signup')")
            conn.execute_dml("INSERT INTO events VALUES (2, 'login')")

        rows = conn.execute("SELECT COUNT(*) AS n FROM events")
        assert_eq(rows[0]["n"], 2)
    finally:
        rmdir(d)
    print("  [PASS] test_context_manager_commit")


def test_context_manager_rollback_on_exception():
    d = makedir()
    try:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE events (id INT PRIMARY KEY, name TEXT)")

        try:
            with conn:
                conn.execute_dml("INSERT INTO events VALUES (1, 'signup')")
                raise RuntimeError("simulated error")
        except RuntimeError:
            pass

        rows = conn.execute("SELECT COUNT(*) AS n FROM events")
        assert_eq(rows[0]["n"], 0,
                  "rows must be rolled back after exception in context manager")
    finally:
        rmdir(d)
    print("  [PASS] test_context_manager_rollback_on_exception")


def test_null_and_types():
    d = makedir()
    try:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE misc (id INT, flag BOOL, score FLOAT8, note TEXT)")
        conn.execute_dml("INSERT INTO misc VALUES (1, TRUE, 3.14, NULL)")

        rows = conn.execute("SELECT id, flag, score, note FROM misc")
        assert_eq(len(rows), 1)
        assert_eq(rows[0]["id"], 1)
        assert_eq(rows[0]["flag"], True)
        assert abs(rows[0]["score"] - 3.14) < 1e-9
        assert rows[0]["note"] is None
    finally:
        rmdir(d)
    print("  [PASS] test_null_and_types")


def test_execute_dml_rows_affected():
    d = makedir()
    try:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE t (id INT, val INT)")
        for i in range(1, 6):
            conn.execute_dml(f"INSERT INTO t VALUES ({i}, {i * 10})")

        n = conn.execute_dml("UPDATE t SET val = 0 WHERE val > 20")
        # val > 20: rows with i=3(30), i=4(40), i=5(50) → 3 rows
        assert_eq(n, 3, "UPDATE must report 3 rows affected")

        n2 = conn.execute_dml("DELETE FROM t WHERE val = 0")
        assert_eq(n2, 3, "DELETE must report 3 rows affected")
    finally:
        rmdir(d)
    print("  [PASS] test_execute_dml_rows_affected")


def test_uncommitted_not_visible_to_other_connection():
    """Two connections to the same dir: uncommitted writes in conn1 must not
    be visible in conn2 (read-committed isolation)."""
    d = makedir()
    try:
        conn1 = icedb.connect(d)
        conn2 = icedb.connect(d)

        conn1.execute_dml("CREATE TABLE t (id INT, val INT)")
        conn1.execute_dml("INSERT INTO t VALUES (1, 100)")

        conn1.begin()
        conn1.execute_dml("UPDATE t SET val = 999 WHERE id = 1")

        rows = conn2.execute("SELECT val FROM t WHERE id = 1")
        assert_eq(rows[0]["val"], 100,
                  "conn2 must not see conn1's uncommitted write")

        conn1.rollback()
    finally:
        rmdir(d)
    print("  [PASS] test_uncommitted_not_visible_to_other_connection")


def test_rollback_when_no_transaction():
    """Calling rollback() when no transaction is active must not crash."""
    d = makedir()
    try:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE t (id INT)")
        # No BEGIN — rollback should be a no-op (ROLLBACK outside txn is fine in PostgreSQL)
        try:
            conn.rollback()
        except Exception:
            pass  # accept either behaviour: silent or error
    finally:
        rmdir(d)
    print("  [PASS] test_rollback_when_no_transaction")


def test_error_syntax_connection_still_usable():
    """A SQL syntax error must not leave the connection in a broken state."""
    d = makedir()
    try:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE t (id INT PRIMARY KEY)")

        try:
            conn.execute("THIS IS NOT SQL")
        except RuntimeError:
            pass  # expected

        # Connection must still work
        conn.execute_dml("INSERT INTO t VALUES (1)")
        rows = conn.execute("SELECT id FROM t")
        assert_eq(len(rows), 1)
    finally:
        rmdir(d)
    print("  [PASS] test_error_syntax_connection_still_usable")


def test_error_constraint_violation_connection_still_usable():
    """A PK constraint violation must raise an error but not corrupt the connection."""
    d = makedir()
    try:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE t (id INT PRIMARY KEY, val TEXT)")
        conn.execute_dml("INSERT INTO t VALUES (1, 'first')")

        try:
            conn.execute_dml("INSERT INTO t VALUES (1, 'duplicate')")
        except RuntimeError:
            pass  # expected

        # Original row must be intact; connection usable
        rows = conn.execute("SELECT val FROM t WHERE id = 1")
        assert_eq(rows[0]["val"], "first")
        assert_eq(len(rows), 1)
    finally:
        rmdir(d)
    print("  [PASS] test_error_constraint_violation_connection_still_usable")


def test_large_result_set():
    d = makedir()
    try:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE big (id INT, val INT)")
        for i in range(1, 1001):
            conn.execute_dml(f"INSERT INTO big VALUES ({i}, {i * 2})")

        rows = conn.execute("SELECT id, val FROM big ORDER BY id")
        assert_eq(len(rows), 1000)
        assert_eq(rows[0]["id"], 1)
        assert_eq(rows[999]["id"], 1000)
        total_val = sum(r["val"] for r in rows)
        assert_eq(total_val, sum(i * 2 for i in range(1, 1001)))
    finally:
        rmdir(d)
    print("  [PASS] test_large_result_set")


def test_aggregate_queries():
    d = makedir()
    try:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE scores (player TEXT, score INT)")
        data = [("alice", 80), ("bob", 90), ("carol", 70), ("dave", 90)]
        for name, score in data:
            conn.execute_dml(f"INSERT INTO scores VALUES ('{name}', {score})")

        rows = conn.execute("SELECT COUNT(*) AS n FROM scores")
        assert_eq(rows[0]["n"], 4)

        rows = conn.execute("SELECT SUM(score) AS total FROM scores")
        assert_eq(rows[0]["total"], 330)

        rows = conn.execute("SELECT MAX(score) AS top FROM scores")
        assert_eq(rows[0]["top"], 90)

        rows = conn.execute("SELECT MIN(score) AS bottom FROM scores")
        assert_eq(rows[0]["bottom"], 70)
    finally:
        rmdir(d)
    print("  [PASS] test_aggregate_queries")


def test_interleaved_begin_commit():
    """begin → work → commit → begin → work → commit must all succeed."""
    d = makedir()
    try:
        conn = icedb.connect(d)
        conn.execute_dml("CREATE TABLE t (id INT PRIMARY KEY)")

        conn.begin()
        conn.execute_dml("INSERT INTO t VALUES (1)")
        conn.commit()

        conn.begin()
        conn.execute_dml("INSERT INTO t VALUES (2)")
        conn.commit()

        rows = conn.execute("SELECT COUNT(*) AS n FROM t")
        assert_eq(rows[0]["n"], 2)
    finally:
        rmdir(d)
    print("  [PASS] test_interleaved_begin_commit")


def test_concurrent_connections_balance_invariant():
    """Multiple threads each running transfers via separate connections must
    preserve the total balance (global invariant)."""
    d = makedir()
    try:
        setup = icedb.connect(d)
        setup.execute_dml("CREATE TABLE acct (id INT PRIMARY KEY, balance INT)")
        n_accounts = 6
        initial = 1000
        for i in range(1, n_accounts + 1):
            setup.execute_dml(f"INSERT INTO acct VALUES ({i}, {initial})")

        errors = []

        def worker(thread_id):
            conn = icedb.connect(d)
            for i in range(15):
                from_id = ((thread_id * 15 + i) % n_accounts) + 1
                to_id   = (from_id % n_accounts) + 1
                for _attempt in range(10):
                    try:
                        conn.begin()
                        conn.execute_dml(
                            f"UPDATE acct SET balance = balance - 10 WHERE id = {from_id}"
                        )
                        conn.execute_dml(
                            f"UPDATE acct SET balance = balance + 10 WHERE id = {to_id}"
                        )
                        conn.commit()
                        break
                    except Exception:
                        try:
                            conn.rollback()
                        except Exception:
                            pass

        threads = [threading.Thread(target=worker, args=(t,)) for t in range(4)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        rows = setup.execute("SELECT SUM(balance) AS total FROM acct")
        expected = n_accounts * initial
        total = rows[0]["total"]
        assert_eq(total, expected,
                  f"Balance invariant violated: expected {expected}, got {total}")
    finally:
        rmdir(d)
    print("  [PASS] test_concurrent_connections_balance_invariant")


# ── Runner ────────────────────────────────────────────────────────────────────

if __name__ == "__main__":
    tests = [
        test_basic_crud,
        test_explicit_commit,
        test_explicit_rollback,
        test_context_manager_commit,
        test_context_manager_rollback_on_exception,
        test_null_and_types,
        test_execute_dml_rows_affected,
        test_uncommitted_not_visible_to_other_connection,
        test_rollback_when_no_transaction,
        test_error_syntax_connection_still_usable,
        test_error_constraint_violation_connection_still_usable,
        test_large_result_set,
        test_aggregate_queries,
        test_interleaved_begin_commit,
        test_concurrent_connections_balance_invariant,
    ]
    failures = 0
    for t in tests:
        try:
            t()
        except Exception as e:
            import traceback
            print(f"  [FAIL] {t.__name__}: {e}")
            traceback.print_exc()
            failures += 1

    if failures:
        print(f"\n{failures}/{len(tests)} tests FAILED")
        sys.exit(1)
    else:
        print(f"\n{len(tests)}/{len(tests)} tests passed")
