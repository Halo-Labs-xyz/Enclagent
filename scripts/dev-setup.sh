#!/usr/bin/env bash
# Developer setup script for Enclagent.
#
# Gets a fresh checkout ready for development without requiring
# Docker, PostgreSQL, or any external services.
#
# Usage:
#   ./scripts/dev-setup.sh
#
# After running, you can:
#   cargo check           # default features (postgres + libsql)
#   cargo test            # default test suite (uses libsql temp DB)
#   cargo test --all-features         # full test suite

set -euo pipefail

cd "$(dirname "$0")/.."

echo "=== Enclagent Developer Setup ==="
echo ""

# 1. Check rustup
if ! command -v rustup &>/dev/null; then
    echo "ERROR: rustup not found. Install from https://rustup.rs"
    exit 1
fi
echo "[1/5] rustup found: $(rustup --version 2>/dev/null | head -1)"

# Resolve required Rust toolchain from Cargo.toml and ensure it is installed.
required_rust="$(sed -n 's/^rust-version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if [ -z "$required_rust" ]; then
    echo "ERROR: Unable to read rust-version from Cargo.toml"
    exit 1
fi
echo "      required toolchain: ${required_rust}"

if ! rustup toolchain list | awk '{print $1}' | grep -Eq "^${required_rust}(\-|$)"; then
    echo "      installing toolchain ${required_rust}..."
    rustup toolchain install "${required_rust}"
fi

# Force all cargo operations in this script to use the workspace-required toolchain.
export RUSTUP_TOOLCHAIN="${required_rust}"
cargo_cmd=(rustup run "${required_rust}" cargo)

# 2. Add WASM target (required by build.rs for channel compilation)
echo "[2/5] Adding wasm32-wasip2 target..."
rustup target add wasm32-wasip2 --toolchain "${required_rust}"

# 3. Install wasm-tools (required by build.rs for WASM component model)
echo "[3/5] Installing wasm-tools..."
if command -v wasm-tools &>/dev/null; then
    echo "  wasm-tools already installed: $(wasm-tools --version)"
else
    "${cargo_cmd[@]}" install wasm-tools --locked
fi

# 4. Verify the project compiles
echo "[4/5] Running cargo check..."
"${cargo_cmd[@]}" check

# 5. Run tests using libsql temp DB (no Docker/external DB needed)
echo "[5/5] Running tests (no external DB required)..."
"${cargo_cmd[@]}" test

echo ""
echo "=== Setup complete ==="
echo ""
echo "Quick start:"
echo "  cargo run                            # Run with default features"
echo "  cargo test                           # Test suite (libsql temp DB)"
echo "  cargo test --all-features            # Full test suite"
echo "  cargo clippy --all-features          # Lint all code"
