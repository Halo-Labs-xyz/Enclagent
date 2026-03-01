# Enclagent Frontdoor Enclave Flow

This document defines the user-facing launch flow for the shared Enclagent gateway when frontdoor mode is enabled.

## Goal

Expose a production entrypoint where users:

1. Connect an EVM wallet in frontdoor UI and bind the wallet to session identity.
2. Sign a mandatory gasless authorization challenge (no SIWE token handshake).
3. Submit validated runtime config (manual form and/or suggest-config API).
4. Pass funding preflight checks before provisioning can start.
5. Trigger provisioning via command backend or static fallback URL.
6. Poll session state and redirect when the session reaches `ready`.

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
GATEWAY_FRONTDOOR_ALLOW_DEFAULT_INSTANCE_FALLBACK=false
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

## Identity Entry Requirements

- Current frontdoor UI provides a single Privy connect button with provider chooser semantics (`wallet`, `email`, social OAuth providers) and enforces wallet binding before launch.
- Privy provider config enables Ethereum embedded wallet auto-creation for users without wallets (`embeddedWallets.ethereum.createOnLogin=users-without-wallets`).
- Wallet connect remains non-SIWE for frontdoor challenge flow: users connect injected EVM wallet, then complete the gateway gasless authorization challenge signature.
- Email/social entry paths authenticate through Privy (email OTP or OAuth) and converge on embedded wallet provisioning before provisioning is allowed.
- Launchpad now binds signing provider from Privy wallet state first (`useWallets`/`getEthereumProvider`) and only falls back to `window.ethereum` when Privy wallet provider is unavailable.
- When `privy_app_id` is present, connect flow initializes Privy client before provider execution so identity paths are deterministic.
- When `GATEWAY_FRONTDOOR_REQUIRE_PRIVY=true`, bootstrap requires resolved `privy_app_id`; launch is blocked if missing.
- `privy_user_id` is carried as wallet-linked identity metadata and validated on verify when present.
- Deterministic launch path in this branch:
1. wallet connect
2. challenge issuance
3. onboarding objective + plan confirmation (`confirm plan`)
4. onboarding signature confirmation (`confirm sign`)
5. challenge signature verification
6. funding preflight
7. provisioning path decision from objective:
   - shared-runtime intent can use `default_instance_url` fallback (no dedicated spin-up) when fallback is enabled
   - otherwise command provisioning runs (dedicated enclave spin-up)

## Conversational Onboarding Requirements

- Backend onboarding APIs are implemented:
1. `GET /api/frontdoor/onboarding/state?session_id=<uuid>`
2. `POST /api/frontdoor/onboarding/chat`
- Implemented onboarding state machine:
1. `capture_objective`
2. `propose_plan`
3. `collect_required_variables`
4. `confirm_and_sign`
5. `ready_to_sign`
- Transcript and step state are persisted per active session and exposed in onboarding state/timeline responses.
- Frontend typed session panel now wires onboarding state + chat and renders transcript, timeline, verification explanation, runtime controls, TODO posture, and funding preflight evidence.
- Frontend launch flow now drives onboarding step-4 deterministically before `POST /api/frontdoor/verify`:
1. captures objective
2. fills required variables from launch config (`profile_name`, `accept_terms`, and non-secret `gateway_auth_key=__from_config__` marker)
3. executes `confirm plan`
4. executes `confirm sign`
- Deterministic module state cards expose dropdown details and pop-out inspection to keep backend contracts and artifact bindings inspectable without leaving launch flow.
- Blocked launch states expose a deterministic next action hint; failed API actions surface typed failure code + operator hint.

### Chat-First Launchpad UX (Current)

