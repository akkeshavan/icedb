# Chapter 13 — The icedb Admin UI

**In this chapter:**
- What the Admin UI is and when to use it instead of the CLI
- Building the frontend and starting the admin server
- Logging in and navigating the interface
- Server status monitoring
- Role management: creating, viewing, and deleting roles
- Schema and table browser: columns, indexes, row counts, drop, truncate
- Query console for ad-hoc SQL
- Permissions page (current state)
- REST API reference for scripting and automation
- Security considerations and known limitations

---

## Overview

The icedb Admin UI is a localhost web application for database administration. It runs as a separate binary (`admin-server`) that embeds the icedb engine directly against a data directory and serves a React single-page application at `http://localhost:8080`.

### When to use it vs the CLI

The `nkv-psql` CLI (Chapter 8) is for SQL work: writing queries, running DML, testing transactions. The Admin UI is for administration:

- Inspecting and managing roles without remembering SQL syntax
- Browsing table schemas and index definitions at a glance
- Dropping or truncating a table with a confirmation dialog
- Checking server health (WAL LSN, uptime, buffer pool state) in a dashboard
- Running one-off queries via a browser-based query console

Neither tool replaces `psql` for application-level SQL work or driver testing. The Admin UI is an operator tool, not a query IDE.

### Architecture

The `admin-server` binary initializes the same WAL writer, transaction manager, catalog, and SQL engine that the main icedb server uses, then opens the specified data directory directly. It exposes a REST API under `/api` and serves the compiled React SPA from `admin-ui/dist/` as a static fallback.

