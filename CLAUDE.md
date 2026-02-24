# Enclagent Development Guide

## Scope

Enclagent is a single Rust runtime with an embedded gateway and frontdoor.

- Runtime crate: `enclagent`
- Frontdoor API: `/api/frontdoor/*`
- Static gateway assets: `src/channels/web/static/`

## Core Commands

```bash
./scripts/dev-setup.sh
cargo fmt --all --check
cargo check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features -- --nocapture
./scripts/verify-local.sh
```

## Startup

```bash
cargo +1.92.0 run -- onboard
cargo +1.92.0 run
cargo +1.92.0 run -- doctor startup
```

## Refactor Constraints

- Keep paper trading default unless policy explicitly enables live execution.
- Keep intent/receipt/verification lineage deterministic and persisted.
- Keep frontdoor contract stable under `/api/frontdoor/*`.
- Do not reintroduce separate frontend applications.
