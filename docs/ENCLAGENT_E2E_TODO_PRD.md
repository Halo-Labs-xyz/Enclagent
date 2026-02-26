# Enclagent End-to-End TODO PRD

Status: Active execution checklist
Last updated: 2026-02-26

## Objective

Deliver a production-grade Enclagent runtime that is verifiable-core, module-governed, and operable as a single embedded gateway product.

## Verified Baseline Snapshot (2026-02-26)

- Production `enclagent-core` and staging `enclagent-core-staging` are deployed and healthy.
- Frontdoor challenge -> suggest-config -> verify -> session polling flow reaches terminal `ready` in both environments.
- Railway signed E2E gate script (`scripts/verify-frontdoor-railway-signed-e2e.sh`) passes for staging + production.
- `/api/frontdoor/sessions` wallet filter requirement is enforced (`400` when missing `wallet_address`).
- Typed frontdoor experience/observability endpoints are live: manifest, onboarding state/chat, timeline, verification explanation, runtime controls, gateway TODO feeds, and funding preflight.
- Public redacted monitor and protected operator monitor are both live (`/api/frontdoor/sessions`, `/api/frontdoor/operator/sessions`).
- Current provisioning path is `default_instance_url`; sessions are not dedicated EigenCloud launches (`dedicated_instance=false`, `launched_on_eigencloud=false`).

## Current Endpoint Contract (Implemented)

Public frontdoor routes:

- `GET /api/frontdoor/bootstrap`
- `GET /api/frontdoor/config-contract`
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

Protected operator routes:

- `GET /api/frontdoor/operator/sessions?wallet_address=<0x...>&limit=<n>`
- `GET /api/gateway/todos?wallet_address=<0x...>&session_id=<uuid>&limit=<n>`

## Next Execution Checklist

### P0. Dedicated Enclave Provisioning (Blocker)

- [ ] Enable per-session dedicated provisioning in Railway staging and production.
- [ ] Configure `GATEWAY_FRONTDOOR_PROVISION_COMMAND` for both environments.
- [ ] Keep `GATEWAY_FRONTDOOR_DEFAULT_INSTANCE_URL` only as explicit fallback, not primary path.
- [ ] Verify `provisioning_source=command` for new sessions.
- [ ] Verify `dedicated_instance=true` and `launched_on_eigencloud=true`.
- [ ] Verify `eigen_app_id` is populated on successful dedicated launch.
- [ ] Fail closed when provisioning command is missing or malformed.

Acceptance criteria:
- New signed session in staging and production returns `ready` with `provisioning_source=command`, `dedicated_instance=true`, and non-null `eigen_app_id`.
- Session failure paths return explicit operator-actionable errors.

### P0. Frontdoor Authentication + Wallet Proof Reliability

- [x] Keep Privy wallet connect flow non-SIWE and deterministic.
- [x] Keep Privy entry UX as a standard connect button with provider chooser.
- [x] Support Privy email login and social login as first-step identity options.
- [x] Require wallet binding before provisioning for any non-wallet initial login.
- [x] Harden connect-wallet UI path to always initialize Privy modal when `privy_app_id` is present.
- [x] Add explicit UI and API diagnostics for missing Privy config.
- [x] Keep gasless authorization signature flow mandatory before provisioning.
- [x] Add replay-protection assertions for challenge nonce/session binding.

Acceptance criteria:
- `POST /api/frontdoor/challenge` and `POST /api/frontdoor/verify` pass across Chrome/Safari mobile+desktop.
- Signature mismatch, stale challenge, and wallet mismatch each produce stable typed errors.
- Email-first users can complete wallet binding and reach the same signed provisioning flow.

### P0. Conversational Onboarding Orchestration

- [x] Add interactive chat onboarding that drives first-run setup end-to-end.
- [x] Step 1: capture agent objective in natural language.
- [x] Step 2: convert objective into proposed config, risk, and module plan.
- [x] Step 3: collect missing required variables with validation and rationale.
- [x] Step 4: confirm final plan, then trigger signature + provisioning.
- [x] Persist onboarding transcript as auditable intent artifact linked to `session_id`.
- [x] Provide resumable onboarding state when users leave and return (within active session TTL window).

Acceptance criteria:
- Users can complete onboarding without manual form-hunting.
- Every onboarding step maps to a backend state transition and saved artifact.
- Abandoned onboarding sessions resume from last completed state.

