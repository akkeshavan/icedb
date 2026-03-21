# Chapter 7: Authentication & Security

**In this chapter:**
- Role-based access control (RBAC) and privilege flags
- Creating roles with `CREATE ROLE`
- Table-level privileges: `GRANT` and `REVOKE`
- SCRAM-SHA-256 password storage format and PBKDF2 iteration count
- Password verification at login (including legacy plaintext fallback)
- Network-level authentication and the cleartext transport note
- Security recommendations for production

## Role-Based Access Control

icedb uses a role model identical in structure to PostgreSQL's. Every connection authenticates as a role. Roles have privilege flags that determine what they can do. There are no separate "user" and "group" concepts — a role can be either (or both), controlled by the `rolcanlogin` flag.

Roles are stored in the `pg_authid` system catalog table. On first startup, icedb bootstraps a single role:

| Role | Privileges |
|------|-----------|
| `icedb` | superuser, can login, no password required |

The `icedb` superuser can connect without a password when using the CLI's embedded mode (`--data-dir` flag), because the CLI does not perform authentication — it accesses the engine directly.

## Creating Roles

```sql
CREATE ROLE alice WITH LOGIN PASSWORD 'secret';
```

This creates a role with:
- `rolcanlogin = true` (can connect over the network)
- `rolsuper = false` (not a superuser)
- `rolcreatedb = false` (cannot create tables)
- `rolcreaterole = false` (cannot create other roles)
- Password stored as a SCRAM-SHA-256 verifier

```sql
CREATE ROLE admin WITH LOGIN SUPERUSER PASSWORD 'adminpass';
```

A superuser role bypasses all privilege checks. The query engine's privilege system grants superusers unrestricted access to all operations.

## Role Privilege Flags

Each role has the following boolean flags in `pg_authid`:

| Flag | Column | Description |
|------|--------|-------------|
| Superuser | `rolsuper` | Bypasses all privilege checks |
| Inherit | `rolinherit` | Inherits privileges from granted roles (not enforced yet) |
| Create role | `rolcreaterole` | Can execute `CREATE ROLE` |
| Create database | `rolcreatedb` | Can execute `CREATE TABLE`, `DROP TABLE`, `CREATE INDEX` |
| Can login | `rolcanlogin` | Can authenticate over the network and run DML |
| Bypass RLS | `rolbypassrls` | Bypasses row-level security (RLS not implemented yet) |

### Privilege Enforcement in the SQL Engine

When a query is submitted, the `QueryEngine::check_privileges` method inspects the logical plan and the current role:

```
Superuser role → all operations allowed

rolcreatedb required for:
  CREATE TABLE, DROP TABLE, CREATE INDEX

rolcreaterole required for:
  CREATE ROLE

rolcanlogin required for:
  SELECT, INSERT, UPDATE, DELETE
```

A role that lacks `rolcanlogin` — for example, a "group role" used only to group privileges — cannot execute DML even if it somehow connects (the authenticator also blocks login for roles with `rolcanlogin = false`).

If a role lacks the required privilege, the engine returns an error:

```
ERROR: Role 'alice' does not have CREATE TABLE privilege
```

## Table-Level Privileges

Role flags (`rolcreatedb`, `rolcanlogin`, etc.) control what kinds of operations a role may perform globally. Table-level privileges control which specific tables a role may read or write.

### Granting Privileges

```sql
GRANT <privilege_list> ON <table_name> TO <role_name>;
```

`<privilege_list>` is a comma-separated list of one or more of:

| Privilege | Operation allowed |
|-----------|-------------------|
| `SELECT`  | Read rows via `SELECT` |
| `INSERT`  | Add rows via `INSERT` |
| `UPDATE`  | Modify rows via `UPDATE` |
| `DELETE`  | Remove rows via `DELETE` |
| `ALL`     | All four of the above |

Example — a read-only reporting role:

```sql
CREATE ROLE reporter WITH LOGIN PASSWORD 'rpt-2024';

GRANT SELECT ON orders TO reporter;
GRANT SELECT ON books  TO reporter;
GRANT SELECT ON authors TO reporter;
```

