# Setup / Onboarding Specification

This document is the authoritative specification for Enclagent's onboarding
wizard. Any code change to `src/setup/` **must** keep this document in sync.
If a future contributor or coding agent modifies setup behavior, update this
file first, then adjust the code to match.

---

## Entry Points

```
enclagent onboard [--skip-auth] [--channels-only]
```

Explicit invocation. Loads `.env` files, runs the wizard, exits.

```
enclagent          (first run, no database configured)
```

Auto-detection via `check_onboard_needed()` in `main.rs`. Triggers when
none of these are true:
- `DATABASE_URL` env var is set
- `LIBSQL_PATH` env var is set
- `~/.enclagent/enclagent.db` exists on disk

The `--no-onboard` CLI flag suppresses auto-detection.

---

## Startup Sequence (main.rs)

```
1. Parse CLI args
2. If Command::Onboard  → load .env, run wizard, exit
3. If Command::Run or no command:
   a. Load .env files (dotenvy::dotenv() then load_enclagent_env())
   b. check_onboard_needed() → run wizard if needed
   c. Config::from_env()     → build config from env vars
   d. Create SessionManager  → load session token
   e. Attach secrets store and inject provider API keys from encrypted secrets
   f. Re-resolve config from DB/TOML after key injection
   g. ensure_authenticated() only when backend is NEAR AI and NEAR API key is absent
   h. ... rest of agent startup
```

**Critical ordering:** `.env` files must be loaded (step 3a) before
`Config::from_env()` (step 3c) because bootstrap vars like
`DATABASE_BACKEND` live in `~/.enclagent/.env`.

---

## The 10-Step Wizard

### Overview

```
Step 1: Database Connection
Step 2: Security (master key)
Step 3: Inference Provider          ← skipped if --skip-auth
Step 4: Model Selection
Step 5: Embeddings
Step 6: Channel Configuration
Step 7: Background Tasks (heartbeat)
Step 8: Hyperliquid Runtime Profile
Step 9: Wallet and Vault Policy
Step 10: Verification Backend
       ↓
   save_and_summarize()
```

`--channels-only` mode runs only Step 6, skipping everything else.

---

### Step 1: Database Connection

**Module:** `wizard.rs` → `step_database()`

**Goal:** Select backend, establish connection, run migrations.

**Decision tree:**

```
Both features compiled?
├─ Yes → DATABASE_BACKEND env var set?
│  ├─ Yes → use that backend
│  └─ No  → interactive selection (PostgreSQL vs libSQL)
├─ Only postgres feature → step_database_postgres()
└─ Only libsql feature  → step_database_libsql()
```

**PostgreSQL path** (`step_database_postgres`):
1. Check `DATABASE_URL` from env or settings
2. Test connection (creates `deadpool_postgres::Pool`)
3. If connection is unavailable, offer local Docker bootstrap:
   - Run `docker compose up -d postgres` when `docker-compose.yml` is discoverable
   - Fallback to a managed `enclagent-postgres` container when compose file is absent
   - Wait for readiness, then use `postgres://enclagent:enclagent@localhost:5432/enclagent`
4. Optionally run refinery migrations (manual URL path) or run migrations automatically (Docker bootstrap path)
5. Store pool in `self.db_pool`

**libSQL path** (`step_database_libsql`):
1. Offer local path (default: `~/.enclagent/enclagent.db`)
2. Optional Turso cloud sync (URL + auth token)
3. Test connection (creates `LibSqlBackend`)
4. Always run migrations (idempotent CREATE IF NOT EXISTS)
5. Store backend in `self.db_backend`

**Invariant:** After Step 1, exactly one of `self.db_pool` or
`self.db_backend` is `Some`. This is required for settings persistence
in `save_and_summarize()`.

After Step 1, the wizard hydrates existing DB-backed settings (if present)
before Step 2 so all "Keep current ..." prompts reflect persisted values.

---

### Step 2: Security (Master Key)

**Module:** `wizard.rs` → `step_security()`

**Goal:** Configure encryption for API tokens and secrets.

**Decision tree:**

```
SECRETS_MASTER_KEY env var set?
├─ Yes → use env var, done
└─ No  → try get_master_key() from OS keychain
   ├─ Ok(bytes) → cache in self.secrets_crypto, ask "use existing?"
   │  ├─ Yes → done (keychain)
   │  └─ No  → clear cache, fall through to options
   └─ Err   → fall through to options
              ├─ OS Keychain: generate + store + build SecretsCrypto
              ├─ Env variable: generate + print export command
              └─ Skip: disable secrets features
```

