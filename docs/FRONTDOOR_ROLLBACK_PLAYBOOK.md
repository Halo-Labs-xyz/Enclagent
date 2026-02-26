# Frontdoor Rollback Playbook

Status: Active
Last updated: 2026-02-26

## Scope

Use this playbook when frontdoor provisioning or verification posture regresses in staging or production:

- Provisioning failures spike (`status=failed`, `provisioning_source=unconfigured|unknown`, command errors).
- Verification posture degrades unexpectedly (`fallback_only_disabled`, invalid fallback settings, degraded receipts).

## Inputs Required

- Railway environment file for target service.
- Last known-good deploy SHA for target service.
- Known-good fallback instance URL (if forcing static fallback mode).

## 1. Confirm Incident and Freeze Promotion

1. Run signed E2E gate for the affected environment:
```bash
bash ./scripts/verify-frontdoor-railway-signed-e2e.sh <env-file>
```
2. Run release readiness check to capture failing stage:
```bash
./scripts/release-demo-readiness.sh <env-file>
```
3. Freeze promotions until rollback validation is green.

## 2. Classify Failure Mode

1. Provisioning failure indicators:
- `provisioning_source` not `command` when command path is expected.
- Sessions fail with command template/fallback misconfiguration errors.
2. Verification degradation indicators:
- Verification level/posture drift from expected signed fallback guarantees.
- Funding/auth checks fail due verification auth material.

## 3. Execute Rollback

## 3A. Fast containment: force explicit static fallback path

Use when command provisioning is unstable and service restoration is urgent.

1. Set environment:
```bash
GATEWAY_FRONTDOOR_PROVISION_COMMAND=
GATEWAY_FRONTDOOR_ALLOW_DEFAULT_INSTANCE_FALLBACK=true
GATEWAY_FRONTDOOR_DEFAULT_INSTANCE_URL=<known_good_gateway_url>
```
2. Keep verification fallback safety enabled:
```bash
GATEWAY_FRONTDOOR_REQUIRE_PRIVY=true
```
3. Redeploy service.

## 3B. Full deploy rollback: revert service to known-good SHA

Use when regression is code-level (not only env misconfiguration).

1. Roll back target service to last known-good deploy.
2. Re-apply known-good frontdoor env set (including provisioning + verification keys).
3. Redeploy and wait for health endpoints to stabilize.

## 4. Post-Rollback Validation (Mandatory)

1. Re-run signed E2E gate:
```bash
bash ./scripts/verify-frontdoor-railway-signed-e2e.sh <env-file>
```
2. Confirm monitor posture:
- New sessions reach terminal `ready`.
- `verification_level` and fallback posture match expected policy.
- `funding_preflight_status` not regressed for baseline path.
3. Run lint/test gate locally before any new promotion:
```bash
npm run lint
cargo test frontdoor::tests --lib
```

## 5. Return-to-Command Path Checklist

Only after stable fallback operation:

1. Restore `GATEWAY_FRONTDOOR_PROVISION_COMMAND` with validated template.
2. Keep `GATEWAY_FRONTDOOR_ALLOW_DEFAULT_INSTANCE_FALLBACK` only as explicit fallback policy.
3. Validate doctor output is clean:
```bash
cargo run -- doctor
```
4. Re-run signed E2E gate in staging, then production.

## Exit Criteria

Rollback is complete only when:

- Signed E2E gate passes.
- Frontdoor session monitor shows healthy terminal readiness.
- Verification fallback posture remains policy-compliant and auditable.
