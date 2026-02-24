# Enclagent Fork Status

## Current State

Enclagent is now maintained as a standalone single-runtime repository.

Completed cleanup:

- Removed legacy external app trees.
- Removed duplicate upstream snapshot tree.
- Removed optional npm SDK wrapper from this repository.
- Consolidated docs and scripts around Enclagent-only runtime behavior.

## Runtime Scope

- Rust runtime and CLI
- Embedded web gateway
- Embedded frontdoor API and static UI
- Hyperliquid execution tooling
- Deterministic intent/receipt/verification pipeline

## Deployment Scope

- Service + Docker assets under `deploy/`
- Local and release verification under `scripts/`

## Remaining Rule

Enclagent remains paper-first with explicit policy gates for any live execution path.