**CRITICAL CAVEAT: macOS Keychain Dialogs**

On macOS, `security_framework::get_generic_password()` can trigger TWO
system dialogs:
1. "Enter your password to unlock the keychain" (keychain locked)
2. "Allow enclagent to access this keychain item" (per-app authorization)

This is OS-level behavior we cannot prevent. To minimize pain:

- **Use `get_master_key()` not `has_master_key()`** in step 2. Both call
  the same underlying API, but `get_master_key()` returns the key bytes
  so we can cache them. `has_master_key()` throws them away, forcing a
  second keychain access later.

- **Build `SecretsCrypto` eagerly.** When the keychain key is retrieved,
  immediately construct `SecretsCrypto` and store in `self.secrets_crypto`.
  Later calls to `init_secrets_context()` check this field first, avoiding
  redundant keychain probes.

- **Never probe the keychain in read-only commands** (e.g., `enclagent status`).
  The status command reports "env not set (keychain may be configured)"
  rather than triggering system dialogs.

**Invariant:** After Step 2, `self.secrets_crypto` is `Some` if the user
chose Keychain or generated a new key. It may be `None` if the user chose
env-var mode or skipped secrets.

---

### Step 3: Inference Provider

**Module:** `wizard.rs` → `step_inference_provider()`

**Goal:** Choose LLM backend and authenticate.

**Providers:**

| Provider | Auth Method | Secret Name | Env Var |
|----------|-------------|-------------|---------|
| NEAR AI | Browser OAuth / session token | DB setting `nearai.session_token` (legacy fallback: `nearai.session`) | `NEARAI_SESSION_TOKEN` (legacy migration only) |
| Anthropic | API key | `llm_anthropic_api_key` | `ANTHROPIC_API_KEY` (+ optional `ANTHROPIC_BASE_URL` for Anthropic-compatible gateways) |
| OpenAI | API key | `llm_openai_api_key` | `OPENAI_API_KEY` |
| Ollama | None | - | - |
| OpenAI-compatible | Optional API key | `llm_compatible_api_key` | `LLM_API_KEY` |

**API-key providers** (`setup_api_key_provider`):
1. Check env var → if set, ask to reuse, persist to secrets store
2. Otherwise prompt for key entry via `secret_input()`
3. Store encrypted in secrets via `init_secrets_context()`
4. **Cache key in `self.llm_api_key`** for model fetching in Step 4

**NEAR AI** (`setup_nearai`):
- Calls `session_manager.ensure_authenticated()` which opens browser
- Session token saved to `~/.enclagent/session.json` and mirrored to DB as `nearai.session_token`

**`self.llm_api_key` caching:** The wizard caches the API key as
`Option<SecretString>` so that Step 4 (model fetching) and Step 5
(embeddings) can use it without re-reading from the secrets store or
mutating environment variables.

**Startup invariant:** The selected backend is persisted as `LLM_BACKEND`
in bootstrap env. Runtime NEAR session validation is executed only for
`LLM_BACKEND=nearai` without `NEARAI_API_KEY`.

---

### Step 4: Model Selection

**Module:** `wizard.rs` → `step_model_selection()`

**Goal:** Choose which model to use.

**Flow:**
1. If model already set → offer to keep it
2. Fetch models from provider API (5-second timeout)
3. On timeout or error → use static fallback list
4. Present list + "Custom model ID" escape hatch
5. Store in `self.settings.selected_model`
6. Provider-specific runtime uses this selected value as fallback model
   - Anthropic: `selected_model` is used when `ANTHROPIC_MODEL` env var is unset
   - OpenAI: `selected_model` is used when `OPENAI_MODEL` env var is unset
   - Ollama: `selected_model` is used when `OLLAMA_MODEL` env var is unset
   - OpenAI-compatible: `selected_model` is used when `LLM_MODEL` env var is unset

**Model fetchers pass the cached API key explicitly:**
```rust
let cached = self.llm_api_key.as_ref().map(|k| k.expose_secret().to_string());
let models = fetch_anthropic_models(cached.as_deref()).await;
```