- `/frontdoor` now serves a simplified chat-first launchpad surface.
- Launchpad UI source-of-truth is the TypeScript app in `launchpad/` and compiled output is embedded via `src/channels/web/static/launchpad*.js` assets served by gateway static routes.
- Launchpad bundle is emitted with hashed chunk names (`launchpad-<chunk>-<hash>.js`) to prevent stale chunk/runtime mismatches across local reloads and Railway deploys.
- Gateway static responses for launchpad assets are served with `Cache-Control: no-store` to force fresh fetches after each deploy.
- Identity boot starts from an explicit user action (`Sign Up / Connect Wallet`).
- First interaction always enforces identity bootstrap: Privy signup/login + wallet binding.
- If login completes without an embedded Ethereum wallet surfaced yet, launchpad triggers `useCreateWallet` as a deterministic fallback and blocks objective/provisioning steps until signer context is ready.
- Wallet-only browser-provider fallback is disabled in launchpad flow; Privy identity is mandatory before objective/config/signature steps.
- Frontend surfaces include early browser error guards for known third-party injected wallet script failures (for example `evmAsk.js` `keccak_256` null-destructure) so launchpad flow remains operational.
- Polling status events are deduplicated and chat history is capped to prevent runaway DOM growth and UI freezes under long-running sessions.
- User objective is collected in chat before any provisioning action.
- Gateway calls `suggest-config` and launchpad then requests a blueprint through the gateway OpenAI-compatible APIs (`GET /v1/models`, `POST /v1/chat/completions`) to use the active Anthropic-compatible model (for example MiniMax via Anthropic-compatible base URL) with deterministic fallback when unavailable.
- `suggest-config` now auto-generates `profile_name` from user intent + connected wallet (unless an explicit non-default profile name is supplied) so dedicated app names are user/objective-specific.
- Blueprint response includes a Mermaid graph plus markdown seed content for `IDENTITY.md` and `MISSION.md`; launchpad stores these under `config.domain_overrides` as launch-time seed artifacts.
- Mermaid graph blocks in assistant messages are rendered into visual diagrams directly in chat (with fallback to source graph text if render fails).
- Launchpad renders the interactive progression CTA (continue/retry) inline in the chat stream to keep the setup flow action-driven inside message context.
- Launchpad derives and displays runtime decision (`shared` fallback posture vs `dedicated` enclave posture) before challenge/sign.
- During `launching`/`provisioning`, launchpad switches from chat panel to a terminal-mode stream fed by timeline events (`/api/frontdoor/session/{session_id}/timeline`) and renders tagged provision logs (Railway/eCloud/provision channels) in near-real time.
- While eCloud builds are in-flight, frontdoor emits heartbeat provisioning logs every ~20s so long verifiable builds are observable instead of appearing stalled.
- No instance is provisioned until explicit user confirmation and successful wallet signature.
- When session status transitions to `ready`, launchpad now auto-redirects to `instance_url` (or `verify_url` fallback). Embedded launchpad posts a redirect bridge message to parent gateway and also applies top-window fallback redirect.

## Funding + Dedicated Provisioning Requirements

- Before provision command execution, perform a funding preflight for wallet-linked dedicated EigenCloud account readiness.
- Preflight must classify failure reason as `gas`, `fee`, `auth`, or `policy`.
- Provision action is blocked until preflight is successful.
- Preflight results must be visible in session detail and generated gateway TODO state.

### Command Provisioning

`GATEWAY_FRONTDOOR_PROVISION_COMMAND` is executed by `/bin/sh -c`.

The command **must print** provisioning output to stdout:

- Plain URL line is accepted.
- JSON is preferred to return both gateway URL and Eigen verify URL.
- Supported JSON keys:
  - `instance_url` / `url` / `gateway_url`
  - `app_url`
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
- Provision command stdout/stderr lines are streamed into session timeline `provision_log` events for launchpad terminal rendering; timeline retains the latest bounded window to avoid unbounded memory growth.

Example:

```bash
ECLOUD_FRONTDOOR_IMAGE_REF=docker.io/your-org/enclagent:latest
ECLOUD_FRONTDOOR_ENV_FILE=/app/deploy/ecloud-instance.env
ECLOUD_FRONTDOOR_FORCE_VERIFIABLE=true
ECLOUD_FRONTDOOR_SOURCE_REPO_URL=https://github.com/halo-labs-xyz/Enclagent
ECLOUD_FRONTDOOR_SOURCE_COMMIT=<git-sha>
ECLOUD_FRONTDOOR_STRICT_SOURCE_PROVENANCE=true
GATEWAY_FRONTDOOR_ECLOUD_APP_BASE_URL=https://sepolia.eigencloud.xyz/app
ECLOUD_FRONTDOOR_STRICT_INSTANCE_INIT=false
GATEWAY_FRONTDOOR_PROVISION_COMMAND=/app/scripts/provision-user-ecloud.sh --wallet '{wallet_address}' --session '{session_id}' --config '{config_b64}'
```

The bundled provisioner script also:

