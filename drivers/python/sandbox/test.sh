#!/usr/bin/env bash
# Run the icedb Python driver sandbox tests.
# Assumes install.sh has been run first.
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ ! -d "$SCRIPT_DIR/.venv" ]; then
    echo "ERROR: virtual environment not found. Run install.sh first."
    exit 1
fi

source "$SCRIPT_DIR/.venv/bin/activate"
echo "==> Running icedb Python driver tests..."
python3 "$SCRIPT_DIR/test_icedb.py"
echo "==> All tests passed."
