# Enclagent Frontdoor Enclave Flow

This document defines the user-facing launch flow for the shared Enclagent gateway when frontdoor mode is enabled.

## Goal

Expose a production entrypoint where users:

1. Connect wallet (Privy-mode EVM wallet flow, no SIWE token handshake).
2. Sign a mandatory gasless authorization challenge.
3. Complete required risk/runtime configuration.
4. Trigger per-user enclave provisioning.
5. Get redirected to their personal instance URL when ready.

## Enable Frontdoor

Set the gateway env flags:

```bash
GATEWAY_FRONTDOOR_ENABLED=true
GATEWAY_FRONTDOOR_REQUIRE_PRIVY=true
GATEWAY_FRONTDOOR_PRIVY_APP_ID=<privy_app_id>
GATEWAY_FRONTDOOR_PRIVY_CLIENT_ID=<privy_client_id>   # optional
GATEWAY_FRONTDOOR_SESSION_TTL_SECS=900
GATEWAY_FRONTDOOR_POLL_INTERVAL_MS=1500
GATEWAY_FRONTDOOR_VERIFY_APP_BASE_URL=https://verify-sepolia.eigencloud.xyz/app
```

Privy environment resolution order:

1. `GATEWAY_FRONTDOOR_PRIVY_APP_ID` (recommended canonical key)
2. `FRONTDOOR_PRIVY_APP_ID`
3. `PRIVY_APP_ID`
4. `NEXT_PUBLIC_PRIVY_APP_ID`

Client ID resolution order:

1. `GATEWAY_FRONTDOOR_PRIVY_CLIENT_ID` (recommended canonical key)
2. `FRONTDOOR_PRIVY_CLIENT_ID`
3. `PRIVY_CLIENT_ID`
4. `NEXT_PUBLIC_PRIVY_CLIENT_ID`

For Railway staging and production services, use canonical `GATEWAY_FRONTDOOR_*` keys in both environments and reserve aliases only for migration from legacy deploys.

Configure one provisioning path:

1. Command-based provisioning (recommended for per-user enclave creation)
2. Static fallback URL (shared instance redirect)

### Command Provisioning

`GATEWAY_FRONTDOOR_PROVISION_COMMAND` is executed by `/bin/sh -c`.

The command **must print** provisioning output to stdout:

- Plain URL line is accepted.
- JSON is preferred to return both gateway URL and Eigen verify URL.
- Supported JSON keys:
  - `instance_url` / `url` / `gateway_url`
  - `verify_url` / `eigen_verify_url` / `eigen_app_url`
  - `app_id` / `eigen_app_id` (used with `GATEWAY_FRONTDOOR_VERIFY_APP_BASE_URL`)

Template placeholders:

- `{session_id}`
- `{wallet_address}`
- `{privy_user_id}`
- `{privy_identity_token}`
- `{privy_access_token}`
- `{chain_id}`
- `{version}`
- `{config_version}`
- `{profile_domain}`
- `{domain_overrides_json}`
- `{profile_name}`
- `{custody_mode}`
- `{operator_wallet_address}`
- `{user_wallet_address}`
- `{vault_address}`
- `{gateway_auth_key}`
- `{eigencloud_auth_key}`
- `{verification_backend}`
- `{verification_eigencloud_endpoint}`
- `{verification_eigencloud_auth_scheme}`
- `{verification_eigencloud_timeout_ms}`
- `{verification_fallback_enabled}`
- `{verification_fallback_signing_key_id}`
- `{verification_fallback_chain_path}`
- `{verification_fallback_require_signed_receipts}`
- `{verify_app_base_url}`
- `{inference_summary}`
- `{inference_confidence}`
- `{config_json}`
- `{config_b64}`

Placeholder substitution hardening:

- Placeholders are rendered as environment variable references before execution.
- Runtime values are injected via process env (not raw string interpolation) to prevent shell command injection from user-supplied config fields.
- Keep placeholders quoted in command templates when values may contain whitespace.

Example:

```bash
ECLOUD_FRONTDOOR_IMAGE_REF=docker.io/your-org/enclagent:latest
ECLOUD_FRONTDOOR_ENV_FILE=/app/deploy/ecloud-instance.env
GATEWAY_FRONTDOOR_PROVISION_COMMAND=/app/scripts/provision-user-ecloud.sh --wallet '{wallet_address}' --session '{session_id}' --config '{config_b64}'
```

The bundled provisioner script also:

- Sets per-instance `GATEWAY_AUTH_TOKEN` to the user-provided `gateway_auth_key`.
- Applies runtime/wallet env overrides from frontdoor config before deploy.
- Forces wallet association by defaulting `wallet_vault_policy.user_wallet_address` to the connected wallet when not explicitly set.
- Applies verification-backend overrides (`verification_backend.*`) for each spawned enclave.
- Waits for instance health, then imports full settings (including copytrading + verification policy) via `/api/settings/import`.
- Returns the gateway token in the URL fragment (`#token=...`) instead of query string to reduce token leakage in request logs/referrers.