Example — a read-write application role with access to specific tables:

```sql
CREATE ROLE appuser WITH LOGIN PASSWORD 'app-2024';

GRANT SELECT, INSERT, UPDATE, DELETE ON books   TO appuser;
GRANT SELECT, INSERT, UPDATE, DELETE ON authors TO appuser;
GRANT ALL ON orders TO appuser;
```

### Revoking Privileges

```sql
REVOKE <privilege_list> ON <table_name> FROM <role_name>;
```

Examples:

```sql
-- Remove write access from appuser on books
REVOKE INSERT, UPDATE, DELETE ON books FROM appuser;

-- Remove all access from reporter on orders
REVOKE ALL ON orders FROM reporter;
```

`REVOKE` removes only the named privileges; other existing grants on the same table are unaffected.

### How Privilege Checks Work

When a role (that is not a superuser) executes a DML statement, the query engine checks both the role-level flag and the table-level ACL before proceeding:

```
SELECT, INSERT, UPDATE, DELETE on a table
  → role must have rolcanlogin = true
  → role must have the matching table privilege (SELECT/INSERT/UPDATE/DELETE)

If either check fails → Permission denied error
```

ACL entries are stored as JSON files in the `acls/` subdirectory of the data directory (one file per table: `<schema>.<table_name>.json`). Superusers bypass all ACL checks.

### Privilege Check Example

```sql
-- As the icedb superuser:
CREATE ROLE reader WITH LOGIN PASSWORD 'pass';
GRANT SELECT ON books TO reader;

-- Connect as reader (via psql -U reader):
SELECT title FROM books;          -- succeeds
INSERT INTO books VALUES (...);   -- ERROR: Permission denied for table 'books'
```

The error message always names both the table and the role:

```
ERROR: Permission denied for table 'books': role 'reader' does not have INSERT privilege
```

### ACL Persistence

Grants and revokes are durable: the ACL file is written to disk immediately when the `GRANT` or `REVOKE` statement is executed. The ACL is reloaded automatically when the server restarts. There is no need to run a separate flush command.

## SCRAM-SHA-256 Authentication

### Overview

SCRAM-SHA-256 (Salted Challenge Response Authentication Mechanism with SHA-256, defined in RFC 5802) is the authentication standard used in PostgreSQL since version 10. icedb implements the same password storage format, making passwords portable between icedb and PostgreSQL.

The key security properties:
- The server never stores the plaintext password
- The server never even stores a hash of the password that could be replayed directly — it stores only derived keys
- The stored verifier cannot be used to authenticate without knowing the original password

### How Passwords Are Stored

When you execute `CREATE ROLE alice WITH LOGIN PASSWORD 'secret'`, icedb:

1. Generates a random 16-byte salt.
2. Derives a `SaltedPassword` using **PBKDF2 with HMAC-SHA-256** at 4,096 iterations:
   ```
   SaltedPassword = PBKDF2(HMAC-SHA-256, password, salt, 4096, 32)
   ```
3. Derives `ClientKey` and `ServerKey`:
   ```
   ClientKey = HMAC-SHA-256(SaltedPassword, "Client Key")
   ServerKey = HMAC-SHA-256(SaltedPassword, "Server Key")
   ```
4. Computes `StoredKey`:
   ```
   StoredKey = SHA-256(ClientKey)
   ```
5. Stores the verifier in `pg_authid.rolpassword` as:
   ```
   SCRAM-SHA-256$4096:<base64-salt>$<base64-StoredKey>:<base64-ServerKey>
   ```

This format is identical to what PostgreSQL uses. You can confirm it by reading the stored verifier:

```sql
-- Only superusers can query pg_authid directly (when exposed via SQL)
-- The format looks like:
-- SCRAM-SHA-256$4096:c2FsdGVkc2FsdA==$storedkey==:serverkey==
```

The 4,096 iteration count means an attacker who obtains the verifier must run 4,096 HMAC-SHA-256 operations per password guess. This significantly slows brute-force attacks.

### Password Verification at Login

When a client connects and provides a password:

