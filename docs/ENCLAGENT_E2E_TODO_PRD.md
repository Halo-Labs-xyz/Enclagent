# Enclagent End-to-End TODO PRD

Status: Active execution checklist

## Objective

Deliver a production-grade Enclagent runtime that is Hyperliquid-first, verifiable, and operable as a single embedded gateway product.

## Milestones

1. Frontdoor contract stability
- Keep `/api/frontdoor/*` contract stable.
- Keep static frontdoor UI served by runtime.
- Keep provisioning placeholder injection contract documented and tested.

2. Execution lineage integrity
- Persist deterministic intent/receipt/verification records.
- Keep verification fallback chain active and queryable.
- Expose verification status in runtime and gateway surfaces.

3. Policy and safety
- Enforce paper-default and live-gate checks.
- Enforce custody/risk limits before any execution path.
- Keep kill-switch behavior deterministic and auditable.

4. Operability
- Keep startup doctor and status surfaces actionable.
- Keep deployment scripts and service units Enclagent-only.
- Keep rollback and restart flow documented and repeatable.

5. Validation
- `cargo fmt --all --check`
- `cargo check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features -- --nocapture`
- `./scripts/verify-local.sh`

## Exit Criteria

- No legacy app dependencies.
- No legacy naming in active code or docs.
- Frontdoor, verification lineage, and policy gates validated.
