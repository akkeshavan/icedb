# Chapter 12: Troubleshooting

This chapter covers the most common problems encountered when running icedb, with diagnosis steps and solutions for each.

**In this chapter:**
- Connection problems: port conflicts, permission errors, authentication failures
- SQL errors: table not found, serialization failure, constraint violations, privilege errors
- Storage problems: disk full, WAL segment management
- Debugging: enabling debug logging, reading WAL records, rebuilding indexes
- Server won't start after a crash
- Checking data directory integrity

---

## Connection Problems

### "Port already in use"

**Symptom:**

```
Error: Address already in use (os error 98)
```

or (on macOS):

```
Error: Address already in use (os error 48)
```

**Cause:** Another process (possibly another icedb instance, PostgreSQL, or another application) is already listening on port 5432.

**Solutions:**

1. Find and stop the conflicting process:
   ```sh
   lsof -i :5432
   # or
   ss -tlnp | grep 5432
   ```
   If it is an icedb process you forgot to stop:
   ```sh
   pkill -f icedb-server
   ```

2. Start icedb on a different port:
   ```sh
   cargo run -p server -- --port 5433 --data-dir ./data
   # Then connect with:
   psql -h 127.0.0.1 -p 5433 -U icedb
   ```

---

### "Data directory not found" or Permission Denied

**Symptom:**

```
Error creating data directory: No such file or directory (os error 2)
```

or:

```
Failed to open WAL: Permission denied (os error 13)
```

**Cause:** The specified `--data-dir` path does not exist, or the process does not have write permission.

**Solutions:**

1. Create the directory manually and verify the path:
   ```sh
   mkdir -p /var/lib/icedb/data
   ls -la /var/lib/icedb/
   ```

2. Check ownership and permissions:
   ```sh
   ls -la /var/lib/icedb/data
   # Should show owner = icedb (or whatever user runs the server)
   ```

3. Fix permissions:
   ```sh
   chown -R icedb:icedb /var/lib/icedb/data
   chmod 700 /var/lib/icedb/data
   ```

---

### "Cannot connect with psql"

**Symptom:**

```
psql: error: connection to server at "127.0.0.1", port 5432 failed:
Connection refused
```

**Cause:** The icedb server is not running, or is listening on a different port or interface.

**Diagnosis:**

1. Check whether the server is running:
   ```sh
   ps aux | grep icedb-server
   # or
   systemctl status icedb
   ```

2. Check which port it is listening on:
   ```sh
   lsof -i -P | grep icedb
   # or
   ss -tlnp | grep 5432
   ```

3. Try connecting explicitly by IP:
   ```sh
   psql -h 127.0.0.1 -p 5432 -U icedb
   ```
   Some `psql` versions default to a Unix socket which icedb does not support. Always use `-h 127.0.0.1`.

**Solution:** Start the server if it is not running:

```sh
cargo run -p server --release -- --port 5432 --data-dir /var/lib/icedb/data
```

---

### "Authentication failed"

**Symptom:**

```
psql: error: connection to server at "127.0.0.1", port 5432 failed:
FATAL:  password authentication failed for user "alice"
```

**Causes:**
- The role does not exist.
- The password provided does not match the stored SCRAM verifier.
- The role exists but has `rolcanlogin = false`.

**Solutions:**

1. Verify the role exists using the CLI (which bypasses authentication):
   ```sh
   cargo run -p cli -- --data-dir ./data
   icedb=# \du
   ```

2. If the role does not exist, create it:
   ```sql
   CREATE ROLE alice WITH LOGIN PASSWORD 'correctpassword';
   ```

3. If the password is wrong but you cannot reset it (you have lost admin access), use the CLI as the `icedb` superuser to recreate the role:
   ```sql
   -- In the CLI (no authentication required)
   -- Drop and recreate, or (once ALTER ROLE is implemented) change the password
   ```

4. For the `icedb` superuser with no password, connect via the CLI's embedded mode instead of TCP:
   ```sh
   cargo run -p cli -- --data-dir ./data
   ```
   The CLI does not authenticate — it opens the engine directly.

---

## SQL Errors

### "Table not found"

**Symptom:**

```
ERROR:  Table not found: myTable
```

**Causes:**
- The table name is misspelled.
- The table is in a different schema (icedb only supports `public` currently).
- Case sensitivity: icedb stores table names in lowercase; `MyTable` and `mytable` are the same.
- The table was created in a different data directory.

