# Workspace Layout

This repository is Enclagent-only.

## Current state

- Single Rust runtime repository
- No JS workspaces
- No separate frontend applications

## Active source roots

- `src/`
- `tests/`
- `migrations/`
- `channels-src/`
- `tools-src/`
- `deploy/`
- `scripts/`
- `docs/`

## Removed during refactor

- legacy external frontend app trees
- legacy duplicate upstream snapshot tree
- optional npm SDK wrapper

## Validation

Run from repo root:

```bash
./scripts/verify-local.sh
cargo build
cargo test
```