Because the admin server opens the data directory through the same engine code, it can read and write catalog state, execute SQL, and inspect WAL position — without a separate server process running. This also means you should not run the admin server and `icedb-server` against the same data directory simultaneously in production (see [Important: data directory access](#important-data-directory-access)).

---

## Prerequisites

- icedb built from source (Chapter 2)
- Node.js 18 or later and npm (for building the React frontend)
- A data directory that has been initialized by icedb (or one that will be created on first run)

---

## Building and Starting

### Build the frontend (one-time setup)

The React frontend must be compiled before the admin server can serve it. Run this once from the workspace root, and again after any frontend code changes:

```bash
cd admin-ui
npm install
npm run build
cd ..
```

This produces `admin-ui/dist/`. The admin server looks for this directory relative to the process working directory (i.e., the workspace root when using `cargo run`). If the directory is absent, the server starts without serving the SPA and logs a warning:

```
WARN admin_server: admin-ui/dist not found at /path/to/admin-ui/dist.
Run `cd admin-ui && npm run build` to build the frontend.
```

The REST API remains fully functional even without the SPA.

### Start the admin server

Run from the workspace root:

```bash
ICEDB_ADMIN_TOKEN=your-secret-token cargo run -p admin-server -- \
  --port 8080 \
  --data-dir ./data
```

**Flags:**

| Flag | Default | Description |
|------|---------|-------------|
| `--port PORT` | `8080` | HTTP port the admin server listens on |
| `--data-dir DIR` | `./data` | Path to the icedb data directory |
| `--admin-token TOKEN` | `changeme` | Admin token for bearer authentication |

The admin token can also be provided via the `ICEDB_ADMIN_TOKEN` environment variable. If neither `--admin-token` nor `ICEDB_ADMIN_TOKEN` is set, the server falls back to the insecure default `changeme` and logs a warning. Always set an explicit token before exposing the admin server on a network.

On startup you should see:

```
INFO  admin_server: Initializing icedb components from data dir: ./data
INFO  admin_server: Serving React SPA from: /path/to/admin-ui/dist
INFO  admin_server: icedb Admin UI running at http://localhost:8080
```

### Important: data directory access

The admin server opens the icedb data directory directly using the engine — it is not a client connecting over TCP to `icedb-server`. This means:

- The data directory must not be opened by another process simultaneously in production. Running both `icedb-server` and `admin-server` against the same directory is not prevented by a hard lock, but it will produce undefined behavior as both processes write to the same WAL and catalog files.
- The safe workflow is: stop `icedb-server`, start `admin-server` for administration, stop `admin-server`, restart `icedb-server`.
- For development and read-heavy inspection, the risk is lower but still present.

### Dev mode (hot-reload)

During frontend development, Vite's dev server provides hot-reload. The Vite config proxies `/api` requests to the backend at `:8080`.

```bash
# Terminal 1: backend
ICEDB_ADMIN_TOKEN=secret cargo run -p admin-server -- --data-dir ./data

# Terminal 2: frontend with hot-reload
cd admin-ui
npm run dev
# Open http://localhost:5173
```

In this setup, open `http://localhost:5173` (Vite's port) — not `:8080` — for hot-reload. API calls are proxied transparently.

---

## Logging In

Open `http://localhost:8080` in a browser.

You will see a login screen with a single field: **Admin Token**. Enter the value you set for `ICEDB_ADMIN_TOKEN`. The token is stored in browser `localStorage` under the key `icedb_admin_token`. It persists across page reloads and browser restarts until you explicitly log out.

To log out, click **Log out** in the sidebar footer. This clears the token from `localStorage` and returns you to the login screen.

All API calls include the stored token as `Authorization: Bearer <token>`. If the token is wrong or missing, every API call returns HTTP 401.

---

## Navigating the UI

The left sidebar contains the main navigation. Entries in order:

| Page | Description |
|------|-------------|
| Server Status | Health dashboard — version, uptime, WAL LSN, buffer pool |
| Roles | List, create, and delete database roles |
| Schemas | Schema list, table browser, table detail |
| Query Console | Ad-hoc SQL execution with a result table |
| Permissions | Current status of table-level ACL support |

The sidebar footer contains:
- A **dark mode** toggle
- A **Log out** button

All pages require a valid admin token. Navigating to any page while unauthenticated (or after logging out) returns you to the login screen.

---

## Server Status Page

The Server Status page polls `GET /api/status` every 10 seconds and displays:

| Field | Description |
|-------|-------------|
| Version | The `admin-server` crate version from `Cargo.toml` |
| Uptime | Seconds since the admin server process started |
| Data directory | The `--data-dir` path passed at startup |
| Port | The HTTP port the server is listening on |
| WAL LSN | Current write-ahead log sequence number (monotonically increasing `u64`) |
| Buffer pool frames | Fixed at 256 (the buffer pool frame count) |
| Buffer pool dirty | Currently always 0 (dirty frame tracking is not yet exposed via the API) |
| Table count | Number of tables in the `public` schema |

Use this page to confirm the admin server started correctly, verify the expected data directory is open, and track WAL advancement during write activity.

The WAL LSN updates in real time as writes occur. If the LSN is not advancing during expected write activity, this indicates a problem with WAL flushing.

---

## Role Management

### Viewing roles

The Roles page calls `GET /api/roles`, which runs `SELECT rolname FROM pg_authid` via the SQL engine and then fetches full role details from the catalog for each row. The table shows:

| Column | Description |
|--------|-------------|
| Name | Role name (`rolname`) |
| Superuser | `rolsuper` flag |
| Create DB | `rolcreatedb` flag |
| Create Role | `rolcreaterole` flag |
| Can Login | `rolcanlogin` flag |
| Has Password | Whether a password verifier is stored (does not show the hash) |

### Creating a role

Click **New Role**. A form opens with:

- **Name** (required): the role name, as it will appear in `pg_authid`
- **Password** (required): hashed with SCRAM-SHA-256 before storage (see Chapter 7 for the storage format)
- **Superuser** checkbox: sets `rolsuper`
- **Can Login** checkbox: sets `rolcanlogin` (defaults to enabled)

The `rolcreatedb` and `rolcreaterole` fields are accepted by the API but are currently not stored independently — they track the superuser flag in the catalog. Use `ALTER ROLE` via the Query Console or CLI if you need to set them independently once the catalog supports it.

Click **Create Role**. On success the role appears in the list. On error (e.g., duplicate name) an error message is shown inline.

Passwords are passed to the server as plaintext over the loopback connection and hashed server-side using `auth::scram::hash_password`. The stored value in `pg_authid.rolpassword` is always a SCRAM-SHA-256 verifier — the plaintext is never written to disk.

### Deleting a role

Click the delete icon on a role row. A confirmation dialog appears. Click **Confirm** to proceed.

The server enforces one safety check: the last superuser cannot be deleted. If you attempt to delete the only role with `rolsuper = true`, the API returns HTTP 400 with:

```json
{ "error": "Cannot delete the last superuser" }
```

The UI surfaces this as an error message.

### Editing a role

The `PUT /api/roles/:name` endpoint exists and accepts `password`, `rolsuper`, and `rolcanlogin`. The UI does not currently expose an edit form. To update a role's password or flags after creation, use the Query Console or the `nkv-psql` CLI:

```sql
ALTER ROLE alice WITH PASSWORD 'newpassword';
ALTER ROLE alice WITH SUPERUSER;
ALTER ROLE alice WITH NOSUPERUSER;
```

---

## Schema and Table Browser

### Schemas

The Schemas page calls `GET /api/schemas`, which returns a hardcoded list: `["public", "pg_catalog"]`. icedb currently has a single user-accessible schema (`public`). Each schema entry links to its table list.

### Tables

Click a schema name to navigate to its table list. The page calls `GET /api/schemas/:schema/tables`, which queries the catalog's `list_tables` method. The table count is shown next to the schema name.

Click a table name to open the Table Detail view.

### Table Detail

The Table Detail view shows three panels, populated by three separate API calls:

**1. Columns** — from `GET /api/tables/:schema/:table`

| Column | Description |
|--------|-------------|
| Name | Column name |
| Type | Data type (from `catalog::DataType` debug representation) |
| Not Null | Whether `NOT NULL` is set |

**2. Indexes** — from `GET /api/tables/:schema/:table/indexes`

Lists all B+ tree indexes on the table. Each entry shows the indexed column name and type (`btree`). Index discovery works by iterating the table's columns and checking for a registered index path in the catalog. If no indexes are found, "No indexes" is displayed.

**3. Row count** — from `GET /api/tables/:schema/:table/rowcount`

Executes `SELECT COUNT(*) FROM <table>` (or `SELECT COUNT(*) FROM <schema>.<table>` for non-public schemas) and displays the result. This is an exact count, not an estimate — it performs a full table scan via the query engine. For large tables, this call may be slow.

### Dropping a table

Click **Drop Table** on the Table Detail page. A confirmation dialog requires you to type the exact table name before the button becomes active. This is irreversible.

The server executes `DROP TABLE IF EXISTS <table>`. On success, you are returned to the schema's table list. The table no longer appears.

### Truncating a table

Click **Truncate** on the Table Detail page. A confirmation dialog appears. On confirmation, the server executes `DELETE FROM <table>`, which removes all rows. The table structure (columns, indexes) is preserved.

Note: this uses `DELETE FROM`, not SQL `TRUNCATE`. Dead tuple space from the deleted rows is not immediately reclaimed — it will be cleaned up by a future VACUUM pass. Sequences (if any) are not reset.

The response includes the number of rows deleted:

```json
{
  "message": "Table 'public.books' truncated",
  "rows_deleted": 42
}
```

---

## Query Console

The Query Console page provides a text area for typing SQL and a **Run** button (also triggered by Ctrl+Enter). The query is sent to `POST /api/query` with `{ "sql": "..." }`.

Results appear below the editor as a table with column headers. For DML statements (`INSERT`, `UPDATE`, `DELETE`), the result shows the command tag and rows affected rather than a result set.

Errors are displayed in red below the editor. The error field contains the engine's error message. The HTTP response is always 200 — the error is carried in the JSON body's `error` field, not in the HTTP status code.

### Example queries

```sql
-- Check active table row counts
SELECT COUNT(*) FROM books;

-- Inspect a specific row
SELECT * FROM authors WHERE id = 1;

-- Run DDL
CREATE TABLE logs (id INTEGER, message TEXT, created_at TEXT);

-- Check WAL-visible transaction state
SELECT rolname, rolsuper, rolcanlogin FROM pg_authid;
```

### Limitations

- One statement per submission. Multi-statement batches separated by semicolons are not supported in the query console (unlike the CLI, which buffers until semicolon). Submit each statement separately.
- Query history is not persisted across page reloads. The text area clears on navigation.
- No syntax highlighting in the current implementation. A CodeMirror 6 upgrade is planned.
- Result sets are limited to what the browser can render reasonably. Very large result sets (hundreds of thousands of rows) will be slow to display — use the CLI or a driver for bulk queries.

---

## Permissions Page

The Permissions page calls `GET /api/permissions`, which returns:

```json
{
  "note": "Table-level ACLs not yet implemented in the engine. Role-level privileges are managed via the Roles API."
}
```

This is a stub. Table-level `GRANT` and `REVOKE` support is planned but not yet implemented in the SQL engine. Role-level privileges (`rolsuper`, `rolcanlogin`, `rolcreatedb`, `rolcreaterole`) are managed via the Roles page and the Query Console.

For the current state of privilege enforcement in the engine, see Chapter 7.

---

## API Reference

All endpoints are under the `/api` prefix and require an `Authorization: Bearer <token>` header. Unauthenticated requests receive HTTP 401.

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/status` | Server version, uptime, data dir, WAL LSN, buffer pool stats, table count |
| GET | `/api/roles` | List all roles from `pg_authid` |
| POST | `/api/roles` | Create a role |
| GET | `/api/roles/:name` | Get a single role by name |
| PUT | `/api/roles/:name` | Update a role (password, superuser flag, can-login flag) |
| DELETE | `/api/roles/:name` | Delete a role |
| GET | `/api/schemas` | List schemas (currently: `public`, `pg_catalog`) |
| GET | `/api/schemas/:schema/tables` | List table names in a schema |
| GET | `/api/tables/:schema/:table` | Get table column definitions |
| GET | `/api/tables/:schema/:table/indexes` | List B+ tree indexes on a table |
| GET | `/api/tables/:schema/:table/rowcount` | Get exact row count via `SELECT COUNT(*)` |
| DELETE | `/api/tables/:schema/:table` | Drop a table (`DROP TABLE IF EXISTS`) |
| POST | `/api/tables/:schema/:table/truncate` | Truncate a table (`DELETE FROM`) |
| POST | `/api/query` | Execute a SQL statement |
| GET | `/api/permissions` | Permissions info (stub) |

### Request and response formats

**POST /api/roles — create a role**

```json
{
  "name": "alice",
  "password": "s3cr3t",
  "rolsuper": false,
  "rolcreatedb": false,
  "rolcreaterole": false,
  "rolcanlogin": true
}
```

All fields except `name` are optional. `rolcanlogin` defaults to `true`. `rolsuper` defaults to `false`.

Response on success (HTTP 201):

```json
{
  "oid": 16389,
  "rolname": "alice",
  "rolsuper": false,
  "rolcanlogin": true
}
```

**PUT /api/roles/:name — update a role**

```json
{
  "password": "newpassword",
  "rolsuper": false,
  "rolcanlogin": true
}
```

All fields are optional. Only provided fields are updated. The server builds and executes `ALTER ROLE <name> PASSWORD '...' [SUPERUSER|NOSUPERUSER] [LOGIN|NOLOGIN]`.

**POST /api/query — execute SQL**

```json
{ "sql": "SELECT COUNT(*) FROM books" }
```

Response (HTTP 200 for both success and error):

```json
{
  "columns": ["COUNT(*)"],
  "rows": [[42]],
  "rows_affected": 0,
  "command": "SELECT",
  "error": null
}
```

On error, `columns` and `rows` are empty arrays and `error` contains the engine's error message.

### Scripting with curl

```bash
TOKEN=mysecrettoken
BASE=http://localhost:8080

# List roles
curl -s -H "Authorization: Bearer $TOKEN" $BASE/api/roles | jq .

# Create a role
curl -s -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name":"alice","password":"s3cr3t","rolsuper":false,"rolcanlogin":true}' \
  $BASE/api/roles

# Get server status
curl -s -H "Authorization: Bearer $TOKEN" $BASE/api/status | jq .

# List tables in the public schema
curl -s -H "Authorization: Bearer $TOKEN" $BASE/api/schemas/public/tables | jq .

# Get column definitions for a table
curl -s -H "Authorization: Bearer $TOKEN" $BASE/api/tables/public/books | jq .

# Run a query
curl -s -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"sql":"SELECT COUNT(*) FROM books"}' \
  $BASE/api/query | jq .

# Drop a table
curl -s -X DELETE \
  -H "Authorization: Bearer $TOKEN" \
  $BASE/api/tables/public/old_logs

# Truncate a table
curl -s -X POST \
  -H "Authorization: Bearer $TOKEN" \
  $BASE/api/tables/public/logs/truncate | jq .
```

---

## Security Considerations

**Choose a strong token.** The admin token is the sole authentication mechanism. Generate it with a cryptographically secure source:

```bash
openssl rand -hex 32
```

A 32-byte (64 hex character) random value is sufficient. Do not use dictionary words or short strings.

**Restrict network access.** The admin server binds to `0.0.0.0` — all interfaces. If you run it on a multi-homed host or a cloud VM, restrict access to localhost using a firewall rule before starting:

```bash
# Allow loopback only (Linux — adjust for your firewall)
iptables -A INPUT -p tcp --dport 8080 ! -i lo -j DROP
```

Do not expose the admin server to the public internet. There is no TLS support, no rate limiting, and no second authentication factor.

**Token in environment, not shell history.** Pass the token via the environment variable rather than `--admin-token` on the command line, to avoid it appearing in shell history and `ps` output:

```bash
export ICEDB_ADMIN_TOKEN=$(openssl rand -hex 32)
cargo run -p admin-server -- --data-dir ./data
```

**Confirmation for destructive actions.** The UI requires explicit confirmation before drop, truncate, and role delete operations. The REST API does not — scripted `DELETE` and `POST .../truncate` calls take effect immediately.

**Data directory permissions.** The admin server reads and writes the icedb data directory with the same access as the running process. Restrict filesystem permissions on the data directory so only the icedb process user can read or write it. See Chapter 7 and Chapter 11.

For a full discussion of role security and SCRAM-SHA-256 password storage, see Chapter 7.

---

## Limitations and Known Issues

| Limitation | Current State |
|-----------|---------------|
| Role attribute editing in the UI | No edit form; use the Query Console or CLI with `ALTER ROLE` |
| `rolcreatedb` / `rolcreaterole` independent of superuser | Not yet independently stored in the catalog; use CLI if needed |
| Buffer pool dirty frame count | Always reported as 0; dirty frame tracking is not exposed via the API |
| Table-level GRANT / REVOKE | Not yet implemented in the engine; the Permissions page is a stub |
| Running alongside icedb-server | Not recommended; file locking is advisory only — concurrent access produces undefined behavior |
| Query history persistence | Not preserved across page reloads |
| SQL syntax highlighting | Not implemented; CodeMirror 6 upgrade is planned |
| Multi-statement batch in query console | One statement per submit; use the CLI for multi-statement scripts |
| Schema support | Only `public` and `pg_catalog` are returned; user-defined schemas are not yet supported |
| Row count on large tables | `SELECT COUNT(*)` performs a full scan; may be slow on large tables |