**Solutions:**

1. List all tables to verify the name:
   ```
   \dt
   ```

2. Check the exact spelling (all lowercase):
   ```sql
   SELECT * FROM mytable;  -- not MyTable
   ```

3. Verify you are pointing at the correct data directory. Tables created with `--data-dir ./data1` are not visible when connecting to `--data-dir ./data2`.

---

### "Serialization failure" (SSI in development)

**Symptom:**

```
ERROR:  could not serialize access due to concurrent update
SQLSTATE: 40001
```

**Cause:** This error will appear once full SSI cycle detection is implemented. Currently, SSI tracking is in place but `check_serializable_conflict` always passes (returns `Ok(())`), so SQLSTATE 40001 is not yet returned in practice. When SSI is active, icedb will return SQLSTATE 40001 when two Serializable transactions conflict in a way that cannot be resolved. This is **correct behavior**, not a bug.

**Solution:** Retry the transaction from the beginning. Serialization failures are a normal part of operating under Serializable isolation. Application code using Serializable transactions must be written to retry on this error:

```python
import icedb

conn = icedb.connect("./mydata")

for attempt in range(5):
    try:
        conn.execute("BEGIN TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        conn.execute_dml("UPDATE accounts SET balance = balance - 100 WHERE id = 1")
        conn.execute_dml("UPDATE accounts SET balance = balance + 100 WHERE id = 2")
        conn.execute("COMMIT")
        break
    except Exception as e:
        if "serialize" in str(e).lower():
            conn.execute("ROLLBACK")
            continue
        raise
```

---

### "Null value in column violates not-null constraint"

**Symptom:**

```
ERROR:  null value in column "name" violates not-null constraint
```

**Cause:** An INSERT or UPDATE attempted to write NULL into a column declared `NOT NULL`.

**Solution:** Provide a non-null value for the column:

```sql
-- Wrong:
INSERT INTO products (id, name) VALUES (1, NULL);

-- Right:
INSERT INTO products (id, name) VALUES (1, 'Widget');
```

---

### "Role does not have CREATE TABLE privilege"

**Symptom:**

```
ERROR:  Role 'alice' does not have CREATE TABLE privilege
```

**Cause:** The role connecting to the server has `rolcreatedb = false` and attempted DDL.

**Solution:** Connect as a superuser to perform DDL, or grant the role DDL privileges by recreating it with superuser:

```sql
-- As icedb superuser:
CREATE ROLE alice WITH LOGIN SUPERUSER PASSWORD 'secret';
```

Note: `ALTER ROLE` is not yet implemented. To change privileges, drop and recreate the role (losing the password; set a new one).

---

## Storage Problems

### "Disk full"

**Symptom:**

```
ERROR:  No space left on device (os error 28)
```

The server may also fail to start or crash with this error.

**Causes:** The disk containing the data directory has run out of space. WAL segments are typically the first to fill the disk under sustained write load.

**Diagnosis:**

```sh
df -h /var/lib/icedb/data
du -sh /var/lib/icedb/data/*.wal
```

**Solutions:**

1. Archive and delete old WAL segments:
   ```sh
   # Stop the server first
   systemctl stop icedb

   # Archive segments you want to keep
   cp /var/lib/icedb/data/0000000000000001.wal /archive/

   # Delete segments older than the most recent 2
   ls /var/lib/icedb/data/*.wal | sort | head -n -2 | xargs rm

   # Restart
   systemctl start icedb
   ```

2. Move the data directory to a larger disk and update the `--data-dir` path.

3. Free disk space on the host.

**Important:** Do not delete WAL segments that are newer than the last checkpoint LSN (recorded in `checkpoint.ctl`). Deleting recent WAL segments makes crash recovery impossible. Keep at minimum the segments written since the last checkpoint.

---

## Debugging and Diagnostics

### Enabling Debug Logging

Set the `RUST_LOG` environment variable before starting the server:

```sh
RUST_LOG=debug cargo run -p server -- --port 5432 --data-dir ./data
```

Debug output includes:
- WAL record appends: `txn 200: inserted tuple at (page=0, slot=1)`
- Transaction lifecycle: `txn 200: committed`, `txn 201: aborted`
- WAL segment rotation: `WAL segment rotated → 0000000000000002.wal`
- SSI information: `SSI commit check for xid=200: read_set=5 entries, write_set=2 entries`
- Connection events: `accepted connection from 127.0.0.1:54321`