1. The server looks up the stored verifier for the role in `pg_authid`.
2. The verifier is parsed to extract the salt, iteration count, and StoredKey.
3. The provided plaintext password is run through PBKDF2 with the same salt and iteration count.
4. The derived StoredKey is compared to the stored StoredKey.
5. If they match, authentication succeeds. If not, the connection is rejected with error code `28P01` (invalid password).

**Legacy/development plaintext fallback:** If the stored value in `rolpassword` cannot be parsed as a SCRAM-SHA-256 verifier (i.e., it does not start with `SCRAM-SHA-256$`), `verify_password` falls back to a direct string comparison of the stored value against the provided password. This path exists for test and development convenience only. In production, all passwords should be stored as SCRAM-SHA-256 verifiers (which `CREATE ROLE ... PASSWORD '...'` does automatically).

This means the server re-derives the key on every login attempt. The computation takes the same time as the original key derivation — approximately 4,096 HMAC-SHA-256 operations, which is fast for legitimate users (milliseconds) but slow for attackers who want to try millions of passwords per second.

### Network-Level Authentication

When a client connects via TCP (using `psql` or any PostgreSQL driver), the startup handshake works as follows:

1. Client sends a `Startup` message with the username.
2. Server responds with `Authentication: CleartextPassword` — requesting the password in cleartext over the connection.
3. Client sends the password.
4. Server runs the SCRAM verification against the stored verifier.
5. If verification passes, server sends `AuthenticationOK` followed by parameter status messages and `ReadyForQuery`.

**Note on cleartext transport:** The current wire-protocol implementation requests passwords in cleartext. In production, the connection **must** be protected by TLS (SSL) to prevent eavesdropping. TLS termination can be handled by a reverse proxy (e.g., `stunnel`, `nginx`, `haproxy`) in front of the icedb port. Native TLS support in the network layer is planned for a future release.

### The CLI Does Not Require Authentication

The `nkv-psql` CLI (`cargo run -p cli -- --data-dir ./data`) opens the database engine directly in-process. No network connection is established and no password is required. The CLI runs with full superuser access. This is intentional for local administration — it mirrors PostgreSQL's `peer` authentication for local Unix socket connections.

## Creating and Managing Roles in Practice

```sql
-- Create a read-write application user
CREATE ROLE appuser WITH LOGIN PASSWORD 'app-secret-2024';

-- Create a superuser for administration
CREATE ROLE dbadmin WITH LOGIN SUPERUSER PASSWORD 'admin-secret-2024';
```

List all roles:

```
icedb=# \du
                                   List of roles
 Role name |  Attributes
-----------+--------------
 icedb     | Superuser
```

(The `\du` output currently shows a static list. Full role enumeration from the catalog is in development.)

Connect as a specific user via `psql`:

```sh
psql -h 127.0.0.1 -p 5432 -U appuser -d icedb
```

If the password is wrong:

```
psql: error: connection to server at "127.0.0.1", port 5432 failed:
FATAL:  password authentication failed for user "appuser"
```

## Security Recommendations for Production

1. **Always use TLS on the network.** The wire protocol sends passwords in cleartext. Protect the port with a TLS proxy.

2. **Create application roles without superuser.** The `icedb` superuser should be used only for administration. Application code should connect as a role that has only the privileges it needs. Use `GRANT` to grant the minimum required table-level privileges (prefer `SELECT, INSERT, UPDATE` over `ALL` when `DELETE` is not needed by the application).

3. **Set strong passwords.** The SCRAM-SHA-256 verifier with 4,096 iterations is designed for passwords of reasonable length and entropy. Dictionary passwords are still vulnerable to offline attacks if the verifier is obtained.

4. **Restrict the listening address.** The server binds to `0.0.0.0` (all interfaces) by default and does not yet have a `--bind` flag. To restrict access to localhost only, use a firewall rule or a reverse proxy in front of the icedb port.

5. **Protect the data directory.** The data directory contains the catalog heap files including `catalog_pg_authid.heap`, which stores password verifiers. Restrict filesystem permissions so only the icedb process user can read the data directory.
