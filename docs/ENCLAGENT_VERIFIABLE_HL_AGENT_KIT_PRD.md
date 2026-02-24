# Enclagent Verifiable Hyperliquid Agent Kit PRD

Status: Active

## Product Directive

Enclagent is a single-runtime Hyperliquid agent with embedded gateway and frontdoor.

- Runtime: Rust crate and binary `enclagent`
- Gateway: embedded Axum server
- Frontdoor API: `/api/frontdoor/*`
- Verifiability: deterministic `IntentEnvelope -> ExecutionReceipt -> VerificationRecord`

## Non-Negotiables

- Paper mode is default.
- Live execution requires explicit signer and policy gates.
- Every in-scope action persists verifiable intent and receipt lineage.
- Verification fallback remains available if primary backend degrades.
- Sandbox, secrets handling, and policy controls are never weakened.

## API Contract

Canonical frontdoor endpoints:

- `GET /api/frontdoor/bootstrap`
- `GET /api/frontdoor/config-contract`
- `POST /api/frontdoor/challenge`
- `POST /api/frontdoor/suggest-config`
- `POST /api/frontdoor/verify`
- `GET /api/frontdoor/session/{session_id}`

## Primary Implementation Surfaces

- `src/channels/web/frontdoor.rs`
- `src/channels/web/server.rs`
- `src/channels/web/types.rs`
- `src/channels/web/static/frontdoor.html`
- `src/channels/web/static/frontdoor.js`
- `src/channels/web/static/app.js`
- `src/agent/intent.rs`
- `src/tools/hyperliquid.rs`

## Validation Gates

- `./scripts/verify-local.sh`
- `cargo build`
- `cargo test`

## Operating Rule

`MAKE NO MISTAKES.`