### P0. Verification Observability (EigenCloud + Gateway)

- [x] Add structured event logging for every frontdoor state transition.
- [x] Log session metadata: `session_id`, wallet, verification backend, level, provisioning source, enclave app ID.
- [x] Expose admin-safe monitoring endpoint for verification status rollups.
- [x] Include verification latency and backend result in observability payloads.
- [x] Ensure redacted public monitor endpoint remains redacted.

Acceptance criteria:
- Operator can query last N sessions and distinguish: primary verified, fallback verified, degraded, failed.
- Logs include enough fields to reconstruct a session path without reading raw request bodies.

### P0. Generated Gateway TODO Visibility

- [x] Publish a typed TODO feed for each generated gateway session.
- [x] Categorize TODOs as `required` or `recommended` with explicit owner and action.
- [x] Attach each TODO to evidence fields (`session_id`, `provisioning_source`, `verification_level`, `module_state`, `control_state`).
- [ ] Render unresolved `required` TODOs at generated gateway entry before normal dashboard flow.
- [x] Keep TODO states machine-readable: `open`, `blocked`, `in_progress`, `resolved`.
- [x] Emit lifecycle snapshot events for TODO state transitions.

Acceptance criteria:
- Every generated gateway session has a deterministic TODO snapshot and transition history.
- Required TODOs are visible in both frontdoor session detail and generated gateway landing state.
- Resolving a TODO produces an auditable backend event with timestamp and actor.

### P1. Policy + Module Governance Hardening

- [x] Keep addon modules disabled by default across bootstrap and config suggestion paths.
- [ ] Enforce signer/policy gate before any live-trading action.
- [x] Add tests for custody-mode invariants (`user_wallet`, `operator_wallet`, `dual_mode`).
- [x] Add tests for profile-domain invariants (`general` default, addon-specific requirements).
- [ ] Ensure fallback verification chain remains active when primary backend is degraded.

Acceptance criteria:
- All live-order paths fail closed without required signer/policy authorization.
- Regression tests cover module enable/disable, custody constraints, and fallback verification behavior.

### P1. Operability + Deployment Discipline

- [x] Add a single script for Railway signed E2E verification (staging + production).
- [x] Gate deploy promotion on E2E script success.
- [x] Add rollback playbook for failed provisioning or verification degradation.
- [x] Keep doctor surfaces actionable for frontdoor/provisioning misconfiguration.
- [x] Add alert thresholds for elevated `failed` session rates.

Acceptance criteria:
- Promotion checklist requires green lint/build/tests plus green signed E2E verification.
- Incident response path includes reproducible rollback steps with no manual guesswork.

### P1. EigenCloud Funding + Dedicated Account Readiness

- [x] Enforce preflight wallet-linked account checks before provisioning.
- [x] Validate gas + platform fee sufficiency before issuing final provision action.
- [x] Return deterministic insufficiency errors with exact missing requirement category (`gas`, `fee`, `auth`, `policy`).
- [ ] Bind spawned enclave to wallet-linked dedicated EigenCloud account identity.
- [x] Persist preflight/funding evidence in session timeline.

Acceptance criteria:
- Provisioning cannot start when funding preflight fails.
- Funding success and failure states are visible in frontdoor session and generated gateway TODO feed.

### P2. Product Quality and Trust Surface

- [ ] Improve policy assistant outputs to domain-specific profiles with explicit assumptions.
- [x] Add user-facing verification explanation panel (what was verified, by whom, and fallback status).
- [x] Add session timeline UI with deterministic milestones.
- [x] Add explicit post-launch runtime controls API (pause, resume, terminate, rotate auth key).
- [x] Add policy template library for common objectives.

Acceptance criteria:
- User can inspect session proof posture without opening server logs.
- Runtime control actions are auditable and reflected in session state history.

### P2. Experience System (Frontend + Backend Co-Design)

- [x] Define a strict component-purpose contract for all frontdoor UI components.
- [ ] Remove or block any component that does not map to a backend action or verifiable artifact.
- [x] Render flow as a deterministic state machine driven by backend session status, never by local-only assumptions.
- [x] Keep progressive disclosure: show only current step controls, unlock deeper controls by state.
- [x] Add interaction-rich proof UX: timeline, verification level badge, provisioning source, fallback posture, and runtime controls.
- [ ] Add domain-aware content modules that are concise, objective-oriented, and specific to user intent.
- [ ] Keep content blocks interactive and evidence-linked (click to inspect policy input, challenge payload class, verify outcome class).

