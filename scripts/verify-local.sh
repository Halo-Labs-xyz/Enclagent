#!/usr/bin/env bash
# Full local verification for Enclagent.
#
# Runs formatter, build/lint/test checks, and rebrand consistency checks.
# Intended for running in a normal terminal with network access.

set -euo pipefail

cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

echo "== Enclagent local verification =="
RUST_TOOLCHAIN="1.92.0"
CR="cargo +${RUST_TOOLCHAIN}"

echo "[1/10] rustup check"
command -v rustup >/dev/null

echo "[2/10] ensure rust toolchain ${RUST_TOOLCHAIN}"
rustup toolchain install "${RUST_TOOLCHAIN}" --profile minimal

echo "[3/10] ensure rustfmt + clippy components"
rustup component add --toolchain "${RUST_TOOLCHAIN}" rustfmt clippy

echo "[4/10] ensure wasm target"
rustup target add wasm32-wasip2 --toolchain "${RUST_TOOLCHAIN}"

echo "[5/10] ensure wasm-tools"
if ! command -v wasm-tools >/dev/null; then
  ${CR} install wasm-tools --locked
fi

echo "[6/10] fetch crates"
${CR} fetch

echo "[7/10] format check"
${CR} fmt --all --check

echo "[8/10] compile check"
${CR} check

echo "[9/10] lint + tests"
${CR} clippy --workspace --all-targets --all-features -- -D warnings
${CR} test --workspace --all-features -- --nocapture --test-threads=1

echo "[10/10] verification flow complete"

echo "all checks passed"
