# icedb

A production-grade, PostgreSQL-compatible RDBMS built in Rust.

icedb implements the full PostgreSQL wire protocol (v3.0), ACID transactions via WAL + MVCC, a page-based storage engine with persistent B+ tree indexes, and cross-language drivers for Rust, Python, and Node.js. Any standard PostgreSQL client — `psql`, DBeaver, pgAdmin, or your favourite ORM — connects without modification.

---

## Contents

- [Requirements](#requirements)
- [Quick start](#quick-start)
- [Building from source](#building-from-source)
  - [macOS](#macos)
  - [Linux](#linux)
  - [Windows](#windows)
- [Running the server](#running-the-server)
  - [Running the server with TLS](#running-the-server-with-tls)
- [Connecting with the built-in CLI](#connecting-with-the-built-in-cli)
- [Connecting with psql](#connecting-with-psql)
- [Admin UI](#admin-ui)
- [Running tests](#running-tests)
- [Building the drivers](#building-the-drivers)
  - [Python driver](#python-driver)
  - [Node.js driver](#nodejs-driver)
- [Troubleshooting](#troubleshooting)

---

## Requirements

| Tool | Minimum version | Notes |
|------|----------------|-------|
| Rust + Cargo | **1.80** | Install via [rustup](https://rustup.rs) |
| Node.js | **18 LTS** | Only needed for the admin UI and Node.js driver |
| npm | **9** | Bundled with Node.js |
| Python | **3.8** | Only needed for the Python driver |
| maturin | **1.0** | Only needed for the Python driver (`pip install maturin`) |

No external database or system library is required to build or run the core server. All storage is handled natively.

---

## Quick start

```sh
# 1. Clone the repo
git clone https://github.com/your-org/icedb.git
cd icedb

# 2. Build the server and CLI (optimised release build)
cargo build --release -p server -p cli

# 3. Start the server
./target/release/icedb-server --port 5432 --data-dir ./data

# 4. Connect (in a second terminal)
./target/release/isql --data-dir ./data
```

---

## Building from source

### macOS

#### 1. Install Rust

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Verify:

```sh
rustc --version   # should print 1.80 or later
cargo --version
```

#### 2. Install Xcode Command Line Tools (if not already present)

```sh
xcode-select --install
```

This provides the system linker (`cc`) and `git`. If Xcode is already installed you can skip this.

#### 3. Clone and build

```sh
git clone https://github.com/your-org/icedb.git
cd icedb

# Debug build (fast compile, slower runtime — good for development)
cargo build --workspace

# Release build (optimised — use this for benchmarks and production)
cargo build --workspace --release
```

Build artefacts land in `target/debug/` or `target/release/`.

#### 4. (Optional) Admin UI

```sh
cd admin-ui
npm install
npm run build        # production bundle → admin-ui/dist/
npm run dev          # development server on http://localhost:5173
```

---

### Linux

Tested on Ubuntu 22.04 LTS, Debian 12, Fedora 39, and Arch Linux.

#### 1. Install system dependencies

**Ubuntu / Debian:**

```sh
sudo apt update
sudo apt install -y build-essential curl git pkg-config
```

**Fedora / RHEL / CentOS:**

```sh
sudo dnf install -y gcc make curl git pkg-config
```

**Arch Linux:**

```sh
sudo pacman -Sy --needed base-devel curl git
```

#### 2. Install Rust

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

#### 3. Clone and build

```sh
git clone https://github.com/your-org/icedb.git
cd icedb
cargo build --workspace --release
```

#### 4. (Optional) Admin UI

Node.js 18+ is required. Install via [nvm](https://github.com/nvm-sh/nvm) or your distribution's package manager:

```sh
# Via nvm (recommended — works on all distros)
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.7/install.sh | bash
source ~/.bashrc
nvm install 20
nvm use 20

cd admin-ui
npm install
npm run build
```

---

### Windows

Tested on Windows 10 and Windows 11 (x86-64). Windows on ARM (ARM64) is not yet validated.

#### 1. Install Visual Studio Build Tools

Rust on Windows requires the MSVC linker. Install **Visual Studio Build Tools 2022** (free):

1. Download from [visualstudio.microsoft.com/visual-cpp-build-tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
2. Run the installer and select **"Desktop development with C++"**
3. Make sure **MSVC v143** and **Windows SDK** are checked

> **Alternative:** If you have a full Visual Studio 2019 or 2022 installation (Community, Professional, or Enterprise) with the "C++ workload" enabled, that also satisfies this requirement.

#### 2. Install Rust

Download and run the installer from [rustup.rs](https://rustup.rs). Accept the defaults (stable toolchain, MSVC ABI).

Open a new **Command Prompt** or **PowerShell** window after installation:

```powershell
rustc --version
cargo --version
```

#### 3. Clone and build

Using **Git for Windows** (download from [git-scm.com](https://git-scm.com)) or Windows Subsystem for Linux (WSL2):

```powershell
git clone https://github.com/your-org/icedb.git
cd icedb
cargo build --workspace --release
```

Build output: `target\release\icedb-server.exe` and `target\release\isql.exe`.

#### 4. (Optional) Admin UI

Install Node.js 18+ from [nodejs.org](https://nodejs.org). Then in PowerShell:

```powershell
cd admin-ui
npm install
npm run build
```

#### Windows-specific notes

- The data directory path must use backslashes or be quoted: `--data-dir .\data`
- Long path support should be enabled. Run this once in an elevated PowerShell:
  ```powershell
  Set-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem" `
      -Name LongPathsEnabled -Value 1
  ```
- WSL2 is a fully supported alternative — follow the Linux instructions inside a WSL2 Ubuntu shell.

---

## Running the server

```sh
# Create a data directory and start the server
mkdir -p ./data
./target/release/icedb-server --port 5432 --data-dir ./data
```

```
INFO  icedb listening on 0.0.0.0:5432
```

On first startup icedb bootstraps the system catalogs (`pg_class`, `pg_attribute`, `pg_authid`, `pg_namespace`) and writes the initial WAL segment. This takes under a second and happens only once.

### Server options

| Flag | Default | Description |
|------|---------|-------------|
| `--port` | `5432` | TCP port to listen on |
| `--data-dir` | required | Directory for WAL segments, heap files, and indexes |
| `--host` | `0.0.0.0` | Bind address |
| `--max-connections` | `128` | Maximum simultaneous client connections |
| `--shared-buffers` | `1024` | Buffer pool size in 8 kB frames |
| `--tls-cert` | none | Path to a PEM-encoded X.509 server certificate (enables TLS) |
| `--tls-key` | none | Path to a PEM-encoded PKCS#8 private key (required when `--tls-cert` is set) |

Both `--tls-cert` and `--tls-key` must be supplied together. If either is omitted, the server starts without TLS and logs a warning. See [Running the server with TLS](#running-the-server-with-tls).

---

### Running the server with TLS

TLS encrypts all traffic between clients and the server. It is optional today (see [known issues #36](./known_issues.md)) but strongly recommended for any deployment beyond a local development machine.

#### Step 1 — Generate a certificate

**Self-signed certificate (development / testing)**

Use OpenSSL to create a certificate that is valid for 365 days:

```sh
openssl req -x509 \
  -newkey rsa:4096 \
  -keyout server.key \
  -out server.crt \
  -days 365 \
  -nodes \
  -subj "/CN=localhost"
```

This writes two files into the current directory:

| File | Contents |
|------|---------|
| `server.crt` | PEM-encoded X.509 certificate (public) |
| `server.key` | PEM-encoded PKCS#8 private key (keep this secret) |

Restrict permissions on the key file:

```sh
chmod 600 server.key
```

**Production certificate**

For a production deployment, obtain a certificate from a trusted Certificate Authority (CA) such as Let's Encrypt, DigiCert, or your internal PKI. The certificate file must be PEM-encoded and may include intermediate certificates chained after the server certificate. The key file must be PKCS#8 PEM format.

---

#### Step 2 — Start the server with TLS

```sh
./target/release/icedb-server \
  --port 5432 \
  --data-dir ./data \
  --tls-cert ./server.crt \
  --tls-key  ./server.key
```

The server log confirms TLS is active:

```
INFO  TLS enabled (cert: ./server.crt)
INFO  icedb listening on 0.0.0.0:5432
```

If either flag is missing the log will instead say:

```
INFO  TLS disabled (no --tls-cert/--tls-key)
```

and the server accepts plaintext connections — do not use this in production.

---

#### Step 3 — Connect over TLS

**icedb CLI (`isql`) — recommended**

`isql` has a built-in network client mode. When `--host` is given it connects to a running server over TCP with optional TLS rather than opening the embedded engine.

```sh
# TLS with a self-signed cert (skips certificate verification)
isql --host localhost --port 5432 --user icedb --sslmode require

# TLS with certificate verification against a CA file
isql --host localhost --port 5432 --user icedb \
     --sslmode verify-full --sslrootcert ./ca.crt
```

You will see the server address in the prompt banner:

```
isql (icedb 0.1.0) (server localhost:5432)
Type "help" for help, "\q" to quit.

icedb=#
```

All meta-commands (`\dt`, `\du`, `\l`, `\d tablename`, `\c`, `\timing`, `\x`, `\q`) work the same as in embedded mode.  Table-listing commands (`\dt`, `\du`, `\l`) are translated to SQL queries against `pg_catalog` and `information_schema` on the server.

**`psql` (standard PostgreSQL client)**

Any standard PostgreSQL client also works since icedb speaks the wire protocol:

```sh
psql "host=localhost port=5432 user=icedb sslmode=require"
```

For a CA-signed certificate:

```sh
psql "host=localhost port=5432 user=icedb sslmode=verify-full sslrootcert=./ca.crt"
```

**Connection string (ORMs, drivers, DBeaver, pgAdmin)**

```
postgresql://icedb@localhost:5432/icedb?sslmode=require
```

With certificate verification:

```
postgresql://icedb@localhost:5432/icedb?sslmode=verify-full&sslrootcert=/path/to/ca.crt
```

**icedb Rust driver**

```rust
let conn = icedb::connect("postgresql://icedb@localhost:5432/icedb?sslmode=require")?;
```

**icedb Python driver**

```python
import icedb
conn = icedb.connect(host="localhost", port=5432, user="icedb", sslmode="require")
```

**icedb Node.js driver**

```js
const icedb = require("@icedb/driver");
const conn = icedb.connect({ host: "localhost", port: 5432, user: "icedb", sslmode: "require" });
```

---

#### `sslmode` reference

| `sslmode` value | Encrypts | Verifies server cert | When to use |
|----------------|----------|---------------------|-------------|
| `disable` | No | No | Local development only (not recommended) |
| `allow` | Maybe | No | Never use |
| `prefer` | If available | No | Not recommended |
| `require` | Yes | No | Development with a self-signed cert |
| `verify-ca` | Yes | Yes (CA chain) | Production with a known CA |
| `verify-full` | Yes | Yes (CA + hostname) | Production (recommended) |

> **isql `sslmode` mapping**: `disable` → no TLS; `allow`/`prefer`/`require` → encrypt without cert verification; `verify-ca`/`verify-full` → encrypt and verify against `--sslrootcert`.

---

### Running in the background (Linux / macOS)

```sh
nohup ./target/release/icedb-server --port 5432 --data-dir ./data > icedb.log 2>&1 &
echo $! > icedb.pid
```

Stop it:

```sh
kill $(cat icedb.pid)
```

---

## Connecting with the built-in CLI

The CLI (`isql`) embeds the storage engine directly — no TCP connection or separate server is needed for local development:

```sh
./target/release/isql --data-dir ./data
```

```
icedb=#
```

Useful meta-commands:

| Command | Description |
|---------|-------------|
| `\dt` | List all tables |
| `\d tablename` | Describe a table's columns |
| `\du` | List roles |
| `\i file.sql` | Execute a SQL file |
| `\timing` | Toggle query timing |
| `\x` | Toggle expanded output |
| `\dump path` | Logical backup (SQL dump) |
| `\restore path` | Restore from a dump |
| `\q` | Quit |

---

## Connecting with psql

Any PostgreSQL client works. With the server running on port 5432:

```sh
psql -h 127.0.0.1 -p 5432 -U icedb
```

```
psql (16.x)
Type "help" for help.

icedb=#
```

Connection string format (for ORMs and drivers):

```
postgresql://icedb@127.0.0.1:5432/icedb
```

---

## Admin UI

The admin UI is a React + Vite application that talks to the icedb admin REST API server.

### Development

```sh
# Start the admin API server (serves on port 8080 by default)
./target/release/icedb-server --port 5432 --data-dir ./data &

# Start the UI dev server (hot-reload on http://localhost:5173)
cd admin-ui
npm install
npm run dev
```

### Production

```sh
cd admin-ui
npm run build          # output → admin-ui/dist/
```

Serve `admin-ui/dist/` from any static file server (nginx, Caddy, etc.) or mount it inside the icedb admin server.

---

## Running tests

### Unit tests (Rust crates)

```sh
# All crates
cargo test --workspace

# A single crate
cargo test -p storage
cargo test -p txn
cargo test -p sql

# A single test by name
cargo test -p txn -- mvcc_visibility --nocapture
```

### Integration tests — three modes

The integration tests live in `tests/`, a separate Cargo workspace. Every test runs in **three modes** automatically:

| Mode | Description |
|------|-------------|
| **Embedded** | SQL dispatched directly through an in-process `QueryEngine` — no TCP, no server process |
| **Plain TCP** | A real `icedb-server` subprocess is started on a free port; `PgClient` connects over plaintext PostgreSQL wire protocol |
| **TLS** | Same as plain TCP but with `sslmode=require` and a self-signed certificate generated via `openssl` |

```sh
# Run all integration tests (all three modes)
cargo test --manifest-path tests/Cargo.toml

# Single module (runs embedded + plain TCP + TLS variants)
cargo test --manifest-path tests/Cargo.toml sql_conformance::joins

# Embedded-only (fastest — no server startup overhead)
cargo test --manifest-path tests/Cargo.toml -- --skip _net

# Network variants only
cargo test --manifest-path tests/Cargo.toml -- _net

# Skip TLS (useful when openssl is unavailable)
cargo test --manifest-path tests/Cargo.toml -- --skip _net_tls

# With output
cargo test --manifest-path tests/Cargo.toml -- --nocapture
```

### Chapter 3 sandbox — three modes

`sandbox/ch03` is a standalone Cargo workspace that runs every SQL example from the icedb-book chapter 3 quick-start guide in all three modes:

```sh
# Run all three ch03 modes as cargo tests
cargo test --manifest-path sandbox/ch03/Cargo.toml

# Or run the sandbox as a binary (prints a formatted pass/fail/skip table)
cargo run --manifest-path sandbox/ch03/Cargo.toml
```

### Full sweep

```sh
# From the repo root
cargo test --workspace                                    # 313 unit tests
cargo test --manifest-path tests/Cargo.toml              # 2204 integration tests (3 modes)
cargo test --manifest-path sandbox/ch03/Cargo.toml       # 3 ch03 tests (3 modes)
```

Total: **2520 tests**, 6 ignored, 0 failures.

> **Note:** Plain TCP and TLS network tests start a real `icedb-server` process on a free port.
> Build the server binary first if you haven't already:
> ```sh
> cargo build -p server
> ```
> TLS tests require `openssl` on `PATH`. Skip them with `--skip _net_tls` if unavailable.

### Lint and format

```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check                # check only
cargo fmt --all                  # format in place
```

---

## Building the drivers

### Python driver

Requires Python 3.8+ and [maturin](https://github.com/PyO3/maturin).

```sh
pip install maturin

cd drivers/python

# Build and install into the current Python environment
maturin develop

# Build a wheel for distribution
maturin build --release
# Output: drivers/python/target/wheels/icedb-*.whl
```

Install the wheel:

```sh
pip install target/wheels/icedb-*.whl
```

Usage:

```python
import icedb
conn = icedb.connect(host="127.0.0.1", port=5432, user="icedb")
rows = conn.execute("SELECT * FROM books")
```

### Node.js driver

Requires Node.js 18+ and the [NAPI-RS CLI](https://napi.rs).

```sh
npm install -g @napi-rs/cli

cd drivers/nodejs
npm install

# Build the native .node binary for the current platform
napi build --platform --release
```

This produces `drivers/nodejs/index.darwin-arm64.node` (or the equivalent for your platform).

Usage:

```js
const icedb = require("@icedb/driver");
const conn = icedb.connect({ host: "127.0.0.1", port: 5432, user: "icedb" });
const rows = conn.execute("SELECT * FROM books");
```

---

## Troubleshooting

### `error: linker 'cc' not found` (Linux)

Install build tools:

```sh
# Ubuntu / Debian
sudo apt install build-essential

# Fedora
sudo dnf install gcc
```

### `error: linker 'link.exe' not found` (Windows)

The MSVC linker is missing. Re-run the Visual Studio Build Tools installer and ensure **"Desktop development with C++"** is selected. Then restart your terminal.

### Port 5432 already in use

Another PostgreSQL instance is running. Either stop it or run icedb on a different port:

```sh
./target/release/icedb-server --port 5433 --data-dir ./data
psql -h 127.0.0.1 -p 5433 -U icedb
```

### Long compile times on first build

Rust compiles all dependencies from source on first build. Subsequent builds only recompile changed crates. On a modern laptop expect 60–120 seconds for the first `--release` build; incremental rebuilds take 2–10 seconds.

To speed up repeated builds install [sccache](https://github.com/mozilla/sccache):

```sh
cargo install sccache
export RUSTC_WRAPPER=sccache
```

### `Permission denied` on the data directory (Linux / macOS)

```sh
chmod 700 ./data
```

The data directory should be readable and writable only by the user running the server.

### Admin UI shows a blank page

Make sure the admin API server is running and the `VITE_API_URL` environment variable points to it:

```sh
# In admin-ui/.env.local
VITE_API_URL=http://localhost:8080
```

Then restart `npm run dev`.

### TLS: `SSL connection has been closed unexpectedly`

The server is running without TLS but the client is requiring it (or vice versa). Check that:

1. The server was started with both `--tls-cert` and `--tls-key`.
2. The server log says `TLS enabled` — if it says `TLS disabled` the server is in plaintext mode.
3. The client connection string includes `sslmode=require` (or another TLS-enabling mode).

### TLS: `certificate verify failed` or `SSL certificate problem`

You are connecting with `sslmode=verify-full` or `sslmode=verify-ca` to a server using a self-signed certificate. Either:

- Switch to `sslmode=require` for development (skips CA verification):
  ```sh
  isql --host localhost --user icedb --sslmode require
  # or with psql:
  psql "host=localhost port=5432 user=icedb sslmode=require"
  ```
- Or pass the self-signed cert as the trusted root:
  ```sh
  isql --host localhost --user icedb --sslmode verify-full --sslrootcert ./server.crt
  # or with psql:
  psql "host=localhost port=5432 user=icedb sslmode=verify-ca sslrootcert=./server.crt"
  ```

### TLS: `no private key found in key file`

The key file must be in PKCS#8 PEM format. If you generated an RSA key with an older OpenSSL command, convert it:

```sh
openssl pkcs8 -topk8 -nocrypt -in server-rsa.key -out server.key
```
