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
./target/release/nkv-psql --data-dir ./data
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

Build output: `target\release\icedb-server.exe` and `target\release\nkv-psql.exe`.

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

The CLI (`nkv-psql`) embeds the storage engine directly — no TCP connection or separate server is needed for local development:

```sh
./target/release/nkv-psql --data-dir ./data
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

### Integration tests

The integration tests live in `tests/`, which is a separate Cargo workspace:

```sh
cd tests
cargo test

# Single module
cargo test sql_conformance::joins

# With output
cargo test -- --nocapture
```

### Full sweep

```sh
# From the repo root
cargo test --workspace          # 253 unit tests
cd tests && cargo test          # 715 integration tests
```

Total: **968 tests**, 4 ignored, 0 failures.

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
