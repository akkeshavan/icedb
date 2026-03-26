#!/usr/bin/env bash
set -e
echo "Building icedb-rust-sandbox..."
cargo build 2>&1
echo "Build complete."