- Sets per-instance `GATEWAY_AUTH_TOKEN` to the user-provided `gateway_auth_key`.
- Applies runtime/wallet env overrides from frontdoor config before deploy.
- Carries forward base runtime/LLM env (`DATABASE_*`, `LLM_BACKEND`, `ANTHROPIC_*`, etc.) from the frontdoor service process so per-session instances do not inherit template placeholders.
- Uses non-interactive deploy mode. Set `ECLOUD_FRONTDOOR_FORCE_VERIFIABLE=true` to require `--verifiable` image/source checks during app deploy.
- Uses a slim verifiable runtime build profile (`Dockerfile` + `Dockerfile.ecloud`: `--profile ecloud --no-default-features --features libsql`; `profile.ecloud` uses lower optimization + high codegen units) so EigenCloud source builds match enclave runtime database mode and stay within upstream build-duration limits.
- Retries verifiable deploy on EigenCloud throttling/queue conflicts (`429 Too Many Requests`, `409 build already in progress`) using queue-safe fixed backoff with bounded retry budget (defaults: `ECLOUD_FRONTDOOR_DEPLOY_MAX_RETRIES=24`, `ECLOUD_FRONTDOOR_DEPLOY_RETRY_BACKOFF_SECS=15`, `ECLOUD_FRONTDOOR_DEPLOY_RETRY_TIMEOUT_SECS=900`; set `ECLOUD_FRONTDOOR_DEPLOY_MAX_RETRIES=0` to disable attempt-cap and rely on timeout budget only).
- On `409` queue conflicts, the provisioner emits explicit EigenCloud build-queue context (`status`, `build_id`, `repo`, `git_ref`, timestamps) to timeline logs so operators can correlate frontdoor retries to the currently active upstream verifiable build.
- When a queue/throttle response references an in-flight `build_id`, the provisioner waits on that build status (instead of repeatedly re-submitting) and only retries app deploy after the build reaches terminal state.
- If the queued `build_id` belongs to a different repo/commit than the requested source provenance, provisioner treats it as queue contention, skips build-wait for that `build_id`, and continues retries with backoff instead of stalling the session on unrelated builds.
- When verifiable mode is enabled, set `ECLOUD_FRONTDOOR_SOURCE_REPO_URL` + `ECLOUD_FRONTDOOR_SOURCE_COMMIT` so EigenCloud provenance points to the correct GitHub source.
- Falls back to verify portal URL when direct gateway health/import seeding fails; set `ECLOUD_FRONTDOOR_STRICT_INSTANCE_INIT=true` to fail hard instead.
- Returns `app_url` when available so frontdoor can surface distinct runtime/app/verify links instead of duplicating verify URLs.
- If eCloud omits `App URL`, derives runtime `app_url` from verify URL + `app_id` (`verify-sepolia` -> `sepolia`) so runtime endpoint does not collapse to verify endpoint.
- Forces wallet association by defaulting `wallet_vault_policy.user_wallet_address` to the connected wallet when not explicitly set.
- Applies verification-backend overrides (`verification_backend.*`) for each spawned enclave.
- Waits for instance health, then imports full settings (including copytrading + verification policy) via `/api/settings/import`.
- Returns the gateway token in the URL fragment (`#token=...`) instead of query string to reduce token leakage in request logs/referrers.

### Static Fallback URL

If dynamic provisioning is not enabled, set:

```bash
GATEWAY_FRONTDOOR_DEFAULT_INSTANCE_URL=https://verify-sepolia.eigencloud.xyz/app/<app-id>
GATEWAY_FRONTDOOR_ALLOW_DEFAULT_INSTANCE_FALLBACK=true
```

Default fallback behavior is fail-closed. `GATEWAY_FRONTDOOR_ALLOW_DEFAULT_INSTANCE_FALLBACK=true` is required to permit static fallback URL usage.

## API Contract

Public (no gateway token required):

- `GET /api/frontdoor/config-contract`
- `GET /api/frontdoor/policy-templates`
- `GET /api/frontdoor/experience/manifest`
- `GET /api/frontdoor/bootstrap`
- `GET /api/frontdoor/onboarding/state?session_id=<uuid>`
- `POST /api/frontdoor/onboarding/chat`
- `POST /api/frontdoor/challenge`
- `POST /api/frontdoor/suggest-config`
- `POST /api/frontdoor/verify`
- `GET /api/frontdoor/session/{session_id}`
- `GET /api/frontdoor/session/{session_id}/timeline`
- `GET /api/frontdoor/session/{session_id}/verification-explanation`
- `GET /api/frontdoor/session/{session_id}/gateway-todos`
- `GET /api/frontdoor/session/{session_id}/funding-preflight`
- `GET /api/frontdoor/sessions?wallet_address=<0x...>&limit=<n>` (wallet filter required, `limit` clamped to `1..100`)

Gateway APIs when frontdoor mode is enabled:

- `POST /api/frontdoor/session/{session_id}/runtime-control` (requires gateway auth token)
- `GET /api/frontdoor/operator/sessions?wallet_address=<0x...>&limit=<n>` (full session payloads)
- `GET /api/gateway/todos?wallet_address=<0x...>&session_id=<uuid>&limit=<n>` (aggregated TODO feeds)

