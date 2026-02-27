# Enclagent Fork Status

## Current State

Enclagent is maintained as a standalone single-runtime repository with IronClaw-lineage runtime foundations.

Completed platform-direction updates:

- Re-anchored directives from Hyperliquid-first to verifiable-core + module suite.
- Added Core-8 module catalog and module state APIs.
- Added org workspace and membership control APIs.
- Added platform schema migrations for org/module/provisioning/skill-fork lineage.
- Added module-aware inference route enforcement in gateway chat path.
- Added execution-layer capability gating for commands/tools and ingress-agnostic policy checks.
- Added EigenDA frontdoor domain profile and addon-facing contract surface.
- Refined setup wizard onboarding copy and validation paths for Enclagent-specific runtime flows.
- Added typed frontdoor experience APIs: manifest, onboarding state/chat, session timeline, and verification explanation.
- Added typed policy template library API for common objective presets (`/api/frontdoor/policy-templates`).
- Added frontdoor runtime control API and typed gateway TODO feeds (per-session and aggregated operator view).
- Added funding preflight gating and categorized failure reporting (`gas`, `fee`, `auth`, `policy`) before provisioning.
- Added redacted public frontdoor monitor plus protected operator monitor endpoint split.
- Wired static frontdoor typed evidence surfaces (onboarding chat/transcript, timeline, verification explanation, runtime controls, TODO posture, funding preflight).
- Added Railway signed frontdoor E2E verification script and release-readiness gating integration.
- Promoted staging + production Railway services to the typed frontdoor surface (manifest + policy templates) and validated signed E2E parity under explicit static fallback posture.
- Upgraded frontdoor identity UX to a single Privy provider-chooser connect path with wallet/email/social entry and mandatory wallet-binding convergence before provisioning.
- Added actionable `doctor` validation for frontdoor provisioning misconfiguration (Privy requirements, command-template validity, fallback URL validity, backend fail-closed checks).
- Added focused frontdoor config invariants tests for custody modes and profile-domain/addon-domain normalization behavior.
- Wired launch-time onboarding step-4 automation (`confirm plan` + `confirm sign`) before signature verification, including non-secret gateway auth marker handling for transcript safety.
- Added deterministic next-action remediation and typed failure/operator-hint rendering across frontdoor UI action paths.
- Added explicit frontdoor rollback playbook for provisioning or verification degradation incidents.
- Added enclave swarm orchestration runbook for OpenClaw-style multi-agent delivery on IronClaw-lineage instances.
- Added ROMA fork integration blueprint for optional sidecar decomposition routing into Enclagent job and verification surfaces.

## Runtime Scope

- Rust runtime and CLI
- Embedded web gateway
- Embedded frontdoor API and static UI
- Module catalog/state and org APIs
- Deterministic intent/receipt/verification pipeline
- Optional addon modules:
  - Hyperliquid execution addon
  - EigenDA data-availability addon

## Deployment Scope

- Service + Docker assets under `deploy/`
- Local and release verification under `scripts/`

## Remaining Rule

Enclagent remains verifiability-first with strict policy gates and mandatory verification fallback posture for stable release readiness.
