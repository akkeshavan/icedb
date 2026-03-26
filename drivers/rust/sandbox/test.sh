#!/usr/bin/env bash
set -e
echo "Running icedb Rust driver sandbox tests..."
cargo run 2>&1
echo "Done."