In frontdoor mode, frontdoor onboarding/session-read APIs stay public for launch flow, while operator/control-plane APIs remain bearer-token protected.

Session status/monitor responses include provisioning audit fields:

- `provisioning_source` (`command`, `default_instance_url`, `unconfigured`, `unknown`)
- `dedicated_instance` (true when command-based per-session provisioning path is used)
- `launched_on_eigencloud` (heuristic from returned app id/URL surfaces)
- `verification_backend`
- `verification_level` (`primary_only`, `primary_plus_signed_fallback`, `signed_fallback_only`, etc.)
- `verification_fallback_enabled`
- `verification_fallback_require_signed_receipts`
- `runtime_state` (`running`, `paused`, `terminated`)
- `funding_preflight_status` (`pending`, `passed`, `failed`)
- `funding_preflight_failure_category` (`gas`, `fee`, `auth`, `policy`)

Generated gateway state should also surface TODO readiness fields:

- `todo_open_required_count`
- `todo_open_recommended_count`
- `todo_status_summary`

`/api/frontdoor/sessions` returns redacted summary rows (no session UUID, instance/app/verify URLs) for safer public monitoring.

`POST /api/frontdoor/session/{session_id}/runtime-control` accepts:

- `pause`
- `resume`
- `terminate`
- `rotate_auth_key`

The root UI (`/`) is always the main gateway entry.

When frontdoor mode is enabled:

- Launchpad onboarding is integrated as a gateway tab (`Launchpad`) and loaded from `/frontdoor` within the gateway.
- `/frontdoor` remains available as a standalone route for embedded/legacy access.
- `/gateway` is a backward-compatible path that redirects to `/`.

## Railway Signed E2E Gate

Use the canonical signed-flow verifier for staging + production:

```bash
bash ./scripts/verify-frontdoor-railway-signed-e2e.sh <env-file>
```

Current deployed fallback topology contract:

- `FRONTDOOR_E2E_REQUIRED_PROVISIONING_SOURCE=default_instance_url`
- `FRONTDOOR_E2E_REQUIRE_DEDICATED_INSTANCE=false`
- `FRONTDOOR_E2E_REQUIRE_LAUNCHED_ON_EIGENCLOUD=false`

Incident rollback runbook is maintained at:

- `docs/FRONTDOOR_ROLLBACK_PLAYBOOK.md`

Post-provision target UX requirements:

- Redirect destination should open a unified enclave workspace (thread-first interaction).
- Workspace should expose integrated surfaces for:
1. threads/chat
2. private storage tree
3. skills
4. automations
5. logs/action history/compute usage
6. settings

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

## Validation Gates (Exact, Server-Enforced)

- `wallet_address` and session wallet fields must be `0x`-prefixed 40-hex EVM addresses.
- `signature` must be a `0x`-prefixed 65-byte hex payload; recovered signer must match `wallet_address`.
- `session_id` must be UUID; verify requires exact challenge message match and unexpired challenge.
- `config_version` must be one of `1` or `2`.
- `profile_domain` must be normalized lowercase and match `[a-z0-9_-]` with max length `32`.
- `domain_overrides` is limited to `32` keys; each key max `64` chars; each value max serialized size `4096` bytes.
- `profile_name` is required and max `64` chars.
- `gateway_auth_key` must be printable ASCII, no whitespace, length `16..128`.
- `verification_backend` must be `eigencloud_primary` or `fallback_only`.
- `verification_eigencloud_auth_scheme` must be `bearer` or `api_key`.
- `verification_eigencloud_timeout_ms` must be `1..120000`.
- `verification_backend=fallback_only` requires `verification_fallback_enabled=true`.
- `verification_fallback_signing_key_id` max length is `128`; `verification_fallback_chain_path` must not contain newlines.
- `accept_terms` must be `true`.
- For `hyperliquid` domain: network/policy enums are enforced, request/risk bounds are validated, symbol lists are normalized and disjoint, and `mainnet + live_allowed` caps `max_position_size_usd` at `1_000_000`.

## Gasless Signature Notes

Frontdoor launch authentication is wallet-signature based and does not require SIWE identity/access tokens.

- The launch proof uses an EIP-191 personal-sign message labeled as a gasless authorization transaction.
- Client signing retries include both payload encodings (UTF-8 hex and plain text) and both parameter orders (`[message, address]` and `[address, message]`) for wallet-provider compatibility.
- Server-side verification recovers the signer wallet address from the signature and rejects mismatches.

If users still see signature verification errors:

- Confirm the connected wallet account matches the challenge wallet address.
- Confirm browser wallet extensions are not rewriting the signed payload.