### Static Fallback URL

If dynamic provisioning is not enabled, set:

```bash
GATEWAY_FRONTDOOR_DEFAULT_INSTANCE_URL=https://verify-sepolia.eigencloud.xyz/app/<app-id>
```

## API Contract

Public (no gateway token required):

- `GET /api/frontdoor/config-contract`
- `GET /api/frontdoor/bootstrap`
- `POST /api/frontdoor/challenge`
- `POST /api/frontdoor/suggest-config`
- `POST /api/frontdoor/verify`
- `GET /api/frontdoor/session/{session_id}`
- `GET /api/frontdoor/sessions?wallet_address=<0x...>&limit=<n>` (wallet filter required)

Session status/monitor responses include provisioning audit fields:

- `provisioning_source` (`command`, `default_instance_url`, `unconfigured`, `unknown`)
- `dedicated_instance` (true when command-based per-session provisioning path is used)
- `launched_on_eigencloud` (heuristic from returned app id/URL surfaces)
- `verification_backend`
- `verification_level` (`primary_only`, `primary_plus_signed_fallback`, `signed_fallback_only`, etc.)
- `verification_fallback_enabled`
- `verification_fallback_require_signed_receipts`

`/api/frontdoor/sessions` returns redacted summary rows (no session UUID, instance URL, or verify URL) for safer public monitoring.

The root UI (`/`) auto-switches to the frontdoor page when frontdoor mode is enabled.

Legacy gateway UI remains reachable at `/gateway` (token auth still required).

## Session Versioning

Each wallet increments a version counter (`v1`, `v2`, ...). Latest session metadata is persisted to:

`~/.enclagent/frontdoor/wallet_sessions.json`

Stored fields per wallet:

- latest version
- latest instance URL
- latest profile name
- update timestamp

## Required User Config

Before provisioning, users must supply:

- `config_version` (supported: `1`, `2`; current: `2`)
- `profile_domain` (current default: `general`)
- `profile_name`
- `custody_mode`
- `information_sharing_scope`
- `kill_switch_behavior`
- `gateway_auth_key`
- `verification_backend`
- `verification_eigencloud_auth_scheme`
- `verification_eigencloud_timeout_ms`
- `accept_terms=true`

Optional:

- `domain_overrides` (domain-scoped extensibility map)
- `inference_summary`
- `inference_confidence`
- `inference_warnings`
- `hyperliquid_network` (required when `profile_domain=hyperliquid`)
- `paper_live_policy` (required when `profile_domain=hyperliquid`)
- `request_timeout_ms` (required when `profile_domain=hyperliquid`)
- `max_retries` (required when `profile_domain=hyperliquid`)
- `retry_backoff_ms` (required when `profile_domain=hyperliquid`)
- `max_position_size_usd` (required when `profile_domain=hyperliquid`)
- `leverage_cap` (required when `profile_domain=hyperliquid`)
- `max_allocation_usd` (required when `profile_domain=hyperliquid`)
- `per_trade_notional_cap_usd` (required when `profile_domain=hyperliquid`)
- `max_leverage` (required when `profile_domain=hyperliquid`)
- `max_slippage_bps` (required when `profile_domain=hyperliquid`)
- `symbol_allowlist` (required when `profile_domain=hyperliquid`)
- `hyperliquid_api_base_url`
- `hyperliquid_ws_url`
- `symbol_denylist`
- `operator_wallet_address` (required for `operator_wallet`/`dual_mode`)
- `user_wallet_address` (required for `user_wallet`/`dual_mode`)
- `vault_address`
- `eigencloud_auth_key`
- `enable_memory`
- `verification_eigencloud_endpoint`
- `verification_fallback_enabled` (must be true if `verification_backend=fallback_only`)
- `verification_fallback_signing_key_id`
- `verification_fallback_chain_path`
- `verification_fallback_require_signed_receipts`

Additional enforcement:

- If `custody_mode` is `user_wallet` or `dual_mode`, `user_wallet_address` must match the connected wallet.
- `GATEWAY_FRONTDOOR_REQUIRE_PRIVY=true` requires `GATEWAY_FRONTDOOR_PRIVY_APP_ID` to be set; otherwise launch is blocked.
- `POST /api/frontdoor/verify` cryptographically recovers signer address from the EIP-191 signed challenge and rejects mismatches.
- `POST /api/frontdoor/suggest-config` always returns server-validated suggestions that pass the same policy checks used by verify/provision.

## Gasless Signature Notes

Frontdoor launch authentication is wallet-signature based and does not require SIWE identity/access tokens.

- The launch proof uses an EIP-191 personal-sign message labeled as a gasless authorization transaction.
- Client signing retries include both payload encodings (UTF-8 hex and plain text) and both parameter orders (`[message, address]` and `[address, message]`) for wallet-provider compatibility.
- Server-side verification recovers the signer wallet address from the signature and rejects mismatches.

If users still see signature verification errors:

- Confirm the connected wallet account matches the challenge wallet address.
- Confirm browser wallet extensions are not rewriting the signed payload.
