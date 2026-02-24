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
