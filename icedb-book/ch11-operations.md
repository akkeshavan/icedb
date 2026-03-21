# Chapter 11: Running in Production

This chapter covers everything you need to deploy icedb reliably: choosing storage, starting the server, running as a system service, backup, monitoring, and understanding the current limitations compared to PostgreSQL.

**In this chapter:**
- Choosing a data directory and sizing storage
- Starting the server and available flags
- Running as a systemd service
- Connecting with psql, DBeaver, and application drivers
- Backup strategy (cold backup) and restore procedure
- WAL segment management
- Graceful shutdown and connection handling
- Monitoring and known limitations

## Choosing a Data Directory

The data directory is the single root of all icedb state. It contains:

- WAL segment files (`*.wal`)
- System catalog heap files (`catalog_*.heap`)
- User table heap files (`<oid>.heap`)
- B+ tree index files (`idx_<oid>_<col>.btree`)
- The checkpoint control file (`checkpoint.ctl`)

**Location:** Choose a directory on a fast, reliable disk. SSDs are strongly preferred. The WAL is append-only and write-latency-sensitive: each committed transaction fsyncs the WAL before returning. High-latency disks (spinning HDDs, network-attached storage) will slow commits proportionally.

**Sizing:** Storage consumption depends on your data volume and write pattern. Key factors:

- Each 8 kB page holds approximately 50–200 rows depending on row width.
- WAL segments are 16 MiB each. A sustained write workload of 1,000 inserts/second produces roughly 1 MiB of WAL per second. WAL segments can be deleted after checkpointing (manual process in the current version — no WAL archiving daemon).
- Dead tuples from UPDATE and DELETE accumulate until VACUUM runs. Without VACUUM, storage grows monotonically even if net data size is stable. Plan storage accordingly.

**Permissions:** The directory and all files within it should be owned by the user account that runs the icedb process. Set permissions to `700` (rwx for owner only) to prevent other users from reading the catalog files, which contain password verifiers.

```sh
mkdir -p /var/lib/icedb/data
chown icedb:icedb /var/lib/icedb/data
chmod 700 /var/lib/icedb/data
```

## Starting the Server

The recommended production startup command:

```sh
icedb-server --port 5432 --data-dir /var/lib/icedb/data
```

Or with `cargo run` during development:

```sh
cargo run -p server --release -- --port 5432 --data-dir /var/lib/icedb/data
```

**Flags:**

| Flag | Default | Description |
|------|---------|-------------|
| `--port PORT` | 5432 | TCP port to listen on |
| `--data-dir DIR` | `./data` | Data directory path |

The server binds to `0.0.0.0` (all interfaces) by default. There is no `--bind` flag — to restrict access to localhost only, use a firewall rule or a reverse proxy in front of the icedb port.

**Log output:**

The server uses the `log` crate with `env_logger`. Set `RUST_LOG` to control verbosity:

```sh
# Default: warnings only
RUST_LOG=warn icedb-server --port 5432 --data-dir ./data

# Info: startup messages and connection events
RUST_LOG=info icedb-server --port 5432 --data-dir ./data

# Debug: detailed WAL, tuple, and transaction events
RUST_LOG=debug icedb-server --port 5432 --data-dir ./data
```

The default filter (used when `RUST_LOG` is not set) is `warn`. Set `RUST_LOG=info` for a production deployment so startup and connection events appear in logs.

**On startup, expect:**

```
INFO  icedb listening on 0.0.0.0:5432
```

If the data directory is non-empty and contains valid WAL, WAL recovery may print:

```
INFO  Starting WAL recovery from LSN 42
INFO  WAL recovery finished; last replayed LSN = 87
```

## Running as a systemd Service (Linux)

Create `/etc/systemd/system/icedb.service`:

```ini
[Unit]
Description=icedb database server
After=network.target
Wants=network.target

[Service]
Type=simple
User=icedb
Group=icedb
ExecStart=/usr/local/bin/icedb-server --port 5432 --data-dir /var/lib/icedb/data
Restart=on-failure
RestartSec=5s
TimeoutStopSec=30s
KillMode=mixed     # sends SIGTERM to the main process, then SIGKILL to any remaining child processes
KillSignal=SIGTERM

# Log to journald
StandardOutput=journal
StandardError=journal
SyslogIdentifier=icedb

# Environment
Environment=RUST_LOG=info

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ReadWritePaths=/var/lib/icedb

[Install]
WantedBy=multi-user.target
```

Create the system user and enable the service:

```sh
# Create a system user with no login shell and no home directory
useradd --system --no-create-home --shell /usr/sbin/nologin icedb

# Create and secure the data directory
mkdir -p /var/lib/icedb/data
chown icedb:icedb /var/lib/icedb/data
chmod 700 /var/lib/icedb/data

# Place the binary (the server crate produces the icedb-server binary)
cp target/release/icedb-server /usr/local/bin/icedb-server
chmod 755 /usr/local/bin/icedb-server

# Enable and start the service
systemctl daemon-reload
systemctl enable icedb
systemctl start icedb
systemctl status icedb
```