This avoids mutating environment variables. The fetcher checks the explicit
key first, then falls back to the standard env var.

---

### Step 5: Embeddings

**Module:** `wizard.rs` → `step_embeddings()`

**Goal:** Configure semantic search for workspace memory.

**Flow:**
1. Ask "Enable semantic search?" (default: yes)
2. Detect available providers:
   - NEAR AI: if backend is `nearai` OR setup session manager is active
   - OpenAI: if `OPENAI_API_KEY` in env OR (backend is `openai` AND cached key)
3. Special case: backend `openai` with available key auto-selects OpenAI embeddings
4. Otherwise prompt provider selection from available options
5. If neither NEAR AI session path nor OpenAI key is available → disable embeddings

**Current provider scope:** Embeddings providers are `nearai` and `openai`.
Anthropic, Ollama, and OpenAI-compatible are valid LLM backends but currently
reuse NEAR/OpenAI embeddings options instead of provider-native embeddings.

**Default model:** `text-embedding-3-small` (for both providers)

---

### Step 6: Channel Configuration

**Module:** `wizard.rs` → `step_channels()`, delegating to `channels.rs`

**Goal:** Enable input channels (TUI, HTTP, Telegram, etc.).

**Sub-steps:**

```
6a. Tunnel setup (if webhook channels needed)
6b. Discover WASM channels from ~/.enclagent/channels/
6c. Multi-select: CLI/TUI, HTTP, discovered channels, bundled channels
6d. Install missing bundled channels (copy WASM binaries)
6e. Initialize SecretsContext (for token storage)
6f. Setup HTTP webhook (if selected)
6g. Setup each WASM channel (secrets, owner binding)
```

**Tunnel setup** (`setup_tunnel`):
- Options: ngrok, Cloudflare Tunnel, localtunnel, custom URL
- Validates HTTPS requirement
- Stored in `self.settings.tunnel.public_url`

**WASM channel setup** (`setup_wasm_channel`):
- Reads `capabilities.json` for `setup.required_secrets`
- For each secret: check existing, prompt or auto-generate, validate regex
- Save each secret via `SecretsContext`

**Telegram special case** (`setup_telegram`):
- Validates bot token via Telegram `getMe` API
- Owner binding: polls `getUpdates` for 120s to capture sender's user ID
- Optional webhook secret generation

**SecretsContext creation** (`init_secrets_context`):
1. Check `self.secrets_crypto` (set in Step 2) → use if available
2. Else try `SECRETS_MASTER_KEY` env var
3. Else try `get_master_key()` from keychain (only in `channels_only` mode)
4. Create backend-appropriate secrets store (respects selected database backend)

---

### Step 7: Heartbeat

**Module:** `wizard.rs` → `step_heartbeat()`

**Goal:** Configure periodic background execution.

**Flow:**
1. Ask "Enable heartbeat?" (default: no)
2. If yes: interval in minutes (default: 30), notification channel
3. Store in `self.settings.heartbeat`

---

### Step 8: Hyperliquid Runtime Profile + Copytrading Initialization

**Module:** `wizard.rs` → `step_hyperliquid_runtime_profile()`

**Goal:** Configure Hyperliquid network targets, resiliency, paper/live policy, and fixed copytrading guardrails.

**Flow:**
1. Normalize current values (`network`, `paper_live_policy`) with safe defaults:
   - network default: `testnet`
   - policy default: `paper_first`
2. Ask to keep current profile (default: yes)
3. Prompt network (`testnet` / `mainnet`, aliases normalized)
4. Configure endpoints:
   - Keep network defaults (stores `api_base_url=None`, `ws_url=None`)
   - Or set explicit overrides (must be `http(s)` for API and `ws(s)` for WebSocket)
5. Prompt request timeout in milliseconds (`> 0`)
6. Prompt max retries (`>= 0`)
7. Prompt retry backoff in milliseconds (`>= 0`)
8. Prompt paper/live policy (`paper_only`, `paper_first`, `live_allowed`)
9. Safety confirmation for `mainnet + live_allowed`; decline forces `paper_first`
10. Persist to `self.settings.hyperliquid_runtime`
11. Configure WS-10 copytrading profile:
    - max mirrored allocation (USD)
    - per-trade notional cap (USD)
    - max leverage
    - symbol allowlist / denylist
    - max slippage (bps)
    - information-sharing scope (`none`, `signals_only`, `signals_and_execution`, `full_audit`)
