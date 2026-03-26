#!/usr/bin/env bash
# Build the icedb Node.js driver and link it for the sandbox.
# Run from: drivers/nodejs/sandbox/
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DRIVER_DIR="$(dirname "$SCRIPT_DIR")"

echo "==> Checking for @napi-rs/cli..."
if ! command -v napi &>/dev/null; then
    echo "    Installing @napi-rs/cli globally..."
    npm install -g @napi-rs/cli
fi

echo "==> Building icedb Node.js driver..."
cd "$DRIVER_DIR"
npm install
napi build --platform --release

echo "==> Installing sandbox dependencies..."
cd "$SCRIPT_DIR"
npm install

echo "==> Driver built. Run test.sh to execute tests."
