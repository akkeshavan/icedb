# icedb Admin UI — Technical Specification

**Document version**: 1.0
**Date**: 2026-03-17
**Status**: Draft — pending approval before implementation begins

---

## Table of Contents

1. [Goals and Non-Goals](#1-goals-and-non-goals)
2. [Feature Specification](#2-feature-specification)
3. [UI/UX Design Principles](#3-uiux-design-principles)
4. [Architecture](#4-architecture)
5. [Stack Options](#5-stack-options)
6. [Recommended Stack](#6-recommended-stack)
7. [API Endpoint Reference](#7-api-endpoint-reference)
8. [Security Considerations](#8-security-considerations)
9. [Implementation Phases](#9-implementation-phases)
10. [Files to Create](#10-files-to-create)

---

## 1. Goals and Non-Goals

### Goals

- Provide a **localhost web UI** for developers and operators to inspect and manage a running icedb instance without needing `psql` or the CLI.
- Cover the full lifecycle of **role and database management**: create, edit, delete roles with appropriate privilege flags.
- Expose **table and schema browsing**: list tables, view column definitions, view indexes, truncate, drop.
- Provide a **minimal query console** for ad-hoc SQL without leaving the browser.
- Surface **server internals**: WAL LSN, buffer pool stats, uptime, active connection count — the information an operator needs to verify the engine is healthy.
- Run with a single command (`cargo run -p admin-server`) and zero runtime configuration beyond an admin token.
- Authenticate via a **separate admin token** that is distinct from icedb roles, so the UI cannot be accessed even if database credentials are leaked.

### Non-Goals

- **Not a query IDE.** No query history persistence, no multiple tabs, no saved queries, no schema auto-complete. The query console is a debugging escape hatch, not a development environment.
- **Not for end users.** This tool is for the person running the icedb server, not for application users or business analysts.
- **Not a replication or backup UI.** Backup, point-in-time recovery, and replication topology management are out of scope.
- **Not publicly accessible.** The admin server binds to `127.0.0.1` only. There is no plan to expose it over a network.
- **Not a migration tool.** Schema migrations are out of scope. The UI can show schema state but will not manage migration files or history.
- **Not a monitoring dashboard.** Time-series graphing of metrics is out of scope. The server status page shows current point-in-time values only.
- **Not a GRANT/REVOKE IDE for table-level ACLs.** Table-level permissions UI is planned (Phase 4) but depends on the engine implementing a `pg_acl` catalog table. Until then it is displayed as "not yet tracked."

---

## 2. Feature Specification

### 2.1 User and Role Management

**Screen: Roles List** (`/roles`)

Displays a table with one row per role sourced from `pg_authid`. The admin API queries the `CatalogManager` directly (no SQL round-trip needed for listing).

| Column | Source field | Notes |
|---|---|---|
| Name | `rolname` | Clickable link to role detail |
| OID | `oid` | Shown in monospace |
| Superuser | `rolsuper` | Rendered as a badge (Yes / No) |
| Can Login | `rolcanlogin` | Badge |
| Create DB | `rolcreatedb` | Badge |
| Create Role | `rolcreaterole` | Badge |
| Bypass RLS | `rolbypassrls` | Badge |
| Password Set | derived from `rolpassword IS NOT NULL` | Never shows the SCRAM verifier |
| Actions | — | Edit / Delete buttons |

A "New Role" button opens the create role dialog.

**Dialog: Create Role**

Fields:
- Role name (text, required, validated: alphanumeric + underscore, no spaces)
- Password (password input, optional; if blank the role has no password)
- Confirm password (password input, shown only when password is non-empty)
- Flags: `Superuser`, `Can Login`, `Create DB`, `Create Role`, `Bypass RLS` — all boolean toggles, default off except `Can Login` which defaults on

Validation:
- Role name must be unique (checked client-side from the cached list, then enforced server-side).
- If Superuser is toggled on, `Create DB` and `Create Role` are automatically checked and disabled (matching icedb's `create_role` behavior in `catalog/manager.rs` where `rolcreaterole = rolsuper` and `rolcreatedb = rolsuper`).
- Passwords are never sent to the backend in plaintext over the network — the admin API accepts the plaintext password and calls `auth::scram::hash_password` server-side to produce the SCRAM-SHA-256 verifier before storing it.

On submit: `POST /api/roles` → success toast "Role created" → list refreshes.

**Dialog: Edit Role**

Pre-populated from role detail. Editable fields:
- New password (optional; leave blank to keep current password)
- Confirm new password
- All flag toggles

Non-editable: role name, OID.

Safety rule: If the role being edited is the last superuser, the Superuser toggle cannot be turned off. The UI disables the toggle and shows a tooltip: "Cannot remove superuser from the last superuser role."

On submit: `PATCH /api/roles/:name` → success toast "Role updated."

**Dialog: Delete Role**

Confirmation dialog with text: "Are you sure you want to delete role `<rolname>`? This action cannot be undone."

Safety rules enforced server-side (and surfaced in the UI):
- Cannot delete the last superuser.
- If the role is the owner of any table (checked via `pg_class.relowner` once that field is implemented), refuse with an informative error.

On confirm: `DELETE /api/roles/:name` → success toast "Role deleted" → list refreshes.

**View: Role Privileges Summary**

Within the role detail page, a read-only summary panel shows:
- Effective privileges as a human-readable sentence: "This role can log in, create databases, and create roles."
- If GRANT tracking is implemented: tables this role has been granted access to, with privilege type. If not yet implemented: a note "Table-level ACL tracking not yet available."

---

### 2.2 Database and Schema Management

**Screen: Databases / Schemas** (`/databases`)

icedb currently operates as a single-database engine — the `pg_namespace` catalog tracks schemas within that instance. The screen shows:

- **Instance info panel**: data directory path, server port, server version string ("16.0 (icedb)").
- **Schemas table**: one row per namespace from `pg_namespace`.

| Column | Source |
|---|---|
| Schema name | `nspname` |
| OID | `oid` |
| Owner | Resolved role name from `nspowner` |
| Table count | Count of `pg_class` rows with matching `relnamespace` and `relkind = 'r'` |

System schemas (`pg_catalog`) are shown in a separate "System Schemas" section, collapsed by default.

**Action: Create Schema**

Dialog with a single field: schema name. Owner defaults to the icedb superuser. Calls `POST /api/schemas`.

**Action: Drop Schema**

Available on user schemas only (not `pg_catalog` or `public`). Confirmation dialog requires the user to type the schema name before the delete button activates — identical UX to Heroku's "type the app name to confirm" pattern. Calls `DELETE /api/schemas/:name`.

Server-side check: refuse if the schema contains any tables (no `CASCADE` support in this UI).

**Database size display**: Calculated as `(total 8 kB pages across all heap files in the data directory) × 8192 bytes`, rendered as a human-readable string (e.g., "24.6 MB"). This is a filesystem-level count, not a SQL query. Surfaced as a field in the instance info panel.

---

### 2.3 Table Browser

**Screen: Tables List** (`/schemas/:schema/tables`)

Reached by clicking a schema in the databases screen. Shows all user tables in that schema.

| Column | Source |
|---|---|
| Table name | `relname` |
| OID | `oid` |
| Columns | `relnatts` |
| Est. rows | `reltuples` (shown as "~N rows"; 0 if ANALYZE has not been run) |
| Pages | `relpages` (converted to size: `relpages × 8 kB`) |
| Actions | Schema / Indexes / Truncate / Drop |

**View: Table Schema** (slide-out panel or sub-page)

Lists all columns from `pg_attribute` for the selected table:

| Column | Source |
|---|---|
| # | `attnum` |
| Name | `attname` |
| Type | `atttypid` resolved to type name (e.g., `int4`, `text`, `float8`, `bool`) |
| Not Null | `attnotnull` |
| Has Default | `atthasdef` |

Primary key column is highlighted if a PRIMARY KEY constraint is tracked (deferred until constraint catalog is implemented).

**View: Indexes on a Table**

Lists entries from the in-memory index registry (from `CatalogManager::get_index_path`). Displays:
- Index name (derived from file name pattern `idx_<table_oid>_<column_name>.btree`)
- Indexed column
- File path on disk
- File size in bytes

Note: The current index registry is in-memory only and is not persisted to disk as a catalog table. The UI documents this limitation inline: "Index list reflects in-memory registry only; may be incomplete after server restart until indexes are rebuilt."

**Action: Drop Table**

Confirmation dialog: user must type the table name to enable the confirm button. Calls `DELETE /api/schemas/:schema/tables/:table`. Server-side this calls `CatalogManager::drop_table`.

**Action: Truncate Table**

Confirmation dialog: "This will delete all rows from `<table>`. The table structure will be preserved." Button text: "Truncate". Calls `POST /api/schemas/:schema/tables/:table/truncate`. Server-side this executes `DELETE FROM <table>` via the `QueryEngine` (a full table scan delete — a proper TRUNCATE command is a future engine feature).

---

### 2.4 Query Console

**Screen: Query Console** (`/query`)

A focused, minimal SQL execution environment.

Layout:
- Top half: SQL editor (CodeMirror 6 with basic SQL syntax highlighting, no autocomplete)
- Bottom half: Results area (table or error panel)
- Toolbar: "Run" button (keyboard shortcut: `Ctrl+Enter` / `Cmd+Enter`), "Clear" button, execution time display

Behavior:
- Sends the full text of the editor as a single query to `POST /api/query`.
- On success: renders results as a scrollable table. Column headers are the field names from the result schema. Null values are rendered as `NULL` in italics.
- On error: shows a red error panel with the error message and SQLSTATE code (e.g., `42P01 — relation "foo" does not exist`).
- Results are limited to 1000 rows. If the result is truncated, a notice bar is shown: "Showing first 1,000 rows. Use LIMIT to control result size."
- No query history persistence. The editor is reset on page refresh.
- No multi-statement execution — if the SQL contains multiple statements separated by `;`, only the last statement's result is returned (matching `QueryEngine::execute` behavior).

Constraints (intentional):
- No saved queries.
- No export to CSV.
- No parameter binding UI.
- No EXPLAIN visualization.

These are all intentionally excluded to keep this a debugging console, not an IDE.

---

### 2.5 Connection and Server Status

**Screen: Server Status** (`/status`)

Two panels: Instance Info and Engine Internals.

**Instance Info panel**

| Field | Source |
|---|---|
| Server version | `"16.0 (icedb)"` (from `IceDbStartupHandler::new` — `provider.server_version`) |
| Listening address | From admin server config (the icedb port, e.g., `0.0.0.0:5432`) |
| Data directory | From admin server config |
| Admin UI uptime | Calculated from admin server start time |
| Active connections | Count of active pgwire client connections (requires a connection counter in `network/`) |

**WAL Status panel**

| Field | Source |
|---|---|
| Current LSN | `WalWriter::current_lsn()` — the highest LSN written |
| Last checkpoint LSN | `WalWriter::last_checkpoint_lsn()` — the LSN of the last checkpoint record |
| WAL segment files | Count of `.wal` segment files in the data directory |
| WAL size on disk | Total bytes of WAL segment files |

Note: These fields require exposing read accessors on `WalWriter`. If not yet implemented, the panel shows "Not available — WAL metrics API not yet exposed."

**Buffer Pool Stats panel**

| Field | Source |
|---|---|
| Total frames | Fixed pool size (from `BufferPool` configuration) |
| Dirty frames | Count of frames with dirty bit set |
| Pinned frames | Count of frames with pin count > 0 |
| Eviction count (lifetime) | Total frames evicted since server start |

Note: These fields require exposing a stats struct from `storage::BufferPool`. If not yet implemented, the panel shows "Not available — buffer pool stats API not yet exposed."

The status page auto-refreshes every 10 seconds. A "Refresh now" button is available.

---

### 2.6 Permissions / RBAC

**Screen: Permissions** (`/permissions`)

Displays a matrix of table-level privileges.

**Current engine state**: icedb's `QueryEngine::check_privileges` (in `crates/sql/src/engine.rs`) enforces only role-level flags (`rolsuper`, `rolcreatedb`, `rolcreaterole`, `rolcanlogin`). There is no `pg_acl` catalog table tracking table-level `GRANT`/`REVOKE`.

Therefore, in the current implementation:

- The permissions screen shows a notice: "Table-level ACL tracking (GRANT/REVOKE per table per role) is not yet implemented in the icedb engine. This screen shows role-level capabilities only."
- It then renders the role-level capability matrix:

| Role | Login | Create DB | Create Role | Superuser |
|---|---|---|---|---|
| icedb    | Yes | Yes | Yes | Yes |
| app_user | Yes | No | No | No |

**Planned (Phase 4 of admin UI — after engine support is added)**:

Once a `pg_acl` table or equivalent is implemented in the engine:
- The screen will show a table × role privilege matrix.
- Each cell will show which of `SELECT`, `INSERT`, `UPDATE`, `DELETE` the role holds on that table.
- Grant/revoke will be done via toggle checkboxes that call `POST /api/permissions/grant` and `POST /api/permissions/revoke`.

---

## 3. UI/UX Design Principles

### Navigation

A fixed left sidebar with five entries:
1. Databases (icon: database cylinder)
2. Tables (icon: grid)
3. Roles (icon: person with key)
4. Query Console (icon: terminal prompt)
5. Server Status (icon: heartbeat / pulse)

The sidebar is always visible. The active section is highlighted. On screens narrower than 768 px the sidebar collapses to icon-only mode (no labels).

### Color and Theme

- Ships with both dark and light themes.
- Theme is toggled via a button in the top-right corner of the sidebar.
- Default theme follows the OS preference (`prefers-color-scheme` media query).
- Theme preference is persisted in `localStorage`.
- Color palette uses semantic tokens so dark/light variants are a single CSS variable swap:
  - `--color-bg-base`, `--color-bg-surface`, `--color-bg-subtle`
  - `--color-text-primary`, `--color-text-secondary`, `--color-text-muted`
  - `--color-border`, `--color-accent`, `--color-danger`, `--color-success`

### Destructive Actions

All destructive actions (drop table, drop schema, delete role, truncate table) follow the same pattern:
1. User clicks a red "Delete" or "Drop" button.
2. A modal dialog appears explaining the action and its consequences.
3. For irreversible data deletion (drop table, truncate), the user must type the exact name of the object into a confirmation input before the confirm button activates.
4. The confirm button is red and labeled "Delete `<name>`" or "Drop `<name>`".
5. The cancel button is secondary and labeled "Cancel".

### Toast Notifications

- Success: green toast, bottom-right, auto-dismisses after 4 seconds. Example: "Role `app_user` created."
- Error: red toast, bottom-right, persists until dismissed. Includes error message and optionally SQLSTATE.
- In-progress: spinner overlay on the triggering button during async operations. Buttons are disabled while a request is in flight.

### Tables

- All data tables support client-side column sorting by clicking column headers.
- Tables show a loading skeleton while data is being fetched.
- Empty states show a friendly message and a call-to-action: e.g., "No tables in this schema. Create one using SQL in the Query Console."
- Tables do not paginate client-side for lists that are expected to be small (roles, schemas, tables per schema). For the query console results table, virtual scrolling is used for rows beyond 500.

### Responsiveness

- Desktop-first. The minimum supported viewport width is 1024 px.
- The layout does not reflow to a single-column mobile layout. This is an operator tool expected to be used on a workstation.

---

## 4. Architecture

### 4.1 Backend — Admin API Server

**Process model**: The admin server runs as a separate OS process from the icedb server. It is a new binary in the workspace: `crates/admin-server/src/main.rs`.

**Engine access**: The admin server does NOT connect to icedb over the PostgreSQL wire protocol (port 5432). Instead, it links directly against the `sql`, `catalog`, `wal`, `storage`, and `txn` crates and instantiates the same shared data structures as the main server. Both the icedb server and the admin server open the same `--data-dir` on disk; they do not share in-memory state. This avoids a TCP round-trip for every admin operation and allows introspecting internal state (buffer pool, WAL writer) that is not exposed over SQL.

Consequence: the admin server must open the data directory in a read-compatible way. Writes (create role, drop table, etc.) go through `CatalogManager` and `WalWriter` exactly as they would from the main server. This requires appropriate file locking to prevent corruption when both processes write simultaneously. A file advisory lock on the WAL directory will be used (the admin server acquires a shared lock; if a write is needed it upgrades to exclusive). This is a simplification; a future revision may route write operations through the PostgreSQL wire protocol to the main server instead.

**Authentication**: A single static admin token. On startup, the admin server reads `ICEDB_ADMIN_TOKEN` from the environment (or from a config file at `--config`). Every API request must include `Authorization: Bearer <token>`. No token, no access.

**HTTP framework**: Axum.

**Binding**: `127.0.0.1:8080` by default. Configurable via `--admin-port`.

**Frontend serving**: In release mode the admin server serves the compiled frontend SPA from an embedded directory (using `include_dir` or `rust-embed`). In development mode the frontend runs on `http://localhost:5173` (Vite dev server) and the admin server has CORS configured to allow that origin only.

### 4.2 Frontend — SPA

**Framework**: React 18 + TypeScript.
**Build tool**: Vite.
**HTTP client**: TanStack Query (React Query) v5 for data fetching, caching, and background refresh.
**Component library**: shadcn/ui (built on Radix UI primitives) — unstyled accessible components styled with Tailwind CSS.
**SQL editor**: CodeMirror 6 with `@codemirror/lang-sql` for syntax highlighting.
**Routing**: React Router v6.

**Dev URL**: `http://localhost:5173`
**Production URL**: `http://localhost:8080/admin` (served by the Rust binary)

The token is stored in `sessionStorage` (not `localStorage`) so it is cleared when the browser tab is closed. On first load, if no token is found, the user is redirected to a login page that accepts the admin token via a password input.

---

## 5. Stack Options

### Option A: Rust backend (Axum) + React + TypeScript frontend

**Backend**: Axum HTTP server in a new crate `crates/admin-server`. Links directly to `sql`, `catalog`, `wal`, `storage`, `txn` crates. Serves a JSON REST API. In release mode, serves the compiled frontend from an embedded asset directory.

**Frontend**: React 18 + TypeScript + Vite + TanStack Query + shadcn/ui + Tailwind CSS. Separate `admin-ui/` directory at the workspace root. Built with `npm run build` before `cargo build` in CI.

| Dimension | Assessment |
|---|---|
| Complexity | Medium. Two separate build steps (cargo + npm). Frontend and backend are decoupled and can evolve independently. |
| Maintenance burden | Medium. The team must maintain TypeScript knowledge alongside Rust. Node/npm version drift is a long-term concern. |
| Dev experience | Good. Vite hot-reload for the frontend. Axum is familiar territory for the team. TanStack Query gives excellent data-fetching ergonomics. |
| Deployment | Two artifacts: the Rust binary (with embedded frontend) and optionally a standalone frontend build. Single `cargo run -p admin-server` works after `npm run build`. |
| Who it suits | Teams that want the best possible frontend DX and are comfortable with a split toolchain. |

**Pros**:
- Maximum frontend flexibility — can add charts, virtual scrolling, CodeMirror, etc. trivially.
- shadcn/ui provides a polished, accessible component set at no maintenance cost.
- TypeScript type safety catches API contract mismatches at compile time (especially with a generated OpenAPI client).
- Largest ecosystem: easiest to find examples, libraries, and answers.

**Cons**:
- Requires Node.js and npm in the development environment.
- Two build steps in CI. Build times are longer.
- Frontend and backend types must be kept in sync manually (or via code generation).
- Bundle size is larger than server-rendered HTML (~200-400 kB gzipped for a non-trivial SPA).

---

### Option B: Rust backend (Axum) + HTMX + Askama templates

**Backend**: Axum + Askama (Jinja2-style templates compiled into the binary at build time) + HTMX for partial page updates without a full SPA. The backend renders HTML directly; HTMX swaps fragments on user interaction.

**Frontend**: No separate build step. CSS via a CDN-served Tailwind CSS Play CDN (for development) or a single pre-built CSS file checked in. HTMX loaded from CDN or embedded as a single JS file.

| Dimension | Assessment |
|---|---|
| Complexity | Low. No npm, no bundler, no TypeScript. One `cargo build` produces the entire artifact including all HTML/CSS/JS. |
| Maintenance burden | Low. No frontend dependency tree to update. Askama templates are checked by the Rust compiler. |
| Dev experience | Acceptable. No hot-reload for templates by default (requires `cargo-watch`). No TypeScript. HTMX is simple but less expressive than React for complex UI states. |
| Deployment | Single Rust binary. Zero extra toolchain requirements beyond Rust itself. |
| Who it suits | Teams that want the simplest possible deployment and are comfortable with server-rendered HTML patterns. |

**Pros**:
- Zero npm. `cargo run -p admin-server` works out of the box on any machine with Rust installed.
- Smallest runtime footprint. HTML is rendered server-side; the browser receives minimal JS.
- Template correctness is checked at Rust compile time by Askama.
- No frontend/backend type drift — the Rust structs are the source of truth for both API and template data.
- Easier to embed into the main server binary if desired.

**Cons**:
- HTMX is powerful but awkward for complex client-side state (e.g., confirmation dialogs that depend on client-side field values, like the "type the name" confirmation pattern).
- CodeMirror 6 (SQL editor) is a JavaScript library that must be integrated as a custom HTMX extension or alongside vanilla JS. This is doable but adds friction.
- The query console results table is harder to make interactive (virtual scrolling, column sorting) without adding more JS.
- Less familiar to developers who know React/Vue ecosystems.
- Askama template debugging is less ergonomic than browser DevTools inspecting React component trees.

---

### Option C: Standalone Tauri desktop application

**Backend**: Tauri Rust process. The Tauri backend uses the icedb `sql`/`catalog`/`wal`/`storage`/`txn` crates directly (same as Option A's backend).

**Frontend**: React + TypeScript bundled by Vite, wrapped by Tauri in a native WebView (WebKit on macOS, WebView2 on Windows, WebKitGTK on Linux).

**Distribution**: Native desktop app: `.dmg` on macOS, `.msi`/`.exe` on Windows, `.AppImage`/`.deb` on Linux. Distributed via GitHub Releases.

| Dimension | Assessment |
|---|---|
| Complexity | High. Tauri introduces a native build step per platform. macOS code signing and notarization are required for distribution. CI matrix must cover macOS, Windows, Linux. |
| Maintenance burden | High. Tauri API surface evolves. Platform-specific bugs (WebView rendering differences). Cross-compilation is non-trivial. |
| Dev experience | Good for the frontend (Vite hot-reload inside Tauri). Rust backend code is the same as Option A. |
| Deployment | No web server needed. App is installed locally. But the icedb server still runs as a separate process — Tauri is the UI, not the database engine. |
| Who it suits | Teams that want to distribute a polished packaged app to ops teams that cannot or will not run `cargo run`. |

**Pros**:
- Native app feel (window, dock icon, menu bar integration on macOS).
- No port conflicts — no HTTP server needed for the admin UI itself.
- Can be bundled and distributed independently of the icedb server binary.
- Auto-update support via Tauri updater.

**Cons**:
- Overkill for a localhost developer tool. The target user is already running `cargo run` to start icedb.
- Tauri requires platform-specific build environments. CI is significantly more complex.
- The data directory access model (Tauri process opening icedb heap files) introduces the same file-locking concerns as Option A, without any reduction in complexity.
- Platform support for the Tauri WebView is an external dependency (WebKit version on Linux, WebView2 on Windows).
- Does not help the "just run it" goal — installing a native app is arguably more friction than `cargo run -p admin-server`.

---

## 6. Recommended Stack

**Recommendation: Option A — Axum backend + React + TypeScript frontend.**

Rationale:

1. **Feature requirements demand a capable frontend.** The query console requires CodeMirror 6 (a JavaScript library). The confirmation dialogs require client-side state. The results table needs virtual scrolling for large query results. These are all significantly easier to build in React than in HTMX + vanilla JS.

2. **The team already knows Rust.** The Axum backend is straightforward — it is structurally identical to the `network/` crate already in the codebase, minus the pgwire layer. There is no new Rust knowledge required.

3. **npm friction is minimal.** The goal "easy to run with `cargo run -p admin-server`" is fully met: after an initial `npm install && npm run build` in `admin-ui/`, the compiled assets are embedded into the Rust binary via `rust-embed`. Subsequent `cargo run` calls do not require Node. A `build.rs` script in the `admin-server` crate can invoke `npm run build` automatically during `cargo build` when the frontend source changes, so the team only needs Node installed, not actively managed.

4. **Option B is appealing for simplicity, but HTMX falls short on the query console.** A textarea + HTMX partial swap is not sufficient for a usable SQL editor. Adding CodeMirror 6 alongside HTMX creates a hybrid JS architecture that is harder to maintain than a clean React SPA.

5. **Option C is disproportionate for the scope.** Tauri is the right choice for a distributed desktop application. This admin UI is a developer-facing localhost tool. The added CI complexity and platform-specific build requirements are not justified.

**Concrete setup summary**:
- `crates/admin-server/` — Axum HTTP server, Cargo crate
- `admin-ui/` — React + TypeScript + Vite project (`npm create vite@latest`)
- `cargo build -p admin-server` triggers `npm run build` via `build.rs`, embeds the compiled assets
- `cargo run -p admin-server -- --data-dir ./data --admin-port 8080` — single command to run
- Development: run `cargo run -p admin-server` and `cd admin-ui && npm run dev` simultaneously; the Vite dev server proxies API calls to `localhost:8080`

---

## 7. API Endpoint Reference

All endpoints are under the `/api` prefix. All requests and responses use `application/json`. All endpoints except `POST /api/auth/verify` require `Authorization: Bearer <token>`.

### 7.1 Authentication

#### `POST /api/auth/verify`

Verify that the provided admin token is valid. Used by the frontend login page.

Request body:
```json
{ "token": "string" }
```

Response `200 OK`:
```json
{ "valid": true }
```

Response `401 Unauthorized`:
```json
{ "error": "invalid_token", "message": "Invalid admin token" }
```

---

### 7.2 Server Status

#### `GET /api/status`

Returns current server state.

Response `200 OK`:
```json
{
  "server_version": "16.0 (icedb)",
  "data_dir": "/path/to/data",
  "icedb_port": 5432,
  "admin_uptime_seconds": 3842,
  "active_connections": 2,
  "wal": {
    "current_lsn": 1048576,
    "last_checkpoint_lsn": 786432,
    "segment_count": 3,
    "total_bytes": 50331648
  },
  "buffer_pool": {
    "total_frames": 1024,
    "dirty_frames": 12,
    "pinned_frames": 0,
    "eviction_count": 4821
  },
  "disk_usage_bytes": 25165824
}
```

Fields that are not yet available from the engine are returned as `null`.

---

### 7.3 Roles

#### `GET /api/roles`

List all roles.

Response `200 OK`:
```json
{
  "roles": [
    {
      "oid": 10,
      "rolname": "icedb",
      "rolsuper": true,
      "rolinherit": true,
      "rolcreaterole": true,
      "rolcreatedb": true,
      "rolcanlogin": true,
      "rolbypassrls": true,
      "password_set": true
    }
  ]
}
```

Note: `rolpassword` (the SCRAM verifier) is never included in any API response.

---

#### `GET /api/roles/:name`

Get a single role by name.

Response `200 OK`: same shape as a single element of the `roles` array above.

Response `404 Not Found`:
```json
{ "error": "role_not_found", "message": "Role 'foo' does not exist" }
```

---

#### `POST /api/roles`

Create a new role.

Request body:
```json
{
  "rolname": "string",
  "password": "string | null",
  "rolsuper": false,
  "rolcanlogin": true,
  "rolcreatedb": false,
  "rolcreaterole": false,
  "rolbypassrls": false
}
```

Response `201 Created`:
```json
{ "oid": 16384, "rolname": "app_user" }
```

Error codes:
- `409 Conflict` — role already exists: `{ "error": "duplicate_role" }`
- `400 Bad Request` — invalid role name format: `{ "error": "invalid_name" }`

---

#### `PATCH /api/roles/:name`

Update an existing role. All fields except `rolname` are optional; only provided fields are updated.

Request body:
```json
{
  "password": "string | null",
  "rolsuper": "bool | null",
  "rolcanlogin": "bool | null",
  "rolcreatedb": "bool | null",
  "rolcreaterole": "bool | null",
  "rolbypassrls": "bool | null"
}
```

Response `200 OK`: updated role object.

Error codes:
- `404 Not Found` — role does not exist
- `409 Conflict` — would remove superuser from last superuser: `{ "error": "last_superuser" }`

---

#### `DELETE /api/roles/:name`

Delete a role.

Response `204 No Content` on success.

Error codes:
- `404 Not Found` — role does not exist
- `409 Conflict` — cannot delete last superuser: `{ "error": "last_superuser" }`

---

### 7.4 Schemas

#### `GET /api/schemas`

List all namespaces (schemas).

Response `200 OK`:
```json
{
  "schemas": [
    {
      "oid": 2200,
      "nspname": "public",
      "nspowner": 10,
      "nspowner_name": "icedb",
      "table_count": 5
    },
    {
      "oid": 11,
      "nspname": "pg_catalog",
      "nspowner": 10,
      "nspowner_name": "icedb",
      "table_count": 4,
      "system": true
    }
  ]
}
```

---

#### `POST /api/schemas`

Create a new schema.

Request body:
```json
{ "nspname": "string", "owner": "string" }
```

Response `201 Created`:
```json
{ "oid": 16385, "nspname": "analytics" }
```

Error codes:
- `409 Conflict` — schema already exists
- `400 Bad Request` — invalid name

---

#### `DELETE /api/schemas/:name`

Drop a schema. Refuses if the schema contains any tables.

Response `204 No Content` on success.

Error codes:
- `404 Not Found`
- `409 Conflict` — schema is not empty: `{ "error": "schema_not_empty", "table_count": 3 }`
- `403 Forbidden` — attempt to drop a system schema (`pg_catalog`): `{ "error": "system_schema" }`

---

### 7.5 Tables

#### `GET /api/schemas/:schema/tables`

List all tables in a schema.

Response `200 OK`:
```json
{
  "schema": "public",
  "tables": [
    {
      "oid": 16390,
      "relname": "orders",
      "relnatts": 5,
      "relpages": 120,
      "reltuples": 50000.0,
      "size_bytes": 983040
    }
  ]
}
```

---

#### `GET /api/schemas/:schema/tables/:table`

Get table detail including column definitions and indexes.

Response `200 OK`:
```json
{
  "oid": 16390,
  "relname": "orders",
  "schema": "public",
  "columns": [
    {
      "attnum": 1,
      "attname": "id",
      "type_name": "int4",
      "atttypid": 23,
      "attnotnull": true,
      "atthasdef": false
    }
  ],
  "indexes": [
    {
      "column": "id",
      "file_path": "/data/idx_16390_id.btree",
      "size_bytes": 16384
    }
  ],
  "relpages": 120,
  "reltuples": 50000.0,
  "size_bytes": 983040
}
```

---

#### `DELETE /api/schemas/:schema/tables/:table`

Drop a table. Equivalent to `DROP TABLE`.

Response `204 No Content` on success.

Error codes:
- `404 Not Found`

---

#### `POST /api/schemas/:schema/tables/:table/truncate`

Truncate a table (delete all rows, keep schema).

Response `200 OK`:
```json
{ "rows_deleted": 50000 }
```

Error codes:
- `404 Not Found`

---

### 7.6 Query Console

#### `POST /api/query`

Execute a SQL query. Always runs under `ReadCommitted` isolation, auto-committed.

Request body:
```json
{
  "sql": "SELECT * FROM orders WHERE id = 1"
}
```

Response `200 OK` (query returned rows):
```json
{
  "command": "SELECT",
  "rows_affected": 0,
  "execution_ms": 2,
  "truncated": false,
  "columns": ["id", "status", "amount"],
  "rows": [
    [1, "paid", 99.99]
  ]
}
```

Response `200 OK` (DML, no rows):
```json
{
  "command": "INSERT",
  "rows_affected": 1,
  "execution_ms": 1,
  "truncated": false,
  "columns": [],
  "rows": []
}
```

Response `400 Bad Request` (SQL error):
```json
{
  "error": "sql_error",
  "sqlstate": "42P01",
  "message": "relation \"nonexistent\" does not exist"
}
```

Row values are JSON primitives. Null maps to JSON `null`. Integers map to JSON numbers. Floats map to JSON numbers. Booleans map to JSON booleans. Text maps to JSON strings. Results are capped at 1000 rows server-side; if the result had more, `"truncated": true` is set.

---

### 7.7 Permissions

#### `GET /api/permissions`

Returns role-level capability summary and, if available, table-level ACLs.

Response `200 OK`:
```json
{
  "acl_tracking_available": false,
  "role_capabilities": [
    {
      "rolname": "icedb",
      "rolsuper": true,
      "rolcanlogin": true,
      "rolcreatedb": true,
      "rolcreaterole": true
    }
  ],
  "table_acls": null
}
```

When `acl_tracking_available` is `true` (future), `table_acls` will be populated:
```json
{
  "table_acls": [
    {
      "schema": "public",
      "table": "orders",
      "grants": [
        { "rolname": "app_user", "privileges": ["SELECT", "INSERT"] }
      ]
    }
  ]
}
```

---

#### `POST /api/permissions/grant` _(planned — not in Phase 1)_

Grant table-level privilege. Available once `pg_acl` is implemented in the engine.

Request body:
```json
{
  "rolname": "app_user",
  "schema": "public",
  "table": "orders",
  "privilege": "SELECT"
}
```

Response `200 OK`: `{ "granted": true }`

---

#### `POST /api/permissions/revoke` _(planned — not in Phase 1)_

Revoke table-level privilege.

Request body: same shape as grant.

Response `200 OK`: `{ "revoked": true }`

---

## 8. Security Considerations

### Admin Token

- The admin token is a static secret provided via the `ICEDB_ADMIN_TOKEN` environment variable or a config file.
- Minimum length enforced on startup: 32 characters. The server refuses to start with a shorter token.
- The token is compared using a constant-time equality function to prevent timing attacks.
- The token is stored in the browser's `sessionStorage` (cleared on tab close), not `localStorage`.
- The login page accepts the token via a `<input type="password">` field. It is never logged.

### CORS

- In development mode, CORS allows `http://localhost:5173` only.
- In production mode (serving the embedded frontend), CORS is disabled — all requests come from the same origin.
- The `Access-Control-Allow-Origin` header is never set to `*`.

### Binding Address

- The admin server binds to `127.0.0.1` by default, never `0.0.0.0`.
- If an operator changes `--admin-bind` to `0.0.0.0`, a startup warning is logged: "WARNING: Admin server is listening on all interfaces. This is not recommended for production use."

### Rate Limiting

- `POST /api/auth/verify` is rate-limited to 5 requests per 10 seconds per IP address using a token bucket in memory.
- All other endpoints are rate-limited to 100 requests per second per IP. This is a safety measure, not a performance constraint.

### HTTPS

- The admin server does not implement TLS. It is expected to run on localhost where TLS is unnecessary.
- If an operator needs TLS (e.g., accessing the UI over SSH tunnel), they should use a reverse proxy (nginx, Caddy).

### Audit Log

- All mutating operations (create role, delete role, drop table, truncate table, execute query via console) are written to an append-only audit log file at `<data-dir>/admin-audit.log`.
- Each log entry is a JSON object on one line:
  ```json
  {"timestamp":"2026-03-17T14:23:01Z","action":"delete_role","target":"app_user","outcome":"success"}
  ```
- The audit log is never truncated by the admin server. Log rotation is the operator's responsibility.
- Query console entries include the SQL text:
  ```json
  {"timestamp":"2026-03-17T14:25:12Z","action":"query","sql":"DELETE FROM orders","outcome":"success","rows_affected":50000}
  ```

### Password Handling

- Passwords submitted to `POST /api/roles` or `PATCH /api/roles/:name` are accepted in plaintext over the local loopback interface and immediately hashed with `auth::scram::hash_password` (SCRAM-SHA-256, 4096 iterations, 16-byte random salt) before being passed to `CatalogManager::create_role`.
- The plaintext password is never written to disk, logs, or audit entries.
- The SCRAM verifier stored in `pg_authid` is never returned in any API response.

### Data Directory Access

- The admin server opens the icedb data directory directly. It must be started by a user with read/write access to that directory.
- The admin server does not implement additional access control beyond the admin token — anyone who can reach `localhost:8080` with the token can read and modify all database metadata.

---

## 9. Implementation Phases

### Phase 1: Backend Skeleton + Role Management + Schema List

**Deliverables**:
- `crates/admin-server/` Cargo crate with Axum
- Admin token authentication middleware
- `GET /api/status` (partial — no WAL/buffer stats yet)
- `GET /api/roles`, `POST /api/roles`, `PATCH /api/roles/:name`, `DELETE /api/roles/:name`
- `GET /api/schemas`
- React SPA scaffold with sidebar navigation and login page
- Roles list and create/edit/delete dialogs
- Schemas list screen (read-only)
- Audit log file setup

**Gate**: `cargo run -p admin-server` starts. Frontend login works. Roles can be created, edited, and deleted via the UI. Schemas are listed.

---

### Phase 2: Table Browser + Query Console

**Deliverables**:
- `GET /api/schemas/:schema/tables`
- `GET /api/schemas/:schema/tables/:table`
- `DELETE /api/schemas/:schema/tables/:table`
- `POST /api/schemas/:schema/tables/:table/truncate`
- `POST /api/query`
- Table browser screen (list, schema view, indexes view)
- Drop table + truncate table dialogs
- Query console screen with CodeMirror 6 editor and results table

**Gate**: Tables can be browsed, dropped, and truncated via the UI. SQL queries execute and results render correctly including error states.

---

### Phase 3: Full Server Status + WAL/Buffer Stats

**Deliverables**:
- Expose `WalWriter::current_lsn()`, `WalWriter::last_checkpoint_lsn()` as public methods on the `wal` crate
- Expose a `BufferPoolStats` struct from the `storage` crate
- `GET /api/status` fully populated
- Server status screen with auto-refresh
- Disk usage calculation

**Gate**: Server status screen shows live WAL LSN, buffer pool dirty/pinned frame counts, and disk usage.

---

### Phase 4: Permissions UI

**Deliverables**:
- `GET /api/permissions` (role-level capabilities, table ACLs if available)
- Permissions screen showing role capability matrix
- If engine has `pg_acl` by this point: grant/revoke UI
- `POST /api/permissions/grant` and `POST /api/permissions/revoke` (conditional on engine support)

**Gate**: Permissions screen renders correctly. If ACL tracking is not in the engine, the "not yet tracked" notice is shown. If it is, grant/revoke works.

---

### Phase 5: Polish, Dark Mode, Schema Management, Packaging

**Deliverables**:
- `POST /api/schemas` and `DELETE /api/schemas/:name`
- Create schema dialog, drop schema dialog
- Dark/light mode toggle with `localStorage` persistence
- Toast notification system
- Virtual scrolling for query console results table
- `build.rs` in `admin-server` that auto-runs `npm run build` when frontend source changes
- `rust-embed` integration to bundle frontend assets into the binary
- README section: "Running the Admin UI"

**Gate**: `cargo build --release -p admin-server` produces a single self-contained binary that serves the full admin UI. Both dark and light modes work. All confirmation dialogs follow the spec.

---

## 10. Files to Create

When approved, the following files and directories will be created. No existing crate files will be modified except `Cargo.toml` (workspace manifest) to add the new crate.

```
icedb/
├── Cargo.toml                          # Add admin-server to [workspace.members]
│
├── crates/
│   └── admin-server/
│       ├── Cargo.toml                  # Axum, serde, rust-embed, tokio, tower-http deps
│       ├── build.rs                    # Runs `npm run build` if admin-ui/ source changed
│       └── src/
│           ├── main.rs                 # Startup: parse args, init engine, bind Axum
│           ├── config.rs               # AdminConfig struct: port, data_dir, token, bind addr
│           ├── auth.rs                 # Bearer token middleware (constant-time compare)
│           ├── audit.rs                # Audit log writer (append-only JSON lines)
│           ├── error.rs                # AdminError enum → Axum IntoResponse
│           ├── assets.rs               # rust-embed asset server for compiled frontend
│           ├── routes/
│           │   ├── mod.rs              # Axum Router assembly
│           │   ├── status.rs           # GET /api/status
│           │   ├── roles.rs            # GET/POST/PATCH/DELETE /api/roles[/:name]
│           │   ├── schemas.rs          # GET/POST/DELETE /api/schemas[/:name]
│           │   ├── tables.rs           # GET/DELETE/truncate /api/schemas/:s/tables[/:t]
│           │   ├── query.rs            # POST /api/query
│           │   └── permissions.rs      # GET/POST /api/permissions[/grant|revoke]
│           └── engine.rs               # AdminEngine: wraps CatalogManager + QueryEngine
│                                       # for admin-specific operations (last-superuser check, etc.)
│
└── admin-ui/
    ├── package.json                    # React, Vite, TanStack Query, shadcn, Tailwind, CodeMirror
    ├── tsconfig.json
    ├── vite.config.ts                  # Proxy /api → localhost:8080 in dev mode
    ├── tailwind.config.ts
    ├── index.html
    └── src/
        ├── main.tsx                    # React entry point
        ├── App.tsx                     # Router + layout
        ├── api/
        │   ├── client.ts               # Axios/fetch wrapper with Bearer token injection
        │   ├── roles.ts                # TanStack Query hooks for /api/roles
        │   ├── schemas.ts              # TanStack Query hooks for /api/schemas
        │   ├── tables.ts               # TanStack Query hooks for /api/schemas/:s/tables
        │   ├── query.ts                # TanStack Query mutation for /api/query
        │   ├── status.ts               # TanStack Query hook for /api/status
        │   └── permissions.ts          # TanStack Query hooks for /api/permissions
        ├── components/
        │   ├── Sidebar.tsx             # Fixed left nav
        │   ├── ConfirmDialog.tsx       # Reusable destructive action dialog
        │   ├── ConfirmNameDialog.tsx   # Destructive dialog requiring name re-entry
        │   ├── Toast.tsx               # Toast notification system
        │   ├── DataTable.tsx           # Sortable data table with loading skeleton
        │   └── Badge.tsx               # Yes/No flag badges
        ├── pages/
        │   ├── LoginPage.tsx           # Admin token entry
        │   ├── RolesPage.tsx           # Roles list + create/edit/delete dialogs
        │   ├── DatabasesPage.tsx       # Schemas list
        │   ├── TablesPage.tsx          # Tables list for a schema
        │   ├── TableDetailPage.tsx     # Column schema + indexes
        │   ├── QueryConsolePage.tsx    # CodeMirror + results table
        │   ├── StatusPage.tsx          # Server status + WAL + buffer pool
        │   └── PermissionsPage.tsx     # Role capability matrix + ACL matrix
        ├── hooks/
        │   ├── useTheme.ts             # Dark/light mode toggle + localStorage
        │   └── useAdminToken.ts        # sessionStorage token management
        └── lib/
            ├── formatters.ts           # Byte size, LSN, row count formatters
            └── constants.ts            # API base URL, result row limit, etc.
```

**Total new files**: approximately 45 source files across the Rust crate and the React project. No existing crate source files are modified. The workspace `Cargo.toml` gains one member entry.
