# Release Plan

## What needs releasing

The project has 5 distinct deliverables, each with its own distribution channel:

| Artifact | Binary/Package | Distribution |
|---|---|---|
| `icedb-server` | Standalone binary | GitHub Releases, Homebrew, apt, winget |
| `nkv-psql` (CLI) | Standalone binary | Same as server |
| Rust driver (`icedb-driver`) | Crate | crates.io |
| Python driver | Wheel (`.whl`) | PyPI via maturin |
| Node.js driver | Native addon | npm |

---

## Step 1 — Versioning

All workspace crates share a version. Bump `version = "0.1.0"` in the root `Cargo.toml` and each crate's `Cargo.toml`. This is the single source of truth.

For the Python and Node.js drivers, their `pyproject.toml` and `package.json` versions need to match.

A release is triggered by pushing a **git tag** like `v0.1.0`.

---

## Step 2 — Cross-platform binary builds (GitHub Actions)

Pre-compiled binaries for these 5 targets:

| Target triple | OS |
|---|---|
| `x86_64-unknown-linux-gnu` | Linux x86 |
| `aarch64-unknown-linux-gnu` | Linux ARM (e.g. AWS Graviton) |
| `x86_64-apple-darwin` | macOS Intel |
| `aarch64-apple-darwin` | macOS Apple Silicon |
| `x86_64-pc-windows-msvc` | Windows |

A GitHub Actions **matrix workflow** runs on push of a version tag:
- Compiles `icedb-server` and `nkv-psql` with `cargo build --release`
- Zips each binary with `tar.gz` (Linux/Mac) or `.zip` (Windows)
- Uploads all artifacts to a **GitHub Release** automatically

For Linux ARM cross-compilation from x86 runners, use the `cross` tool (a drop-in `cargo` wrapper that uses Docker).

---

## Step 3 — GitHub Release assets

After the matrix build, the release page contains:
```
icedb-v0.1.0-x86_64-apple-darwin.tar.gz
icedb-v0.1.0-aarch64-apple-darwin.tar.gz
icedb-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
icedb-v0.1.0-aarch64-unknown-linux-gnu.tar.gz
icedb-v0.1.0-x86_64-pc-windows-msvc.zip
```

Each archive contains `icedb-server` and `nkv-psql`.

---

## Step 4 — Package managers

### macOS — Homebrew

Create a Homebrew tap (`homebrew-icedb` repository). The formula downloads the correct binary from GitHub Releases based on the platform. Users install with:
```sh
brew tap your-org/icedb
brew install icedb
```

### Linux — apt/deb and rpm

Use `cargo-deb` to generate `.deb` packages and `cargo-rpm` for `.rpm`. These can be hosted on a self-managed apt/yum repository or submitted to a PPA. For a first release, a simple install script (like the one Rust itself uses) is more practical.

### Windows — winget and Chocolatey

Submit a manifest to the [winget-pkgs](https://github.com/microsoft/winget-pkgs) repository. Users install with:
```sh
winget install icedb
```

Chocolatey is similar but requires a chocolatey.org account.

### Cross-platform install script

A shell script (`install.sh`) that detects the OS/arch, downloads the right binary from GitHub Releases, and places it in `/usr/local/bin` — the quickest path to "one command install" for Mac and Linux.

---

## Step 5 — crates.io (Rust driver)

Run `cargo publish -p icedb-driver` from `drivers/rust/`. Requires a crates.io API token. The workspace library crates (`storage`, `wal`, `txn`, etc.) are internal and do not need to be published unless you want the engine itself to be a library crate.

---

## Step 6 — PyPI (Python driver)

Use **maturin** in CI:
```sh
maturin publish --manifest-path drivers/python/Cargo.toml
```

The matrix builds produce platform-specific wheels (`.whl`) for each OS/arch. Maturin handles cross-compilation and uploads them all to PyPI in one step. Users install with:
```sh
pip install icedb
```

---

## Step 7 — npm (Node.js driver)

Use **`@napi-rs/cli`** to build and publish:
```sh
napi build --release
npm publish
```

The NAPI-RS framework handles producing platform-specific native addons. Users install with:
```sh
npm install @icedb/driver
```

---

## Full workflow sequence

```
git tag v0.1.0 && git push --tags
        │
        ▼
GitHub Actions: "release" workflow triggers
        ├── matrix: compile binaries for 5 targets
        ├── upload .tar.gz/.zip to GitHub Release
        ├── cargo publish -p icedb-driver (crates.io)
        ├── maturin publish (PyPI, all platforms)
        └── napi publish (npm)
```

---

## Files to create (when ready to implement)

1. **`.github/workflows/release.yml`** — main CI/CD pipeline (matrix build + publish)
2. **`.github/workflows/ci.yml`** — runs `cargo test --workspace` on every PR
3. **`install.sh`** — one-line installer for Mac/Linux
4. **`install.ps1`** — PowerShell installer for Windows
5. **`homebrew-icedb/Formula/icedb.rb`** — Homebrew formula (separate repo)
6. Minor `Cargo.toml` metadata additions (`description`, `license`, `repository`, `homepage`) required by crates.io