Use `RUST_LOG=icedb=debug` to filter to only icedb crates while keeping other crates at warn level.

### Reading WAL Records for Debugging

WAL records are binary files. To understand what is in a WAL segment, write a small Rust program using the `wal::reader::WalReader`:

```rust
use wal::reader::WalReader;
use std::path::Path;

fn main() {
    let mut reader = WalReader::open(Path::new("./data"), 0).unwrap();
    while let Some(record) = reader.next_record().unwrap() {
        println!(
            "LSN={} XID={} type={:?} page={} data_len={}",
            record.lsn, record.xid, record.record_type,
            record.page_no, record.data.len()
        );
    }
}
```

A typical output for a simple INSERT + COMMIT looks like:

```
LSN=1 XID=3 type=PageImage page=0 data_len=8192
LSN=2 XID=3 type=Commit page=0 data_len=0
```

The `PageImage` record at LSN=1 contains the full 8 kB page after the INSERT. The `Commit` record at LSN=2 confirms the transaction committed.

### Rebuilding Corrupted Indexes

If an index becomes inconsistent (this should not happen under normal operation, but can occur after a hardware error or manual file corruption), the safest recovery is to drop and recreate the index:

```sql
-- The index cannot be dropped by name in the current version;
-- this drops the table and recreates it (losing data).
-- A better option: since index files are named idx_<oid>_<col>.btree,
-- stop the server, delete the corrupted index file, and restart.
-- The index will be missing (queries will use sequential scan).
-- Then recreate:
CREATE INDEX ON tablename (columnname);
```

Steps:

1. Stop the server: `systemctl stop icedb`
2. Delete the corrupted index file: `rm /var/lib/icedb/data/idx_<oid>_<col>.btree`
3. Start the server: `systemctl start icedb`
4. Recreate the index: `CREATE INDEX ON tablename (columnname);`

### Server Won't Start After a Crash

**Symptom:** The server exits immediately after startup with an error during WAL recovery.

**Common cause:** A genuinely corrupt WAL record (not just a partial write at the end). This is rare but can occur with hardware errors.

**Diagnosis:**

```sh
RUST_LOG=info cargo run -p server -- --port 5432 --data-dir ./data 2>&1 | head -50
```

Look for lines like:

```
INFO  Starting WAL recovery from LSN 42
ERROR  Corrupt WAL record at LSN 87: ...
```

**Solutions:**

1. If the corrupt record is at the end of the last WAL segment (which is the common case for a crash mid-write), WAL recovery stops at that point and the database is consistent up to the last good record. The server should start normally after this truncation.

2. If the corrupt record is in the middle of a segment, it indicates more serious hardware damage. Restore from the most recent cold backup.

3. To inspect which LSN `checkpoint.ctl` records:
   ```sh
   # checkpoint.ctl is a raw 8-byte little-endian u64
   python3 -c "
   import struct
   with open('./data/checkpoint.ctl', 'rb') as f:
       lsn = struct.unpack('<Q', f.read(8))[0]
       print(f'Last checkpoint LSN: {lsn}')
   "
   ```

### Checking Data Directory Integrity

Verify that all heap files and WAL segments are readable and correctly sized:

```sh
# WAL segments should be <= 16 MiB (16777216 bytes)
ls -la /var/lib/icedb/data/*.wal

# Heap files should be multiples of 8192 bytes
for f in /var/lib/icedb/data/*.heap; do
    size=$(stat --format="%s" "$f")
    if (( size % 8192 != 0 )); then
        echo "CORRUPT: $f has size $size (not a multiple of 8192)"
    else
        echo "OK: $f ($size bytes, $((size/8192)) pages)"
    fi
done
```

A heap file with a size that is not a multiple of 8192 indicates a partial write — the page was being written when the system crashed. WAL recovery will overwrite that page with the correct content from the WAL, so this is handled automatically on next startup.

---

## Getting Help

- Review the debug log output (`RUST_LOG=debug`) — it contains detailed per-operation information.
- Check the WAL records to understand what the last committed state was.
- For bugs, open an issue on the repository with: the icedb version (git commit hash), the `RUST_LOG=debug` output, the SQL that triggered the problem, and the contents of `checkpoint.ctl` (the 8-byte LSN value).
