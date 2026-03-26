# Chapter 2: Installation & Building from Source

## Prerequisites

icedb is built with the standard Rust toolchain. You need:

- **Rust 1.75 or later** (the `stable` channel). Earlier versions may compile but are not tested. Check your version with `rustc --version`.
- **Cargo** (bundled with Rust).
- A C linker (on Linux: `gcc` or `clang`; on macOS: Xcode Command Line Tools).
- Git, to clone the repository.

Optional, for building cross-language drivers:
- **Python 3.8+** and `maturin` (`pip install maturin`) — for the Python driver.
- **Node.js 18+** and `npm` — for the Node.js driver.

### Installing Rust

If you do not have Rust installed, the fastest path is `rustup`:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Follow the prompts. After installation, open a new shell (or run `source ~/.cargo/env`) and confirm:

```sh
rustc --version
# rustc 1.77.0 (expected output format)
cargo --version
# cargo 1.77.0
```

## Cloning the Repository

```sh
git clone <YOUR_REPO_URL>
cd icedb
```

Replace `<YOUR_REPO_URL>` with the actual repository URL from your source.

The repository uses a Cargo workspace. All crates live under `crates/` and all cross-language drivers live under `drivers/`. The workspace manifest is the root `Cargo.toml`.

## Building from Source

### Development Build

A development build compiles all crates with debug symbols and without optimizations. It is faster to compile but slower to run:

```sh
cargo build --workspace
```

Compilation downloads all dependencies on first run. Subsequent builds are incremental. On a modern laptop, the full workspace compiles in 60–90 seconds on first run, and under 5 seconds for incremental rebuilds.

Compiled binaries land in `target/debug/`:

| Binary | Description |
|--------|-------------|
| `target/debug/icedb-server` | The icedb server process |
| `target/debug/isql` | The `isql` interactive terminal |

The Cargo crate names are `server` and `cli` (used with `cargo run -p server` and `cargo run -p cli`). The compiled binary names are `icedb-server` and `isql` respectively, as declared in the `[[bin]]` sections of each crate's `Cargo.toml`.

### Release Build

The release build enables full compiler optimizations (`-C opt-level=3`) and link-time optimization. Use this for any performance measurement or production deployment:

```sh
cargo build --workspace --release
```

Release binaries land in `target/release/`. The release build is 3–5× faster than debug for query execution and I/O.

### Building Only the Server or CLI

If you only need one binary (use the crate name with `-p`):

```sh
cargo build -p server --release   # produces target/release/icedb-server
cargo build -p cli --release      # produces target/release/isql
```

## Running the Test Suite

The test suite covers every crate, from raw page layout to network protocol integration:

```sh
# Run all tests across all crates
cargo test --workspace

# Run tests for a specific crate
cargo test -p storage
cargo test -p btree
cargo test -p wal
cargo test -p txn
cargo test -p catalog
cargo test -p sql
cargo test -p network
cargo test -p cli

# Run a single test by name, with output printed
cargo test -p storage page_header_layout -- --nocapture
cargo test -p txn mvcc_visibility -- --nocapture
```

All tests should pass. The expected final line after a clean run is:

```
test result: ok. N passed; 0 failed; 0 ignored
```

If any test fails, do not proceed — something in the toolchain or build environment needs attention before the binaries are reliable.

### Lint and Format Checks

Before committing changes, the code must pass Clippy (Rust's linter) with zero warnings promoted to errors, and the formatter must produce no diffs:

```sh
# Lint — must pass with no warnings
cargo clippy --workspace --all-targets -- -D warnings

# Format check (no changes made)
cargo fmt --check

# Format in-place (if you want to auto-fix)
cargo fmt --all
```

## Directory Layout After Building

```
icedb/
├── target/
│   ├── debug/
│   │   ├── icedb-server    ← debug server binary
│   │   └── isql        ← debug CLI binary
│   └── release/
│       ├── icedb-server    ← release server binary
│       └── isql        ← release CLI binary
├── crates/                 ← all Rust library crates
├── drivers/                ← cross-language driver crates
├── docs/                   ← internal specs
├── tests/                  ← integration tests
└── Cargo.toml              ← workspace manifest
```

When the server runs, it creates a **data directory** containing all persistent state. The data directory is specified at startup with `--data-dir`. It does not need to exist beforehand — the server creates it on first run.

```
./data/                     ← example data directory
├── 0000000000000001.wal    ← WAL segment 1
├── checkpoint.ctl          ← last checkpoint LSN
├── catalog_pg_class.heap   ← system catalog: table registry
├── catalog_pg_attribute.heap ← system catalog: column definitions
├── catalog_pg_authid.heap  ← system catalog: roles
├── catalog_pg_namespace.heap ← system catalog: schemas
├── 16384.heap              ← user table heap file (OID 16384)
└── idx_16384_id.btree      ← B+ tree index on column "id"
```

## Quick Smoke Test

Start the server in one terminal:

```sh
cargo run -p server -- --port 5432 --data-dir ./smoketest-data
```

You should see:

```
INFO  icedb listening on 0.0.0.0:5432
```

Open a second terminal and connect with the CLI:

```sh
cargo run -p cli -- --data-dir ./smoketest-data
```

You should see the prompt:

```
icedb=#
```

Run a quick query to verify the engine is working:

```sql
CREATE TABLE ping (id INT, msg TEXT);
INSERT INTO ping VALUES (1, 'pong');
SELECT * FROM ping;
```

Expected output:

```
 id | msg
----+------
  1 | pong
(1 row)
```

Exit with `\q`. Stop the server with `Ctrl-C`.

If both steps work, the build is healthy. You can now safely delete `./smoketest-data/` and proceed to Chapter 3 for a complete tutorial.

## A Note on Pre-Built Binaries

Pre-built release binaries for Linux (x86_64, aarch64) and macOS (Apple Silicon, Intel) are planned for the first stable release. Until then, building from source as described above is the only supported installation method. The build process is deterministic and produces identical output given the same Rust toolchain version.