12. Persist to `self.settings.copytrading`

**Network defaults (used when overrides are unset):**
- `testnet`: `https://api.hyperliquid-testnet.xyz`, `wss://api.hyperliquid-testnet.xyz/ws`
- `mainnet`: `https://api.hyperliquid.xyz`, `wss://api.hyperliquid.xyz/ws`

---

### Step 9: Wallet and Vault Policy

**Module:** `wizard.rs` → `step_wallet_vault_policy()`

**Goal:** Configure custody mode and trading risk guardrails.

**Flow:**
1. Normalize current values with conservative defaults:
   - custody: `operator_wallet`
   - max position: `1000` USD
   - leverage cap: `2`
   - kill switch: enabled, behavior `pause_agent`
2. Validate whether current custody/address combination is complete
3. If incomplete, force reconfiguration (cannot keep invalid/incomplete policy)
4. Prompt custody mode (`operator_wallet`, `user_wallet`, `dual_mode`)
5. For operator/user wallets, prompt provisioning method:
   - keep current address (if valid)
   - add existing wallet address
   - create new wallet address locally (paper/dev scaffold)
   - skip (only when wallet is optional for selected custody mode)
6. Prompt vault address (optional)
7. Validate wallet/vault format: `0x` prefix + 40 hex chars
8. Prompt max position size in USD (`> 0`)
9. Prompt leverage cap (`> 0`)
10. Prompt kill switch enabled/disabled
11. If enabled: prompt behavior (`pause_agent`, `cancel_open_orders`, `cancel_and_flatten`)
12. If disabled: force behavior to `pause_agent`
13. Enforce custody/address requirements via `validate_wallet_policy_requirements()`
14. Persist to `self.settings.wallet_vault_policy`

---

### Step 10: Verification Backend

**Module:** `wizard.rs` → `step_verification_backend()`

**Goal:** Configure primary verification path and local signed fallback receipts.

**Flow:**
1. Normalize current values with conservative defaults:
   - backend: `eigencloud_primary`
   - auth scheme: `bearer`
   - fallback enabled: `true`
   - require signed receipts: `true`
2. Ask to keep current policy (default: yes)
3. Prompt backend (`eigencloud_primary` / `fallback_only`)
4. Prompt EigenCloud endpoint URL (optional; must be `http(s)` when provided)
5. Prompt EigenCloud auth scheme (`bearer` / `api_key`)
6. Prompt token handling:
   - keep existing token
   - or enter new token via `secret_input` (empty value unsets)
7. Prompt EigenCloud timeout in milliseconds (`> 0`)
8. Prompt fallback enabled/disabled
9. If backend is `fallback_only` and fallback is disabled, fallback is force-enabled
10. When fallback enabled:
    - prompt fallback signing key id (optional)
    - prompt fallback chain path (default: `~/.enclagent/receipts/receipt-chain.ndjson`)
    - prompt whether signed fallback receipts are required
11. Enforce backend/fallback constraints via `validate_verification_backend_requirements()`
12. Persist to `self.settings.verification_backend`

---

## Settings Persistence

### Two-Layer Architecture

Settings are persisted in two places:

**Layer 1: `~/.enclagent/.env`** (bootstrap vars)

Contains only the settings needed BEFORE database connection. Written by
`save_bootstrap_env()` in `bootstrap.rs`.

```env
DATABASE_BACKEND="libsql"
LIBSQL_PATH="/Users/name/.enclagent/enclagent.db"
LLM_BACKEND="openai_compatible"
LLM_BASE_URL="http://my-vllm:8000/v1"
```

Or for PostgreSQL + NEAR AI:
```env
DATABASE_BACKEND="postgres"
DATABASE_URL="postgres://user:pass@localhost/enclagent"
LLM_BACKEND="nearai"
```

Or for Ollama:
```env
LLM_BACKEND="ollama"
OLLAMA_BASE_URL="http://localhost:11434"
```

**Why separate?** Chicken-and-egg: you need `DATABASE_BACKEND` to know
which database to connect to, and `LLM_BACKEND` to know whether to
attempt NEAR AI session auth -- neither can be stored in the database.

**Layer 2: Database settings table** (everything else)

All other settings are stored as key-value pairs in the `settings` table,
keyed by `(user_id, key)`. Written by `set_all_settings()`.