View logs:

```sh
journalctl -u icedb -f
```

## Connecting with Standard PostgreSQL Tools

Because icedb speaks the PostgreSQL wire protocol, standard tools connect without modification.

### psql

```sh
# Default user (icedb) with no password
psql -h 127.0.0.1 -p 5432 -U icedb

# Specific user with password prompt
psql -h 127.0.0.1 -p 5432 -U alice -d mydb
```

### DBeaver / pgAdmin

In the connection settings:
- **Host:** `127.0.0.1`
- **Port:** `5432`
- **Database:** `icedb` (or the name of your database)
- **Username:** `icedb` (or your user)
- **Password:** leave blank for the `icedb` superuser when connecting locally; enter password for other roles

The server reports version `16.0 (icedb)` (as of this writing), which DBeaver and pgAdmin accept without complaint. Some metadata queries (e.g., listing extensions, pg_catalog introspection) may return errors or empty results if they rely on system views not yet implemented.

### Application Drivers (libpq, JDBC, asyncpg)

Any driver that uses the standard PostgreSQL wire protocol connects to icedb. Connection strings follow the standard format:

```
postgresql://icedb@127.0.0.1:5432/icedb
postgresql://alice:secret@127.0.0.1:5432/mydb
```

Python with `asyncpg`:
```python
import asyncio, asyncpg

async def main():
    conn = await asyncpg.connect('postgresql://icedb@127.0.0.1:5432/icedb')
    rows = await conn.fetch('SELECT * FROM books')
    print(rows)

asyncio.run(main())
```

Node.js with `pg`:
```javascript
const { Pool } = require('pg');
const pool = new Pool({ host: '127.0.0.1', port: 5432, user: 'icedb' });
pool.query('SELECT * FROM books').then(res => console.log(res.rows));
```

## Autovacuum

icedb includes an autovacuum daemon that automatically reclaims dead tuple space. When the server starts, autovacuum runs in the background and vacuums any table that has not been vacuumed in the last 5 minutes.

### Manual VACUUM

You can still run VACUUM manually at any time:

```sql
VACUUM;                    -- vacuum all tables
VACUUM orders;             -- vacuum specific table
VACUUM ANALYZE orders;     -- vacuum and update statistics
```

### Autovacuum behavior

- Runs every 60 seconds
- Vacuums tables that have not been touched in 5 minutes
- Reclaims pages occupied by dead tuples (rows updated or deleted)
- Updates `pd_prune_xid` on reclaimed pages

Unlike PostgreSQL's autovacuum, icedb's current implementation uses a time-based heuristic rather than dead tuple counts. A future release will track dead tuple counts per table for smarter triggering.

---

## Backup and Restore

icedb provides two complementary backup mechanisms: a **logical dump** via the CLI for portability, and a **cold (filesystem) backup** for full fidelity including indexes and WAL.

### Logical Dump and Restore (nkv-psql)

The `nkv-psql` CLI provides built-in dump and restore commands that work without stopping the server:

```
\dump /path/to/backup.sql
```

This writes all table schemas and data as SQL INSERT statements to the specified file.

```
\restore /path/to/backup.sql
```

This executes all SQL statements in the file against the current database.

**What is included:**
- `CREATE TABLE IF NOT EXISTS` for all user tables in the public schema
- `INSERT INTO` for all rows in each table

**What is NOT included:**
- Indexes (must be recreated manually with `CREATE INDEX`)
- Role definitions
- Sequences (SERIAL counter values reset to 1)
- ACL grants

**Example workflow:**

```
# Connect to source database
nkv-psql --data-dir /var/data/icedb_prod

icedb=# \dump /backups/icedb_2026_03_19.sql
Dumped 1247 statements to /backups/icedb_2026_03_19.sql

# Connect to new database
nkv-psql --data-dir /var/data/icedb_new

icedb=# \restore /backups/icedb_2026_03_19.sql
Restored 1247 statements from /backups/icedb_2026_03_19.sql
```

### Cold Backup (filesystem)

icedb does not yet have online backup (hot backup while the server is running). The recommended procedure for a full filesystem backup is a **cold backup**:

1. Stop the icedb server (sends `SIGTERM`, which is handled gracefully — see below).
2. Copy the entire data directory to backup storage.
3. Restart the server.

```sh
systemctl stop icedb

# rsync or tar the data directory
rsync -av /var/lib/icedb/data/ /backup/icedb-$(date +%Y%m%d)/

systemctl start icedb
```

The backup copy is a consistent snapshot because:
- The server is stopped, so no writes are in flight.
- All committed data was fsynced to heap files at commit time (via WAL replay on startup if needed).
- The WAL and checkpoint.ctl are included in the backup, so the backup can be recovered independently.

**To restore from backup:**

1. Stop the server.
2. Replace the data directory with the backup copy.
3. Start the server — WAL recovery runs automatically.

