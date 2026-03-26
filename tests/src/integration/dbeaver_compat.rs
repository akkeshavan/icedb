/// DBeaver / pgAdmin compatibility scaffold (issue 31).
///
/// These are manual-only tests — DBeaver and pgAdmin cannot be driven
/// programmatically in CI.  This file documents the test checklist and
/// provides a stub that prints instructions when run with --ignored.
///
/// To run the manual checklist:
///   1. Start icedb: cargo run -p server -- --port 5432 --data-dir ./data
///   2. Open DBeaver → New Connection → PostgreSQL
///      Host: 127.0.0.1  Port: 5432  Database: icedb  User: icedb
///   3. Click "Test Connection" — must succeed without SSL errors.
///   4. Run the SQL below in DBeaver's SQL editor and verify results.
///
/// Expected results are noted inline.

/// DBeaver connection and basic query smoke.
///
/// Manual checklist — run this with `-- --ignored` to print the steps.
#[test]
#[ignore = "requires DBeaver or pgAdmin installed and running icedb server"]
fn test_dbeaver_connection_checklist() {
    println!();
    println!("=== DBeaver / pgAdmin Compatibility Checklist ===");
    println!();
    println!("Prerequisites:");
    println!("  cargo run -p server -- --port 5432 --data-dir /tmp/icedb-dbeaver");
    println!();
    println!("Step 1 — Connect");
    println!("  DBeaver: New Connection → PostgreSQL");
    println!("    Host: 127.0.0.1  Port: 5432  Database: icedb  User: icedb");
    println!("  Expected: connection succeeds, no TLS/SSL errors");
    println!();
    println!("Step 2 — Table browser");
    println!("  Expand the icedb database → Schemas → public → Tables");
    println!("  Expected: system tables (pg_class, pg_attribute, etc.) visible");
    println!();
    println!("Step 3 — CREATE and query");
    println!("  Run in SQL editor:");
    println!("    CREATE TABLE dbeaver_test (id SERIAL PRIMARY KEY, name TEXT);");
    println!("    INSERT INTO dbeaver_test (name) VALUES ('hello'), ('world');");
    println!("    SELECT * FROM dbeaver_test;");
    println!("  Expected: 2 rows returned, id column auto-incremented");
    println!();
    println!("Step 4 — Table viewer");
    println!("  Double-click dbeaver_test in the tree → Data tab");
    println!("  Expected: rows displayed in grid; inline edit works");
    println!();
    println!("Step 5 — ER diagram");
    println!("  Right-click schema → View Diagram");
    println!("  Expected: diagram renders without errors");
    println!();
    println!("Step 6 — pgAdmin (if available)");
    println!("  Add Server: host=127.0.0.1 port=5432 db=icedb user=icedb");
    println!("  Expected: Dashboard tab shows connection stats");
    println!("  Query Tool: SELECT version(); → should return icedb version string");
    println!();
    println!("Mark issue 31 resolved once all steps pass.");

    // Always pass — this test is documentation only.
}
