#!/usr/bin/env bash
# Run the icedb Node.js driver sandbox tests.
# Assumes install.sh has been run first.
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "==> Running icedb Node.js driver tests..."
cd "$SCRIPT_DIR"
node test_icedb.js
echo "==> All tests passed."
