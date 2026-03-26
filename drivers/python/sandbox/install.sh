#!/usr/bin/env bash
# Install the icedb Python driver into a local virtual environment.
# Run from: drivers/python/sandbox/
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DRIVER_DIR="$(dirname "$SCRIPT_DIR")"

echo "==> Creating virtual environment..."
python3 -m venv "$SCRIPT_DIR/.venv"
source "$SCRIPT_DIR/.venv/bin/activate"

echo "==> Installing maturin..."
pip install --quiet maturin

echo "==> Building and installing icedb driver..."
cd "$DRIVER_DIR"
# PYO3_USE_ABI3_FORWARD_COMPATIBILITY allows building with Python 3.14+
# where PyO3 only officially supports up to 3.13.
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 maturin develop --release

echo "==> Driver installed. Activate with: source sandbox/.venv/bin/activate"