Settings are serialized via `Settings::to_db_map()` as dotted paths:
```
database_backend = "libsql"
llm_backend = "nearai"
selected_model = "anthropic/claude-sonnet-4-5"
embeddings.enabled = true
embeddings.provider = "nearai"
channels.http_enabled = true
heartbeat.enabled = true
heartbeat.interval_secs = 300
hyperliquid_runtime.network = "testnet"
hyperliquid_runtime.paper_live_policy = "paper_first"
wallet_vault_policy.custody_mode = "operator_wallet"
wallet_vault_policy.max_position_size_usd = 1000
wallet_vault_policy.kill_switch_enabled = true
verification_backend.backend = "eigencloud_primary"
verification_backend.fallback_enabled = true
verification_backend.fallback_require_signed_receipts = true
```

### save_and_summarize()

Final step of the wizard:

```
1. Mark onboard_completed = true
2. Write ALL settings to database (try postgres pool, then libSQL backend)
3. Write bootstrap vars to ~/.enclagent/.env:
   - DATABASE_BACKEND (always)
   - DATABASE_URL     (if postgres)
   - LIBSQL_PATH      (if libsql)
   - LIBSQL_URL       (if turso sync)
   - LLM_BACKEND      (always, when set)
   - LLM_BASE_URL     (if openai_compatible)
   - LLM_MODEL        (if openai_compatible + model selected)
   - OLLAMA_BASE_URL  (if ollama)
   - OLLAMA_MODEL     (if ollama + model selected)
4. Print configuration summary
```

**Invariant:** Both Layer 1 and Layer 2 must be written. If the database
write fails, the wizard returns an error and the `.env` file is not written.

### Legacy Migration

`bootstrap.rs` handles one-time upgrades from older config formats:
- `bootstrap.json` → extracts `DATABASE_URL`, writes `.env`, renames to `.migrated`
- `settings.json` → migrated to database via `migrate_disk_to_db()`

---

## Settings Struct

**Module:** `settings.rs`

```rust
pub struct Settings {
    // Meta
    pub onboard_completed: bool,

    // Step 1: Database
    pub database_backend: Option<String>,    // "postgres" | "libsql"
    pub database_url: Option<String>,
    pub libsql_path: Option<String>,
    pub libsql_url: Option<String>,

    // Step 2: Security
    pub secrets_master_key_source: KeySource, // Keychain | Env | None

    // Step 3: Inference
    pub llm_backend: Option<String>,         // "nearai" | "anthropic" | "openai" | "ollama" | "openai_compatible"
    pub ollama_base_url: Option<String>,
    pub openai_compatible_base_url: Option<String>,

    // Step 4: Model
    pub selected_model: Option<String>,

    // Step 5: Embeddings
    pub embeddings: EmbeddingsSettings,      // enabled, provider, model

    // Step 6: Channels
    pub tunnel: TunnelSettings,              // provider, public_url
    pub channels: ChannelSettings,           // http config, telegram owner, etc.

    // Step 7: Heartbeat
    pub heartbeat: HeartbeatSettings,        // enabled, interval, notify

    // Step 8: Hyperliquid runtime profile
    pub hyperliquid_runtime: HyperliquidRuntimeSettings, // network, endpoints, retries, paper/live

    // Step 9: Wallet and vault policy
    pub wallet_vault_policy: WalletVaultPolicySettings, // custody, wallets, risk, kill switch

    // Step 10: Verification backend
    pub verification_backend: VerificationBackendSettings, // eigencloud + local signed fallback

    // Step 12: Copytrading initialization profile
    pub copytrading: CopyTradingSettings,    // allocation, per-trade cap, leverage, symbols, sharing scope

    // Deterministic intent/execution/verification contract settings
    pub intent_runtime: IntentRuntimeSettings,

    // Advanced (not in wizard, set via `enclagent config set`)
    pub agent: AgentSettings,
    pub wasm: WasmSettings,
    pub sandbox: SandboxSettings,
    pub safety: SafetySettings,
    pub builder: BuilderSettings,
}
```

**KeySource enum:** `Keychain | Env | None`

---

## Secrets Flow

### SecretsContext

Thin wrapper for setup-time secret operations:

```rust
pub struct SecretsContext {
    store: Arc<dyn SecretsStore>,
    user_id: String,
}
```