```sh
systemctl stop icedb
rm -rf /var/lib/icedb/data/*
rsync -av /backup/icedb-20260315/ /var/lib/icedb/data/
systemctl start icedb
```

Hot backup (backup without stopping the server) requires WAL archiving, which is planned for a future release.

## WAL Segment Management

WAL segments accumulate in the data directory (`*.wal` files, 16 MiB each). They are not automatically deleted. Under sustained write load, they will fill the disk if not managed.

**Manual checkpoint:** There is no SQL `CHECKPOINT` command yet. The server performs an implicit checkpoint on startup (by recovering to the end of the WAL). WAL segments before the last checkpoint LSN are safe to delete, but this must be done manually and carefully.

**Current guidance:** Monitor the data directory size. If WAL segments accumulate, archive them to cold storage and delete the older segments (keeping at least the most recent 2–3 segments as a safety margin). Automated WAL archiving and checkpoint management are on the roadmap.

## Graceful Shutdown

Send `SIGTERM` to the server process to request a graceful shutdown:

```sh
systemctl stop icedb      # systemd sends SIGTERM
kill -TERM <server-pid>   # direct signal
```

The server receives `SIGTERM`, finishes processing any in-progress request (a single SQL statement), stops accepting new connections, and exits. All committed data is already durable on disk. In-flight uncommitted transactions are effectively aborted (their WAL records will not include a Commit; WAL recovery on next startup treats them as aborted).

Force-killing with `SIGKILL` is safe — WAL recovery on next startup reconstructs the correct state.

## Connection Handling

icedb spawns one Tokio task per incoming TCP connection. Each task runs the full protocol loop until the connection closes. There is no hard connection limit in the current implementation; the practical limit is OS-level resources (file descriptors, thread pool size, memory).

For high-connection workloads, use a connection pooler (e.g., `pgBouncer`) in front of icedb. pgBouncer is compatible with icedb via the standard PostgreSQL protocol.

## Monitoring

**Health check:** Connect with psql and run a simple query:

```sh
psql -h 127.0.0.1 -p 5432 -U icedb -c "SELECT 1" > /dev/null 2>&1 && echo "UP" || echo "DOWN"
```

**Disk usage:** Monitor the data directory size:

```sh
du -sh /var/lib/icedb/data/
du -sh /var/lib/icedb/data/*.wal  # WAL segment total
```

**Log monitoring:** Watch for `ERROR` and `WARN` lines in the journal:

```sh
journalctl -u icedb --since "1 hour ago" | grep -E "ERROR|WARN"
```

## Performance

### Prepared Statements

For workloads that repeat the same query with different parameters, use prepared statements to avoid repeated parse and plan overhead:

```sql
PREPARE find_order AS SELECT * FROM orders WHERE customer_id = $1 AND status = $2;
EXECUTE find_order(42, 'pending');
EXECUTE find_order(99, 'shipped');
DEALLOCATE find_order;
```

Prepared statements are session-scoped. Each new connection starts with an empty prepared statement cache. For connection poolers like pgBouncer in transaction-pooling mode, use `DEALLOCATE ALL` at the end of each transaction to avoid statement leaks.

---

## Known Limitations Compared to PostgreSQL

The following capabilities are absent in the current icedb release. They are planned for future versions.

**Limited ANALYZE support.** The `pg_statistic` table is defined but not populated with per-column histograms. The cost-based optimizer currently rewrites `Filter(TableScan)` to `IndexScan` on equality predicates for indexed columns, but does not yet use statistics-driven selectivity estimates for join reordering or more complex plans.

**No replication.** There is no streaming replication, logical replication, or standby server support. Icedb is a single-node database. High availability requires cold backup and manual failover.

**No tablespaces.** All data lives in the single data directory. You cannot place different tables on different disks.

**No partitioning.** `CREATE TABLE ... PARTITION BY` is not supported.

**No extensions.** The `CREATE EXTENSION` command and the PostgreSQL extension API are not implemented.

**Partial `pg_catalog` coverage.** The most common `pg_catalog` and `information_schema` views are implemented. Some less common views and columns may return stub values or be absent. The CLI meta-commands (`\d`, `\dt`, `\du`) remain the most reliable way to inspect schema metadata.

**No SSL/TLS.** The wire protocol connection is unencrypted. Use a TLS-terminating proxy for production deployments.

**No `SERIAL` / sequences.** Auto-incrementing columns require the application to manage IDs or use a separate counter table.

**No triggers or stored procedures.** User-defined functions using `LANGUAGE SQL` are supported (see Chapter 4). PL/pgSQL and other procedural languages are not yet implemented.

**`EXPLAIN` support.** `EXPLAIN` and `EXPLAIN ANALYZE` are implemented and show the logical plan tree. Advanced plan details (cost estimates, actual row counts) are minimal in the current version.

These limitations reflect the development phase of the project (Phases 1–9 complete; Phase 10 in progress). They do not reflect architectural constraints — the design supports all of these features. They represent engineering work remaining on the roadmap.
