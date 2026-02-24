# Enclagent Verifiable Core Platform PRD

Status: Active

## Product Directive

Enclagent is a single-runtime verifiable agent platform with embedded gateway and frontdoor.

- Runtime: Rust crate and binary `enclagent`
- Gateway: embedded Axum server
- Frontdoor API: `/api/frontdoor/*`
- Verifiability: deterministic `IntentEnvelope -> ExecutionReceipt -> VerificationRecord`
- Core posture: verification and policy first
- Module posture: Core-8 curated modules with optional addons

## Non-Negotiables

- EigenCloud primary verification plus signed fallback remains mandatory for in-scope actions.
- Every in-scope action persists verifiable intent and receipt lineage.
- Sandbox, secrets handling, and policy controls are never weakened.
- Hyperliquid is an optional addon module, not a core dependency.
- EigenDA is an optional addon module, not a core dependency.

## API Contract

Canonical frontdoor endpoints:

- `GET /api/frontdoor/bootstrap`
- `GET /api/frontdoor/config-contract`
- `POST /api/frontdoor/challenge`
- `POST /api/frontdoor/suggest-config`
- `POST /api/frontdoor/verify`
- `GET /api/frontdoor/session/{session_id}`

Canonical module endpoints (protected):

- `GET /api/modules/catalog`
- `GET /api/modules/state`
- `POST /api/modules/{module_id}/enable`
- `POST /api/modules/{module_id}/disable`
- `GET /api/modules/{module_id}/health`
- `PUT /api/modules/{module_id}/config`

Canonical org workspace endpoints (protected):

- `GET /api/org/current`
- `GET /api/org/members`
- `POST /api/org/members/invite`
- `PUT /api/org/members/{member_id}/role`
- `DELETE /api/org/members/{member_id}`

## Core-8 Module Suite

- `general`
- `developer`
- `creative`
- `research`
- `business_ops`
- `communications`
- `hyperliquid_addon` (optional, disabled by default)
- `eigenda_addon` (optional, disabled by default)

## Primary Implementation Surfaces

- `src/platform/mod.rs`
- `src/channels/web/server.rs`
- `src/channels/web/frontdoor.rs`
- `src/channels/web/types.rs`
- `src/channels/web/static/frontdoor.js`
- `src/agent/intent.rs`
- `src/tools/hyperliquid.rs`

## Validation Gates

- `./scripts/verify-local.sh`
- `cargo build`
- `cargo test`

## Operating Rule

`MAKE NO MISTAKES.`
