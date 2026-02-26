# Enclagent Verifiable Core Platform PRD

Status: Active
Last updated: 2026-02-26

## Product Directive

Enclagent is a single-runtime verifiable agent platform with embedded gateway and frontdoor.

- Runtime: Rust crate and binary `enclagent`
- Gateway: embedded Axum server
- Frontdoor API: `/api/frontdoor/*`
- Verifiability: deterministic `IntentEnvelope -> ExecutionReceipt -> VerificationRecord`
- Core posture: verification and policy first
- Module posture: Core-8 curated modules with optional addons
- Identity posture: wallet-first launch with mandatory wallet signature before provisioning; frontdoor UI also supports email/social identity entry that converges on mandatory wallet binding before launch

## Non-Negotiables

- EigenCloud primary verification plus signed fallback remains mandatory for in-scope actions.
- Every in-scope action persists verifiable intent and receipt lineage.
- Sandbox, secrets handling, and policy controls are never weakened.
- Hyperliquid is an optional addon module, not a core dependency.
- EigenDA is an optional addon module, not a core dependency.
- Every user-visible component must map to a backend contract and verifiable artifact.
- Every generated gateway must expose a machine-readable TODO state for required post-launch actions.
- Every non-wallet identity path must complete wallet binding before enclave provisioning.

## API Contract

Canonical frontdoor endpoints:

- `GET /api/frontdoor/bootstrap`
- `GET /api/frontdoor/config-contract`
- `GET /api/frontdoor/policy-templates`
- `GET /api/frontdoor/experience/manifest`
- `GET /api/frontdoor/onboarding/state?session_id=<uuid>`
- `POST /api/frontdoor/onboarding/chat`
- `POST /api/frontdoor/challenge`
- `POST /api/frontdoor/suggest-config`
- `POST /api/frontdoor/verify`
- `GET /api/frontdoor/session/{session_id}`
- `GET /api/frontdoor/session/{session_id}/timeline`
- `GET /api/frontdoor/session/{session_id}/verification-explanation`
- `POST /api/frontdoor/session/{session_id}/runtime-control`
- `GET /api/frontdoor/session/{session_id}/gateway-todos`
- `GET /api/frontdoor/session/{session_id}/funding-preflight`
- `GET /api/frontdoor/sessions?wallet_address=<0x...>&limit=<n>` (wallet filter required)

Protected frontdoor operator endpoints:

- `GET /api/frontdoor/operator/sessions?wallet_address=<0x...>&limit=<n>`
- `GET /api/gateway/todos?wallet_address=<0x...>&session_id=<uuid>&limit=<n>`

## Frontdoor Status Snapshot (2026-02-26)

- [x] Typed experience manifest endpoint (`/api/frontdoor/experience/manifest`)
- [x] Typed onboarding state/chat endpoints (`/api/frontdoor/onboarding/*`)
- [x] Typed policy template library endpoint (`/api/frontdoor/policy-templates`)
- [x] Typed session timeline and verification explanation endpoints
- [x] Typed runtime control endpoint (`pause`, `resume`, `terminate`, `rotate_auth_key`)
- [x] Typed per-session and aggregated gateway TODO payloads
- [x] Funding preflight endpoint with categorized failures (`gas`, `fee`, `auth`, `policy`)
- [x] Redacted public monitor and protected operator monitor split
- [x] Railway signed frontdoor E2E gate passing in staging + production
- [ ] Dedicated command provisioning as the default deployed production path
- [x] Frontdoor email/social login entry UX
- [x] Launch flow enforces onboarding step-4 confirmation before signature verify/provisioning trigger
- [x] Frontdoor UI blocked states and failures emit deterministic next action + typed operator hints

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
- `src/channels/web/static/frontdoor.css`
- `src/agent/intent.rs`
- `src/tools/hyperliquid.rs`

## Experience Architecture Requirements

- Frontend modules are composable and registry-driven: `Identity`, `Policy`, `Verification`, `Provisioning`, `RuntimeControls`, `Evidence`.
- Identity module target must support wallet connect and email/social login entry, then converge on wallet-signature authorization.
- Onboarding module must be conversational-first and stateful, with deterministic checkpointing.
- Each module must declare `purpose_id`, `backend_contract`, `artifact_binding`, `success_state`, and `failure_state`.
- UI state must be server-driven from frontdoor session status; local optimistic success is prohibited for terminal states.
- Interactivity must be state-meaningful: timeline updates, verification detail updates, and runtime control confirmations.
- Content must be concise, objective-linked, and evidence-backed; decorative content without operational value is prohibited.
- Generated gateway entry must prioritize unresolved required TODOs before secondary analytics surfaces.
- Spawned enclave workspace must provide a unified thread-first operator UI with storage tree, skills, automations, logs, action history, compute usage, and settings surfaces.

## Validation Gates

- `./scripts/verify-local.sh`
- `cargo +1.92.0 fmt --all --check`
- `cargo +1.92.0 check`
- `cargo +1.92.0 clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo +1.92.0 test --workspace --all-features -- --nocapture --test-threads=1`
- `./scripts/verify-ecloud-foundation.sh <env-file>`
- `./scripts/release-demo-readiness.sh <env-file>`

## Current Execution Priorities

1. Dedicated enclave provisioning as primary path
- Replace `default_instance_url` primary path with command-based per-session provisioning in Railway staging and production.
- Require session evidence fields for dedicated launch (`provisioning_source=command`, `dedicated_instance=true`, `launched_on_eigencloud=true`, non-null `eigen_app_id`).

2. Verification observability
- Emit structured frontdoor lifecycle telemetry for each session.
- Provide operator-grade monitoring for verification level and backend outcomes while preserving public endpoint redaction.

3. Frontdoor auth reliability
- Keep non-SIWE Privy wallet connect + gasless signature proof flow deterministic.
- Keep explicit, typed errors for missing Privy config, stale challenges, and signature mismatch.

4. Policy/runtime controls
- Maintain signer/policy gates before any live trading action.
- Keep kill-switch and runtime controls auditable with deterministic state transitions.

5. Experience system quality
- Enforce component-purpose contract and remove orphan UI elements.
- Enforce typed experience manifest + timeline + verification explanation payloads.
- Enforce one primary action per step and deterministic blocked-state remediation.

6. Generated gateway TODO intelligence
- Enforce typed TODO payloads and state transitions for generated gateways.
- Enforce auditability for TODO resolution events.

7. Conversational onboarding and workspace cohesion
- Enforce chat-led onboarding from agent objective -> validated config -> signed provisioning.
- Enforce post-provision redirect into unified enclave workspace without identity/session discontinuity.

## Operating Rule

`MAKE NO MISTAKES.`
