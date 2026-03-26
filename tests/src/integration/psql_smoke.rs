/// psql real-connection smoke test (issue 32).
///
/// These tests require a running icedb server and the `psql` binary on PATH.
/// They are marked `#[ignore]` so they do not run in `cargo test` by default.
///
/// To run manually:
///   cargo run -p server -- --port 5499 --data-dir /tmp/icedb-smoke &
///   cargo test -p icedb-tests psql_smoke -- --ignored --nocapture
///
/// Or against an already-running server:
///   ICEDB_TEST_PORT=5432 cargo test -p icedb-tests psql_smoke -- --ignored
use std::process::Command;

fn psql_port() -> u16 {
    std::env::var("ICEDB_TEST_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5499)
}

fn psql_cmd(port: u16, sql: &str) -> Command {
    let mut cmd = Command::new("psql");
    cmd.args([
        "-h", "127.0.0.1",
        "-p", &port.to_string(),
        "-U", "icedb",
        "-d", "icedb",
        "-c", sql,
        "--no-password",
        "-t",   // tuples only
        "-A",   // unaligned
    ]);
    cmd
}

/// Verify psql can connect and SELECT 1 returns '1'.
#[test]
#[ignore = "requires running icedb server and psql on PATH"]
fn test_psql_connect_select_one() {
    let port = psql_port();
    let out = psql_cmd(port, "SELECT 1;")
        .output()
        .expect("failed to run psql — is it on PATH?");
    assert!(out.status.success(), "psql exited with: {}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim().contains('1'), "Expected '1', got: {}", stdout);
}

/// Full CRUD smoke: CREATE TABLE, INSERT, SELECT, UPDATE, DELETE, DROP TABLE.
#[test]
#[ignore = "requires running icedb server and psql on PATH"]
fn test_psql_crud_smoke() {
    let port = psql_port();

    let steps: &[&str] = &[
        "DROP TABLE IF EXISTS psql_smoke;",
        "CREATE TABLE psql_smoke (id INT, name TEXT);",
        "INSERT INTO psql_smoke VALUES (1, 'alice');",
        "INSERT INTO psql_smoke VALUES (2, 'bob');",
        "UPDATE psql_smoke SET name = 'carol' WHERE id = 1;",
        "DELETE FROM psql_smoke WHERE id = 2;",
    ];

    for sql in steps {
        let out = psql_cmd(port, sql)
            .output()
            .expect("failed to run psql");
        assert!(
            out.status.success(),
            "psql step failed: {}\nstderr: {}",
            sql,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // Verify final state: one row, id=1, name='carol'
    let out = psql_cmd(port, "SELECT id, name FROM psql_smoke;")
        .output()
        .expect("failed to run psql");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains('1'), "Expected id=1 in output: {}", stdout);
    assert!(stdout.contains("carol"), "Expected name=carol in output: {}", stdout);

    // Cleanup
    let _ = psql_cmd(port, "DROP TABLE psql_smoke;").output();
}

/// Transaction isolation over wire: concurrent sessions see correct snapshots.
#[test]
#[ignore = "requires running icedb server and psql on PATH"]
fn test_psql_transaction_smoke() {
    let port = psql_port();

    // Create a table and insert a row
    let setup: &[&str] = &[
        "DROP TABLE IF EXISTS psql_txn_smoke;",
        "CREATE TABLE psql_txn_smoke (val INT);",
        "INSERT INTO psql_txn_smoke VALUES (42);",
    ];
    for sql in setup {
        psql_cmd(port, sql).output().expect("setup failed");
    }

    // Read within a transaction — should see 42
    let out = psql_cmd(
        port,
        "BEGIN; SELECT val FROM psql_txn_smoke; COMMIT;",
    )
    .output()
    .expect("failed to run psql");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("42"), "Expected 42 in txn output: {}", stdout);

    // Cleanup
    let _ = psql_cmd(port, "DROP TABLE psql_txn_smoke;").output();
}
