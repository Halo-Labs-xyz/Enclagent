# Agent Rules

## Primary Directive

- Enclagent is a verifiable Hyperliquid agent kit.
- Build on the existing Enclagent runtime baseline while preserving privacy and security as non-negotiable baseline properties.
- Prioritize verifiability, sovereignty, and deterministic audit artifacts over feature breadth.
- Apply maximum diligence for all work in this directory: `MAKE NO MISTAKES.`

## Source-of-Truth PRD

- The living product directive is `docs/ENCLAGENT_VERIFIABLE_HL_AGENT_KIT_PRD.md`.
- The execution checklist is `docs/ENCLAGENT_E2E_TODO_PRD.md`.
- Treat this PRD as active implementation scope, not optional reference material.
- If scope, interfaces, or acceptance criteria change, update the PRD in the same branch.

## Decision Filters (Apply Before Any Change)

- Hyperliquid alignment: the change must directly support Hyperliquid-first operation or demo reliability.
- Verifiability alignment: the change must preserve or improve intent/receipt/proof traceability.
- Security alignment: do not weaken sandbox, secrets handling, policy gating, or prompt/tool safety controls.
- Operability alignment: status/doctor/monitoring and rollback behavior must remain intact or improve.

## Runtime Policy Requirements

- Keep paper-trading mode as the default execution mode unless explicitly overridden by policy.
- Do not permit live-order execution without explicit signer/policy gate checks.
- Persist verifiable intent and execution receipts for every agent action in scope.
- Keep fallback verification path available when primary verification backend is degraded.

## Feature Parity Update Policy

- If you change implementation status for any feature tracked in `FEATURE_PARITY.md`, update that file in the same branch.
- Do not open a PR that changes feature behavior without checking `FEATURE_PARITY.md` for needed status updates (`❌`, `🚧`, `✅`, notes, and priorities).

## Documentation Requirements

- When public utility behavior changes, update docs under `docs/` in the same branch.
- Keep `docs/ENCLAGENT_FORK_STATUS.md` synchronized with completed Hyperliquid-native milestones.

## Git Branch Strategy (Mandatory)

- `main` is the stable production branch and deployment source branch.
- Create one short-lived branch per scope/session; do not reuse branches across unrelated scopes.
- Open PRs targeting `main` only.
- Rebase PR branches on latest `main` before merge when `main` has advanced (`git fetch origin` + `git rebase origin/main`).
- Resolve conflicts in the PR branch, rerun required validation, and merge only after green checks.
- For parallel execution, assign non-overlapping file ownership per branch to minimize conflicts.