Created by `init_secrets_context()` which:
1. Gets `SecretsCrypto` from `self.secrets_crypto` or loads from keychain/env
2. Creates the appropriate backend store:
   - If both features compiled: respects `self.settings.database_backend`
   - Tries selected backend first, falls back to the other
3. Returns `SecretsContext` wrapping the store

### Secret Storage

Secrets are encrypted with AES-256-GCM using the master key, then stored
in the database `secrets` table. The wizard writes secrets like:

```
telegram_bot_token    → encrypted bot token
telegram_webhook_secret → encrypted webhook HMAC secret
llm_anthropic_api_key → encrypted API key
llm_openai_api_key    → encrypted API key
llm_compatible_api_key → encrypted API key
```

---

## Prompt Utilities

**Module:** `prompts.rs`

| Function | Description |
|----------|-------------|
| `select_one(label, options)` | Numbered single-choice menu |
| `select_many(label, options, defaults)` | Checkbox multi-select (raw terminal mode) |
| `input(label)` | Single line text input |
| `optional_input(label, hint)` | Text input that can be empty |
| `secret_input(label)` | Hidden input (shows `*` per char), returns `SecretString` |
| `confirm(label, default)` | `[Y/n]` or `[y/N]` prompt |
| `print_header(text)` | Bold section header with underline |
| `print_step(n, total, text)` | `[1/10] Step Name` |
| `print_success(text)` | Green checkmark prefix |
| `print_error(text)` | Red X prefix |
| `print_info(text)` | Blue info prefix |

`select_many` uses `crossterm` raw mode for arrow key navigation.
Must properly restore terminal state on all exit paths.

---

## Platform Caveats

### macOS Keychain

- `get_generic_password()` triggers system dialogs (unlock + authorize)
- Two dialogs per call is normal, not a bug
- Cache the result after first access to avoid repeat prompts
- Never probe keychain in read-only commands (`status`, `--help`)
- Service name: `"enclagent"`, account: `"master_key"`

### Linux Secret Service

- Uses GNOME Keyring or KWallet via `secret-service` crate
- May need `gnome-keyring` daemon running
- Collection unlock may prompt for password

### URL Passwords

- `#` is common in URL-encoded passwords (`%23` decoded)
- `.env` values must be double-quoted to preserve `#`
- Display masked: `postgres://user:****@host/db`

### Telegram API

- Bot token format: `123456:ABC-DEF...`
- Token goes in URL path: `https://api.telegram.org/bot{TOKEN}/method`
- Webhook secret header: `X-Telegram-Bot-Api-Secret-Token`
- Owner binding polls `getUpdates` (must delete webhook first)

---

## Testing

Tests live in `mod tests {}` at the bottom of each file.

**What to test when modifying setup:**

- Settings round-trip: `to_db_map()` then `from_db_map()` preserves values
- Bootstrap `.env`: dotenvy can parse what `save_bootstrap_env()` writes
- Model fetchers: static fallback works when API is unreachable
- Channel discovery: handles missing dir, invalid JSON, deduplication
- Prompt functions: not tested (interactive I/O), but ensure error paths
  don't panic

**Run setup tests:**
```bash
cargo test --lib -- setup
cargo test --lib -- bootstrap
```

---

## Modification Checklist

When changing the onboarding flow:

1. Update this README first with the intended behavior change
2. If adding a new wizard step:
   - Add to `run()`, adjust `total_steps`
   - Add corresponding settings fields to `Settings`
   - Add `to_db_map` / `from_db_map` serialization
   - If the setting is needed before DB connection, add to `save_bootstrap_env()`
3. If adding a new provider or channel:
   - Add to the selection menu in the appropriate step
   - Add authentication flow (API key or OAuth)
   - Add model fetcher with static fallback + 5s timeout
4. If touching keychain:
   - Cache the result, never call `get_master_key()` twice
   - Test on macOS (dialog behavior differs from Linux)
5. If touching secrets:
   - Ensure `init_secrets_context()` respects the selected database backend
   - Test with both postgres and libsql features
6. Run the full shipping checklist:
   ```bash
   cargo fmt
   cargo clippy --all --benches --tests --examples --all-features -- -D warnings
   cargo test --lib -- setup bootstrap
   ```
7. Test a fresh onboarding: `rm -rf ~/.enclagent && cargo run`