Acceptance criteria:
- Every visible component has an explicit `purpose_id` and server contract mapping.
- Every user action emits a traceable backend event and updates deterministic state.
- Session screen explains outcome in under three surfaces:
1. What happened.
2. Why it was allowed or blocked.
3. What can be done next.

Implementation contract for each frontend component:
- `purpose_id`: stable semantic identifier.
- `user_value`: one-sentence reason the component exists.
- `backend_contract`: endpoint/event that powers the component.
- `artifact_binding`: intent/receipt/verification artifact generated or displayed.
- `state_inputs`: required session/config fields.
- `success_state`: explicit completion criteria.
- `failure_state`: explicit remediation path.

Required backend support for modular UX:
- [x] Publish a typed experience manifest that frontend consumes to render modules (`steps`, `capabilities`, `constraints`, `evidence_labels`).
- [x] Publish typed session timeline events with monotonic sequence IDs.
- [x] Publish typed verification explanation payload (`backend`, `level`, `fallback_used`, `latency_ms`, `failure_reason`).
- [x] Publish typed runtime control result payloads (pause/resume/terminate/rotate).
- [x] Publish generated gateway TODO payloads (`todo_id`, `severity`, `status`, `owner`, `action`, `evidence_refs`).
- [x] Keep redacted public monitor payload and privileged operator payload separated by endpoint/auth.

UI architecture requirements:
- [x] Build frontdoor as composable modules (`Identity`, `Policy`, `Verification`, `Provisioning`, `RuntimeControls`, `Evidence`).
- [x] Keep module registry declarative so modules can be enabled/disabled without page rewrites.
- [ ] Keep addon modules visually and behaviorally optional by default.
- [ ] Keep visual system simple: one primary action per step, clear status hierarchy, low-noise motion only for state transitions.
- [ ] Keep interaction design captivating through state-aware transitions and live evidence updates, not decorative UI.

Quality gates for the experience system:
- [x] No dead-end actions (all blocked states return a deterministic next action).
- [x] No silent failures (every failed action returns typed error + operator hint).
- [x] No orphan data (every fetched payload is rendered in at least one meaningful surface).
- [x] No misleading success states (UI success only after terminal backend success state).

### P2. Unified Enclave Workspace UX (Codex-Class Operator Surface)

- [ ] Deliver a single all-in-one frontend for the spawned enclave runtime.
- [ ] Keep thread-centric chat as primary working surface.
- [ ] Expose private git storage tree panel connected to agent workspace context.
- [ ] Add `Skills` and `Automations` tabs at top-level navigation.
- [ ] Add upper-right toggles for `Logs`, `Action History`, and `Compute/Usage`.
- [ ] Keep runtime instance visibility: direct link/open state for EigenCloud instance URL.
- [ ] Keep settings entry anchored bottom-left with policy-safe controls.
- [ ] Maintain visual quality and responsiveness across desktop/mobile breakpoints.

Acceptance criteria:
- Users can move from onboarding completion to enclave workspace without context loss.
- Workspace exposes threads, storage tree, skills, automations, logs, history, and compute in one surface.
- Instance URL visibility and runtime health are always accessible within two interactions.

## Validation Gates

- `./scripts/verify-local.sh`
- `npm run lint`
- `cargo +1.92.0 fmt --all --check`
- `cargo +1.92.0 check`
- `cargo +1.92.0 clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo +1.92.0 test --workspace --all-features -- --nocapture --test-threads=1`
- `./scripts/verify-ecloud-foundation.sh <env-file>`
- `./scripts/release-demo-readiness.sh <env-file>`
- `scripts/verify-frontdoor-railway-signed-e2e.sh` is implemented and used in release readiness gates.

## Exit Criteria

- Dedicated per-session EigenCloud provisioning is the primary verified path.
- Frontdoor and module/org APIs remain contract-stable.
- Verification lineage and fallback policy are observable and auditable.
- Hyperliquid and EigenDA remain optional addons disabled by default.
- No legacy app dependencies or legacy naming in active code or docs.
