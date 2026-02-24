# Enclagent End-to-End TODO PRD

Status: Active execution checklist

## Objective

Deliver a production-grade Enclagent runtime that is verifiable-core, module-governed, and operable as a single embedded gateway product.

## Milestones

1. Frontdoor and provisioning contract stability
- Keep `/api/frontdoor/*` contract stable.
- Keep static frontdoor UI served by runtime.
- Keep provisioning placeholder injection contract documented and tested.
- Keep frontdoor contract module-aware (`general` default + addon-friendly).

2. Module governance
- Keep Core-8 catalog available from `/api/modules/catalog`.
- Keep module state persistence and protected toggle/config APIs operational.
- Keep addon modules (`hyperliquid_addon`, `eigenda_addon`) disabled by default.
- Keep inference-route module enforcement active in gateway chat ingestion.

3. Org workspace controls
- Keep `owner/admin/member` model operational in protected org APIs.
- Keep per-workspace membership and role transitions auditable.

4. Execution lineage integrity
- Persist deterministic intent/receipt/verification records.
- Keep verification fallback chain active and queryable.
- Expose verification status in runtime and gateway surfaces.

5. Policy and safety
- Enforce signer/policy checks before any live trading action when addon is enabled.
- Enforce sandbox/custody/risk controls where trading module is active.
- Keep kill-switch behavior deterministic and auditable.

6. Operability
- Keep startup doctor and status surfaces actionable.
- Keep deployment scripts and service units Enclagent-only.
- Keep rollback and restart flow documented and repeatable.

7. Validation
- `cargo fmt --all --check`
- `cargo check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features -- --nocapture`
- `./scripts/verify-local.sh`

## Exit Criteria

- Frontdoor, module APIs, and org APIs validated.
- Verification lineage and fallback policy validated.
- Hyperliquid and EigenDA are optional addons and disabled by default.
- No legacy app dependencies.
- No legacy naming in active code or docs.
