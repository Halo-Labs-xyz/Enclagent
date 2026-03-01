#!/usr/bin/env bash
set -euo pipefail

wallet=""
session=""
config_b64=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --wallet)
      wallet="${2:-}"
      shift 2
      ;;
    --session)
      session="${2:-}"
      shift 2
      ;;
    --config|--config-b64)
      config_b64="${2:-}"
      shift 2
      ;;
    *)
      echo "unknown arg: $1" >&2
      exit 2
      ;;
  esac
done

if [[ -z "$wallet" || -z "$session" || -z "$config_b64" ]]; then
  echo "usage: provision-user-ecloud.sh --wallet <0x...> --session <uuid> --config <base64url-json>" >&2
  exit 2
fi

if ! command -v ecloud >/dev/null 2>&1; then
  echo "ecloud CLI not found in PATH" >&2
  exit 2
fi
if ! command -v node >/dev/null 2>&1; then
  echo "node is required to decode frontdoor config" >&2
  exit 2
fi
if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required to seed per-user settings" >&2
  exit 2
fi

if [[ -z "${ECLOUD_FRONTDOOR_IMAGE_REF:-}" ]]; then
  echo "ECLOUD_FRONTDOOR_IMAGE_REF is required" >&2
  exit 2
fi

config_json="$(node -e '
const b64 = process.argv[1] || "";
const pad = "=".repeat((4 - (b64.length % 4)) % 4);
const normalized = (b64 + pad).replace(/-/g, "+").replace(/_/g, "/");
process.stdout.write(Buffer.from(normalized, "base64").toString("utf8"));
' "$config_b64")"

gateway_auth_key="$(printf '%s' "$config_json" | node -e '
let data = "";
process.stdin.on("data", (d) => data += d);
process.stdin.on("end", () => {
  const parsed = JSON.parse(data || "{}");
  process.stdout.write(String(parsed.gateway_auth_key || ""));
});
')"

profile_name="$(printf '%s' "$config_json" | node -e '
let data = "";
process.stdin.on("data", (d) => data += d);
process.stdin.on("end", () => {
  const parsed = JSON.parse(data || "{}");
  process.stdout.write(String(parsed.profile_name || "session"));
});
')"

owner_wallet="$(printf '%s' "$wallet" | tr '[:upper:]' '[:lower:]')"

normalize_repo_url() {
  node -e '
const raw = String(process.argv[1] || "").trim();
if (!raw) process.exit(0);
let url = raw;
const gitAtMatch = url.match(/^git@([^:]+):(.+)$/);
if (gitAtMatch) {
  url = `https://${gitAtMatch[1]}/${gitAtMatch[2]}`;
} else if (/^ssh:\/\/git@/i.test(url)) {
  url = url
    .replace(/^ssh:\/\/git@/i, "https://")
    .replace(/\.git$/i, "");
}
if (!/^https?:\/\//i.test(url)) process.exit(0);
url = url.replace(/\.git$/i, "").replace(/\/+$/g, "");
process.stdout.write(url);
' "$1"
}

resolve_source_repo_url() {
  local candidate="${ECLOUD_FRONTDOOR_SOURCE_REPO_URL:-${ECLOUD_FRONTDOOR_REPO_URL:-${GATEWAY_FRONTDOOR_SOURCE_REPO_URL:-}}}"
  if [[ -z "$candidate" && -n "${RAILWAY_GIT_REPO_FULL_NAME:-}" ]]; then
    candidate="https://github.com/${RAILWAY_GIT_REPO_FULL_NAME}"
  fi
  if [[ -z "$candidate" && -n "${RAILWAY_GIT_REPO_OWNER:-}" && -n "${RAILWAY_GIT_REPO_NAME:-}" ]]; then
    candidate="https://github.com/${RAILWAY_GIT_REPO_OWNER}/${RAILWAY_GIT_REPO_NAME}"
  fi
  if [[ -z "$candidate" && -n "${GITHUB_REPOSITORY:-}" ]]; then
    candidate="https://github.com/${GITHUB_REPOSITORY}"
  fi
  if [[ -z "$candidate" ]] && command -v git >/dev/null 2>&1; then
    candidate="$(git config --get remote.origin.url 2>/dev/null || true)"
  fi
  normalize_repo_url "$candidate"
}

resolve_source_commit() {
  local candidate="${ECLOUD_FRONTDOOR_SOURCE_COMMIT:-${ECLOUD_FRONTDOOR_COMMIT_SHA:-${GATEWAY_FRONTDOOR_SOURCE_COMMIT:-${RAILWAY_GIT_COMMIT_SHA:-${GITHUB_SHA:-}}}}}"
  if [[ -z "$candidate" ]] && command -v git >/dev/null 2>&1; then
    candidate="$(git rev-parse HEAD 2>/dev/null || true)"
  fi
  candidate="$(printf '%s' "$candidate" | tr '[:upper:]' '[:lower:]' | tr -d '[:space:]')"
  if [[ -z "$candidate" ]]; then
    printf ''
    return
  fi
  if [[ "$candidate" =~ ^[0-9a-f]{7,40}$ ]]; then
    printf '%s' "$candidate"
    return
  fi
  echo "warning: ignoring invalid source commit value: ${candidate}" >&2
  printf ''
}

infer_app_url_from_verify_url() {
  node -e '
const verifyUrl = String(process.argv[1] || "").trim();
const appId = String(process.argv[2] || "").trim();
if (!verifyUrl) process.exit(0);
let out = "";
try {
  const url = new URL(verifyUrl);
  const host = url.hostname.toLowerCase();
  if (host === "verify-sepolia.eigencloud.xyz") {
    url.hostname = "sepolia.eigencloud.xyz";
  } else if (host === "verify-mainnet.eigencloud.xyz" || host === "verify.eigencloud.xyz") {
    url.hostname = "mainnet.eigencloud.xyz";
  } else if (host.startsWith("verify-")) {
    url.hostname = host.slice("verify-".length);
  } else if (host.startsWith("verify.")) {
    url.hostname = host.slice("verify.".length);
  } else {
    process.exit(0);
  }

  if (appId) {
    const parts = url.pathname.split("/").filter(Boolean);
    const normalized = parts.length >= 1 ? parts[0].toLowerCase() : "";
    if (normalized !== "app" || parts.length < 2) {
      url.pathname = `/app/${appId}`;
    } else if (parts[1].toLowerCase() !== appId.toLowerCase()) {
      url.pathname = `/app/${appId}`;
    }
  }
  out = url.toString().replace(/\/$/, "");
} catch (_) {
  process.exit(0);
}
process.stdout.write(out);
' "$1" "$2"
}

log_phase() {
  printf 'provision_phase: %s\n' "$1" >&2
}

settings_payload="$(FRONTDOOR_OWNER_WALLET="$owner_wallet" printf '%s' "$config_json" | node -e '
let data = "";
process.stdin.on("data", (d) => data += d);
process.stdin.on("end", () => {
  const cfg = JSON.parse(data || "{}");
  const ownerWallet = String(process.env.FRONTDOOR_OWNER_WALLET || "").toLowerCase();
  const settings = {};
  const set = (k, v) => {
    if (v === undefined || v === null) return;
    settings[k] = v;
  };

  set("hyperliquid_runtime.network", cfg.hyperliquid_network);
  set("hyperliquid_runtime.paper_live_policy", cfg.paper_live_policy);
  set("hyperliquid_runtime.api_base_url", cfg.hyperliquid_api_base_url);
  set("hyperliquid_runtime.ws_url", cfg.hyperliquid_ws_url);
  set("hyperliquid_runtime.timeout_ms", cfg.request_timeout_ms);
  set("hyperliquid_runtime.max_retries", cfg.max_retries);
  set("hyperliquid_runtime.retry_backoff_ms", cfg.retry_backoff_ms);

  set("wallet_vault_policy.custody_mode", cfg.custody_mode);
  set("wallet_vault_policy.operator_wallet_address", cfg.operator_wallet_address);
  set("wallet_vault_policy.user_wallet_address", cfg.user_wallet_address || ownerWallet);
  set("wallet_vault_policy.vault_address", cfg.vault_address);
  set("wallet_vault_policy.max_position_size_usd", cfg.max_position_size_usd);
  set("wallet_vault_policy.leverage_cap", cfg.leverage_cap);
  set("wallet_vault_policy.kill_switch_enabled", cfg.kill_switch_enabled);
  set("wallet_vault_policy.kill_switch_behavior", cfg.kill_switch_behavior);

  set("copytrading.max_allocation_usd", cfg.max_allocation_usd);
  set("copytrading.per_trade_notional_cap_usd", cfg.per_trade_notional_cap_usd);
  set("copytrading.max_leverage", cfg.max_leverage);
  set("copytrading.symbol_allowlist", cfg.symbol_allowlist);
  set("copytrading.symbol_denylist", cfg.symbol_denylist);
  set("copytrading.max_slippage_bps", cfg.max_slippage_bps);
  set("copytrading.information_sharing_scope", cfg.information_sharing_scope);
  set("verification_backend.backend", cfg.verification_backend);
  set("verification_backend.eigencloud_endpoint", cfg.verification_eigencloud_endpoint);
  set("verification_backend.eigencloud_auth_scheme", cfg.verification_eigencloud_auth_scheme);
  set("verification_backend.eigencloud_auth_token", cfg.eigencloud_auth_key);
  set("verification_backend.eigencloud_timeout_ms", cfg.verification_eigencloud_timeout_ms);
  set("verification_backend.fallback_enabled", cfg.verification_fallback_enabled);
  set("verification_backend.fallback_signing_key_id", cfg.verification_fallback_signing_key_id);
  set("verification_backend.fallback_chain_path", cfg.verification_fallback_chain_path);
  set("verification_backend.fallback_require_signed_receipts", cfg.verification_fallback_require_signed_receipts);

  process.stdout.write(JSON.stringify({ settings }));
});
')"

env_name="${ECLOUD_ENV:-sepolia}"
prefix="${ECLOUD_FRONTDOOR_APP_PREFIX:-enclagent-user}"
name="$(node -e '
const rawPrefix = process.argv[1] || "enclagent-user";
const rawProfile = process.argv[2] || "session";
const rawWallet = process.argv[3] || "";
const rawSession = process.argv[4] || "";
const slug = (value, fallback, maxLen) => {
  const normalized = String(value || "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  const trimmed = normalized.slice(0, maxLen).replace(/-+$/g, "");
  return trimmed || fallback;
};
const prefix = slug(rawPrefix, "enclagent", 16);
let profile = slug(rawProfile, "session", 20);
const walletHex = String(rawWallet || "")
  .toLowerCase()
  .replace(/^0x/, "")
  .replace(/[^a-f0-9]/g, "");
const walletTail = walletHex.slice(-6) || "user";
const sessionTag =
  String(rawSession || "")
    .toLowerCase()
    .replace(/[^a-z0-9]/g, "")
    .slice(0, 8) || "session";
let name = `${prefix}-${profile}-${walletTail}-${sessionTag}`;
if (name.length > 50) {
  const staticLen = prefix.length + walletTail.length + sessionTag.length + 3;
  const maxProfile = Math.max(8, 50 - staticLen);
  profile = profile.slice(0, maxProfile).replace(/-+$/g, "") || "session";
  name = `${prefix}-${profile}-${walletTail}-${sessionTag}`;
}
if (name.length > 50) {
  name = name.slice(0, 50).replace(/-+$/g, "");
}
process.stdout.write(name || `enclagent-${sessionTag}`);
' "$prefix" "$profile_name" "$wallet" "$session")"
description="Enclagent session ${session:0:12} (${profile_name})"
instance_type="${ECLOUD_FRONTDOOR_INSTANCE_TYPE:-g1-standard-4t}"
instance_port="${ECLOUD_FRONTDOOR_INSTANCE_PORT:-3000}"
log_visibility="${ECLOUD_FRONTDOOR_LOG_VISIBILITY:-public}"
resource_usage_monitoring="${ECLOUD_FRONTDOOR_RESOURCE_USAGE_MONITORING:-disable}"

effective_env_file="$(mktemp)"
cleanup() {
  rm -f "$effective_env_file"
}
trap cleanup EXIT

append_env_var_if_set() {
  local key="$1"
  local value="${!key:-}"
  if [[ -z "$value" ]]; then
    return
  fi
  if [[ "$value" == *$'\n'* || "$value" == *$'\r'* ]]; then
    echo "skipping ${key}: multiline values are not supported in env-file output" >&2
    return
  fi
  printf '%s=%s\n' "$key" "$value" >> "$effective_env_file"
}

if [[ -n "${ECLOUD_FRONTDOOR_ENV_FILE:-}" ]]; then
  if [[ ! -f "${ECLOUD_FRONTDOOR_ENV_FILE}" ]]; then
    echo "ECLOUD_FRONTDOOR_ENV_FILE does not exist: ${ECLOUD_FRONTDOOR_ENV_FILE}" >&2
    exit 2
  fi
  cp "${ECLOUD_FRONTDOOR_ENV_FILE}" "$effective_env_file"
fi

# Inherit core runtime/LLM settings from the frontdoor process so per-session
# instances do not rely on placeholder values in static env templates.
append_env_var_if_set DATABASE_BACKEND
append_env_var_if_set LIBSQL_PATH
append_env_var_if_set DATABASE_URL
append_env_var_if_set LLM_BACKEND
append_env_var_if_set ANTHROPIC_API_KEY
append_env_var_if_set ANTHROPIC_BASE_URL
append_env_var_if_set ANTHROPIC_MODEL
append_env_var_if_set OPENAI_API_KEY
append_env_var_if_set OPENAI_MODEL
append_env_var_if_set OPENAI_BASE_URL
append_env_var_if_set CLI_ENABLED
append_env_var_if_set RUST_LOG

FRONTDOOR_OWNER_WALLET="$owner_wallet" printf '%s' "$config_json" | node -e '
let data = "";
process.stdin.on("data", (d) => data += d);
process.stdin.on("end", () => {
  const cfg = JSON.parse(data || "{}");
  const ownerWallet = String(process.env.FRONTDOOR_OWNER_WALLET || "").toLowerCase();
  const out = (k, v) => {
    if (v === undefined || v === null) return;
    const s = String(v).trim();
    if (!s || /[\r\n]/.test(s)) return;
    process.stdout.write(`${k}=${s}\n`);
  };
  const outInt = (k, v) => {
    if (v === undefined || v === null) return;
    const n = Number(v);
    if (!Number.isFinite(n)) return;
    process.stdout.write(`${k}=${Math.trunc(n)}\n`);
  };

  process.stdout.write("GATEWAY_FRONTDOOR_ENABLED=false\n");
  out("GATEWAY_AUTH_TOKEN", cfg.gateway_auth_key);
  out("HYPERLIQUID_NETWORK", cfg.hyperliquid_network);
  out("HYPERLIQUID_PAPER_LIVE_POLICY", cfg.paper_live_policy);
  out("HYPERLIQUID_API_BASE_URL", cfg.hyperliquid_api_base_url);
  out("HYPERLIQUID_WS_URL", cfg.hyperliquid_ws_url);
  outInt("HYPERLIQUID_TIMEOUT_MS", cfg.request_timeout_ms);
  outInt("HYPERLIQUID_MAX_RETRIES", cfg.max_retries);
  outInt("HYPERLIQUID_RETRY_BACKOFF_MS", cfg.retry_backoff_ms);
  out("HYPERLIQUID_CUSTODY_MODE", cfg.custody_mode);
  out("HYPERLIQUID_OPERATOR_WALLET_ADDRESS", cfg.operator_wallet_address);
  out("HYPERLIQUID_USER_WALLET_ADDRESS", cfg.user_wallet_address || ownerWallet);
  out("HYPERLIQUID_VAULT_ADDRESS", cfg.vault_address);
  outInt("HYPERLIQUID_MAX_POSITION_SIZE_USD", cfg.max_position_size_usd);
  outInt("HYPERLIQUID_LEVERAGE_CAP", cfg.leverage_cap);
  out("HYPERLIQUID_KILL_SWITCH_ENABLED", cfg.kill_switch_enabled === false ? "false" : "true");
  out("HYPERLIQUID_KILL_SWITCH_BEHAVIOR", cfg.kill_switch_behavior);
  out("VERIFICATION_BACKEND", cfg.verification_backend);
  out("EIGENCLOUD_ENDPOINT", cfg.verification_eigencloud_endpoint);
  out("EIGENCLOUD_AUTH_SCHEME", cfg.verification_eigencloud_auth_scheme);
  out("EIGENCLOUD_AUTH_TOKEN", cfg.eigencloud_auth_key);
  outInt("EIGENCLOUD_TIMEOUT_MS", cfg.verification_eigencloud_timeout_ms);
  out("VERIFICATION_FALLBACK_ENABLED", cfg.verification_fallback_enabled === false ? "false" : "true");
  out("VERIFICATION_FALLBACK_REQUIRE_SIGNED_RECEIPTS", cfg.verification_fallback_require_signed_receipts === false ? "false" : "true");
  out("VERIFICATION_FALLBACK_SIGNING_KEY_ID", cfg.verification_fallback_signing_key_id);
  out("VERIFICATION_FALLBACK_CHAIN_PATH", cfg.verification_fallback_chain_path);
});
' >> "$effective_env_file"

deploy_args=(
  compute app deploy
  --environment "$env_name"
  --name "$name"
  --description "$description"
  --image-ref "$ECLOUD_FRONTDOOR_IMAGE_REF"
  --env-file "$effective_env_file"
  --instance-type "$instance_type"
  --log-visibility "$log_visibility"
  --resource-usage-monitoring "$resource_usage_monitoring"
  --skip-profile
)

source_repo_url="$(resolve_source_repo_url)"
source_commit="$(resolve_source_commit)"
strict_source_provenance="$(printf '%s' "${ECLOUD_FRONTDOOR_STRICT_SOURCE_PROVENANCE:-false}" | tr '[:upper:]' '[:lower:]')"
verifiable_source_args=()
if [[ -n "$source_repo_url" && -n "$source_commit" ]]; then
  verifiable_source_args+=(--repo "$source_repo_url" --commit "$source_commit")
elif [[ -n "$source_repo_url" || -n "$source_commit" ]]; then
  echo "warning: incomplete source provenance metadata; both repo URL and commit are required" >&2
  if [[ "$strict_source_provenance" =~ ^(true|1|yes|on)$ ]]; then
    echo "strict source provenance enabled; refusing deploy without both source repo URL and source commit" >&2
    exit 1
  fi
fi

force_verifiable="$(printf '%s' "${ECLOUD_FRONTDOOR_FORCE_VERIFIABLE:-false}" | tr '[:upper:]' '[:lower:]')"
deploy_max_retries_raw="${ECLOUD_FRONTDOOR_DEPLOY_MAX_RETRIES:-24}"
deploy_retry_backoff_raw="${ECLOUD_FRONTDOOR_DEPLOY_RETRY_BACKOFF_SECS:-15}"
deploy_retry_timeout_raw="${ECLOUD_FRONTDOOR_DEPLOY_RETRY_TIMEOUT_SECS:-900}"
if [[ ! "$deploy_max_retries_raw" =~ ^[0-9]+$ ]]; then
  deploy_max_retries_raw=24
fi
if [[ ! "$deploy_retry_backoff_raw" =~ ^[0-9]+$ ]] || (( deploy_retry_backoff_raw < 1 )); then
  deploy_retry_backoff_raw=15
fi
if [[ ! "$deploy_retry_timeout_raw" =~ ^[0-9]+$ ]] || (( deploy_retry_timeout_raw < 30 )); then
  deploy_retry_timeout_raw=900
fi

run_deploy_once() {
  local output_file="$1"
  : > "$output_file"
  case "$force_verifiable" in
    true|1|yes|on)
      ecloud "${deploy_args[@]}" --verifiable "${verifiable_source_args[@]}" 2>&1 | tee "$output_file"
      local statuses=("${PIPESTATUS[@]}")
      return "${statuses[0]}"
      ;;
    *)
      # Non-interactive safety: answer "no" to verifiable-build prompt when
      # deploying prebuilt images that are not in EigenLayer's verifiable registry.
      printf 'n\n' | ecloud "${deploy_args[@]}" 2>&1 | tee "$output_file"
      local statuses=("${PIPESTATUS[@]}")
      return "${statuses[1]}"
      ;;
  esac
}

capture_active_build_context() {
  local build_json
  local build_context

  build_json="$(ecloud compute build list --environment "$env_name" --limit 1 --json 2>/dev/null || true)"
  if [[ -z "$build_json" ]]; then
    return
  fi

  build_context="$(printf '%s' "$build_json" | node -e '
const raw = require("fs").readFileSync(0, "utf8");
try {
  const parsed = JSON.parse(raw);
  const latest = Array.isArray(parsed) ? parsed[0] : null;
  if (!latest || typeof latest !== "object") process.exit(0);
  const status = String(latest.status || "").toLowerCase();
  if (!status) process.exit(0);
  const buildId = String(latest.buildId || "");
  const repo = String(latest.repoUrl || "").replace(/\.git$/i, "");
  const gitRef = String(latest.gitRef || "");
  const createdAt = String(latest.createdAt || "");
  const updatedAt = String(latest.updatedAt || "");
  const segments = [
    `status=${status}`,
    buildId ? `build_id=${buildId}` : "",
    repo ? `repo=${repo}` : "",
    gitRef ? `git_ref=${gitRef}` : "",
    createdAt ? `created_at=${createdAt}` : "",
    updatedAt ? `updated_at=${updatedAt}` : ""
  ].filter(Boolean);
  process.stdout.write(segments.join(" "));
} catch (_err) {
  process.exit(0);
}
')" || true

  if [[ -n "$build_context" ]]; then
    log_phase "eigencloud build queue context ${build_context}"
  fi
}

extract_build_id_from_text() {
  node -e '
const raw = String(process.argv[1] || "");
const patterns = [
  /\/builds\/([0-9a-fA-F-]{36})\b/,
  /\bbuild[_ -]?id[=: ]+([0-9a-fA-F-]{36})\b/i
];
for (const pattern of patterns) {
  const match = raw.match(pattern);
  if (match && match[1]) {
    process.stdout.write(match[1].toLowerCase());
    process.exit(0);
  }
}
' "$1"
}

resolve_active_build_id_from_queue() {
  local build_json
  build_json="$(ecloud compute build list --environment "$env_name" --limit 20 --json 2>/dev/null || true)"
  if [[ -z "$build_json" ]]; then
    return
  fi

  printf '%s' "$build_json" | node -e '
const raw = require("fs").readFileSync(0, "utf8");
const sourceRepo = String(process.argv[1] || "").replace(/\.git$/i, "").toLowerCase();
const sourceCommit = String(process.argv[2] || "").toLowerCase();
const activeStatuses = new Set([
  "building",
  "queued",
  "pending",
  "running",
  "created",
  "starting",
  "submitted",
  "in_progress"
]);
const normalizeRepo = (value) => String(value || "").replace(/\.git$/i, "").toLowerCase();
try {
  const parsed = JSON.parse(raw);
  const builds = Array.isArray(parsed) ? parsed : [];
  const matching = builds.find((item) => {
    const status = String(item.status || "").toLowerCase();
    if (!activeStatuses.has(status)) return false;
    if (sourceRepo && normalizeRepo(item.repoUrl) !== sourceRepo) return false;
    if (sourceCommit && String(item.gitRef || "").toLowerCase() !== sourceCommit) return false;
    return true;
  });
  const fallback = builds.find((item) => activeStatuses.has(String(item.status || "").toLowerCase()));
  const chosen = matching || fallback;
  if (!chosen || !chosen.buildId) process.exit(0);
  process.stdout.write(String(chosen.buildId).toLowerCase());
} catch (_err) {
  process.exit(0);
}
' "$source_repo_url" "$source_commit"
}

build_id_matches_expected_source() {
  local build_id="$1"
  local status_json
  local expected_repo
  local expected_commit
  local actual_repo
  local actual_commit
  local commit_match=1

  if [[ -z "$build_id" ]]; then
    return 1
  fi
  if [[ -z "$source_repo_url" || -z "$source_commit" ]]; then
    return 0
  fi

  status_json="$(ecloud compute build status "$build_id" --environment "$env_name" --json 2>/dev/null || true)"
  if [[ -z "$status_json" ]]; then
    return 0
  fi

  expected_repo="$(printf '%s' "$source_repo_url" | tr '[:upper:]' '[:lower:]' | sed -E 's/\.git$//' | sed -E 's#/*$##')"
  expected_commit="$(printf '%s' "$source_commit" | tr '[:upper:]' '[:lower:]' | tr -d '[:space:]')"
  actual_repo="$(printf '%s' "$status_json" | node -e '
const raw = require("fs").readFileSync(0, "utf8");
try {
  const parsed = JSON.parse(raw);
  const repo = String(parsed.repoUrl || "")
    .toLowerCase()
    .replace(/\.git$/i, "")
    .replace(/\/+$/g, "");
  process.stdout.write(repo);
} catch (_err) {
  process.exit(0);
}
')"
  actual_commit="$(printf '%s' "$status_json" | node -e '
const raw = require("fs").readFileSync(0, "utf8");
try {
  const parsed = JSON.parse(raw);
  process.stdout.write(String(parsed.gitRef || "").toLowerCase().trim());
} catch (_err) {
  process.exit(0);
}
')"

  if [[ -z "$actual_repo" || -z "$actual_commit" ]]; then
    return 0
  fi
  if [[ "$actual_commit" == "$expected_commit" ]]; then
    commit_match=0
  elif [[ ${#expected_commit} -ge 7 && "$actual_commit" == "$expected_commit"* ]]; then
    commit_match=0
  elif [[ ${#actual_commit} -ge 7 && "$expected_commit" == "$actual_commit"* ]]; then
    commit_match=0
  fi

  if [[ "$actual_repo" == "$expected_repo" && "$commit_match" -eq 0 ]]; then
    if [[ "$actual_commit" != "$expected_commit" ]]; then
      log_phase "eigencloud build commit prefix match build_id=${build_id} expected_commit=${expected_commit} actual_commit=${actual_commit}"
    fi
    return 0
  fi

  log_phase "eigencloud build source mismatch build_id=${build_id} expected_repo=${expected_repo} expected_commit=${expected_commit} actual_repo=${actual_repo} actual_commit=${actual_commit}"
  return 1
}

wait_for_build_terminal_state() {
  local build_id="$1"
  local timeout_secs="$2"
  local poll_secs_raw="${ECLOUD_FRONTDOOR_BUILD_STATUS_POLL_SECS:-15}"
  local poll_secs="$poll_secs_raw"
  local started_at
  local last_status=""
  local status_json
  local build_status
  local status_context
  local status_error

  if [[ -z "$build_id" || -z "$timeout_secs" ]]; then
    return 1
  fi
  if [[ ! "$poll_secs" =~ ^[0-9]+$ ]] || (( poll_secs < 5 )); then
    poll_secs=15
  fi
  if (( timeout_secs < 1 )); then
    return 1
  fi

  started_at="$(date +%s)"
  log_phase "eigencloud build wait start build_id=${build_id} timeout_secs=${timeout_secs}"
  while true; do
    status_json="$(ecloud compute build status "$build_id" --environment "$env_name" --json 2>/dev/null || true)"
    build_status="$(printf '%s' "$status_json" | node -e '
const raw = require("fs").readFileSync(0, "utf8");
try {
  const parsed = JSON.parse(raw);
  process.stdout.write(String(parsed.status || "").toLowerCase());
} catch (_err) {
  process.exit(0);
}
')"
    status_context="$(printf '%s' "$status_json" | node -e '
const raw = require("fs").readFileSync(0, "utf8");
try {
  const parsed = JSON.parse(raw);
  const status = String(parsed.status || "").toLowerCase();
  const repo = String(parsed.repoUrl || "").replace(/\.git$/i, "");
  const gitRef = String(parsed.gitRef || "");
  const createdAt = String(parsed.createdAt || "");
  const updatedAt = String(parsed.updatedAt || "");
  const parts = [
    status ? `status=${status}` : "",
    repo ? `repo=${repo}` : "",
    gitRef ? `git_ref=${gitRef}` : "",
    createdAt ? `created_at=${createdAt}` : "",
    updatedAt ? `updated_at=${updatedAt}` : ""
  ].filter(Boolean);
  process.stdout.write(parts.join(" "));
} catch (_err) {
  process.exit(0);
}
')"
    status_error="$(printf '%s' "$status_json" | node -e '
const raw = require("fs").readFileSync(0, "utf8");
try {
  const parsed = JSON.parse(raw);
  process.stdout.write(String(parsed.errorMessage || ""));
} catch (_err) {
  process.exit(0);
}
')"

    if [[ -z "$build_status" ]]; then
      log_phase "eigencloud build wait build_id=${build_id} status=unknown (status lookup unavailable)"
    elif [[ "$build_status" != "$last_status" ]]; then
      log_phase "eigencloud build wait build_id=${build_id} ${status_context}"
      last_status="$build_status"
    fi

    case "$build_status" in
      success|succeeded|completed)
        log_phase "eigencloud build wait complete build_id=${build_id} status=${build_status}"
        return 0
        ;;
      failed|error|canceled|cancelled|aborted|timed_out|timeout)
        if [[ -n "$status_error" ]]; then
          log_phase "eigencloud build wait failed build_id=${build_id} status=${build_status} error=${status_error}"
        else
          log_phase "eigencloud build wait failed build_id=${build_id} status=${build_status}"
        fi
        return 2
        ;;
    esac

    now="$(date +%s)"
    elapsed=$((now - started_at))
    if (( elapsed >= timeout_secs )); then
      log_phase "eigencloud build wait timeout build_id=${build_id} elapsed=${elapsed}s timeout=${timeout_secs}s"
      return 1
    fi

    sleep "$poll_secs"
  done
}

log_phase "eigencloud provisioning started env=${env_name} ironclaw_profile=${profile_name} session=${session}"
if [[ -n "$source_repo_url" && -n "$source_commit" ]]; then
  log_phase "eigencloud verifiable source repo=${source_repo_url} commit=${source_commit}"
fi

deploy_attempt=1
deploy_retry_started_at="$(date +%s)"
deploy_attempt_cap_display="$deploy_max_retries_raw"
if (( deploy_max_retries_raw == 0 )); then
  deploy_attempt_cap_display="unbounded"
fi
while true; do
  log_phase "eigencloud deploy attempt=${deploy_attempt}/${deploy_attempt_cap_display} verifiable=${force_verifiable}"
  attempt_output_file="$(mktemp)"
  set +e
  run_deploy_once "$attempt_output_file"
  deploy_status=$?
  set -e
  deploy_output="$(cat "$attempt_output_file")"
  rm -f "$attempt_output_file"

  if [[ "$deploy_status" -eq 0 ]]; then
    log_phase "eigencloud deploy succeeded attempt=${deploy_attempt}"
    break
  fi

  if printf '%s' "$deploy_output" | grep -Eiq 'buildapi request failed:\s*429|too many requests|buildapi request failed:\s*409|already have a build in progress'; then
    retry_reason="eigencloud_build_queue_or_throttle"
    if printf '%s' "$deploy_output" | grep -Eiq 'buildapi request failed:\s*409|already have a build in progress'; then
      retry_reason="eigencloud_build_queue_conflict_409"
      capture_active_build_context
    elif printf '%s' "$deploy_output" | grep -Eiq 'buildapi request failed:\s*429|too many requests'; then
      retry_reason="eigencloud_build_throttle_429"
    fi

    retry_now="$(date +%s)"
    retry_elapsed=$((retry_now - deploy_retry_started_at))
    if (( retry_elapsed >= deploy_retry_timeout_raw )); then
      echo "$deploy_output" >&2
      echo "error: deploy retry timeout exceeded (${retry_elapsed}s >= ${deploy_retry_timeout_raw}s) reason=${retry_reason}" >&2
      exit 1
    fi

    remaining_retry_budget=$((deploy_retry_timeout_raw - retry_elapsed))
    if (( remaining_retry_budget < 1 )); then
      echo "$deploy_output" >&2
      echo "error: deploy retry budget exhausted before build wait (elapsed=${retry_elapsed}s/${deploy_retry_timeout_raw}s)" >&2
      exit 1
    fi

    active_build_id="$(extract_build_id_from_text "$deploy_output")"
    if [[ -z "$active_build_id" ]]; then
      active_build_id="$(resolve_active_build_id_from_queue)"
    fi
    if [[ -n "$active_build_id" ]]; then
      log_phase "eigencloud queue conflict resolved active_build_id=${active_build_id} reason=${retry_reason}"
      if ! build_id_matches_expected_source "$active_build_id"; then
        echo "warning: queued build ${active_build_id} does not match requested source provenance; skipping wait and retrying deploy" >&2
      else
        log_phase "eigencloud waiting for active build build_id=${active_build_id} remaining_retry_budget_secs=${remaining_retry_budget}"
        wait_for_build_terminal_state "$active_build_id" "$remaining_retry_budget"
        wait_result=$?
        if (( wait_result == 0 )); then
          log_phase "eigencloud active build complete build_id=${active_build_id}; retrying deploy command"
          deploy_attempt=$((deploy_attempt + 1))
          continue
        elif (( wait_result == 2 )); then
          echo "$deploy_output" >&2
          echo "error: eigencloud build ${active_build_id} failed; aborting deploy retry loop" >&2
          exit 1
        fi
      fi
    else
      log_phase "eigencloud queue conflict without discoverable build id; falling back to timed retry"
    fi

    if (( deploy_max_retries_raw > 0 && deploy_attempt >= deploy_max_retries_raw )); then
      echo "$deploy_output" >&2
      echo "error: deploy retry attempts exceeded (${deploy_attempt}/${deploy_max_retries_raw}) reason=${retry_reason}" >&2
      exit 1
    fi

    sleep_seconds="$deploy_retry_backoff_raw"
    if (( sleep_seconds > remaining_retry_budget )); then
      sleep_seconds="$remaining_retry_budget"
    fi
    if (( sleep_seconds < 1 )); then
      sleep_seconds=1
    fi

    echo "warning: ${retry_reason}; retrying deploy in ${sleep_seconds}s (attempt ${deploy_attempt}/${deploy_attempt_cap_display}, elapsed=${retry_elapsed}s/${deploy_retry_timeout_raw}s)" >&2
    sleep "$sleep_seconds"
    deploy_attempt=$((deploy_attempt + 1))
    continue
  fi

  echo "$deploy_output" >&2
  exit 1
done

app_id="$(printf '%s\n' "$deploy_output" | sed -n 's/^[[:space:]]*App ID:[[:space:]]*//p' | tail -n1)"
if [[ -z "$app_id" ]]; then
  app_id="$(printf '%s\n' "$deploy_output" | sed -n 's/.*\(0x[a-fA-F0-9]\{40\}\).*/\1/p' | tail -n1)"
fi
if [[ -z "$app_id" ]]; then
  echo "failed to determine app id from deploy output" >&2
  echo "$deploy_output" >&2
  exit 1
fi
log_phase "eigencloud app allocated app_id=${app_id}"

instance_ip="$(printf '%s\n' "$deploy_output" | sed -n 's/^[[:space:]]*Instance IP:[[:space:]]*//p' | tail -n1)"
app_url="$(printf '%s\n' "$deploy_output" | sed -n 's/^[[:space:]]*App URL:[[:space:]]*//p' | tail -n1)"
if [[ -z "$instance_ip" || -z "$app_url" ]]; then
  log_phase "eigencloud app info lookup app_id=${app_id}"
  info_output="$(ecloud compute app info "$app_id" --environment "$env_name" 2>&1 || true)"
  if [[ -z "$instance_ip" ]]; then
    instance_ip="$(printf '%s\n' "$info_output" | sed -n 's/^[[:space:]]*Instance IP:[[:space:]]*//p' | tail -n1)"
  fi
  if [[ -z "$app_url" ]]; then
    app_url="$(printf '%s\n' "$info_output" | sed -n 's/^[[:space:]]*App URL:[[:space:]]*//p' | tail -n1)"
  fi
fi

if [[ -z "$app_url" ]]; then
  app_base="${GATEWAY_FRONTDOOR_ECLOUD_APP_BASE_URL:-${ECLOUD_FRONTDOOR_APP_BASE_URL:-}}"
  if [[ -z "$app_base" ]]; then
    case "$env_name" in
      sepolia) app_base="https://sepolia.eigencloud.xyz/app" ;;
      mainnet) app_base="https://mainnet.eigencloud.xyz/app" ;;
    esac
  fi
  if [[ -n "$app_base" ]]; then
    app_url="${app_base%/}/${app_id}"
  fi
fi

verify_base="${GATEWAY_FRONTDOOR_VERIFY_APP_BASE_URL:-https://verify-sepolia.eigencloud.xyz/app}"
verify_url="${verify_base%/}/${app_id}"

if [[ -z "$app_url" ]]; then
  inferred_app_url="$(infer_app_url_from_verify_url "$verify_url" "$app_id")"
  if [[ -n "$inferred_app_url" ]]; then
    app_url="$inferred_app_url"
  fi
fi

instance_url="${app_url:-$verify_url}"

gateway_url=""
if [[ -n "$instance_ip" ]]; then
  log_phase "ironclaw runtime endpoint candidate ip=${instance_ip} port=${instance_port}"
  gateway_url="http://${instance_ip}:${instance_port}/gateway"
  if [[ -n "$gateway_auth_key" ]]; then
    encoded_gateway_auth_key="$(node -e 'process.stdout.write(encodeURIComponent(process.argv[1] || ""));' "$gateway_auth_key")"
    # Use URL fragment to avoid sending gateway auth token via HTTP request line/referrer.
    gateway_url="${gateway_url}#token=${encoded_gateway_auth_key}"
  fi
fi

strict_instance_init="$(printf '%s' "${ECLOUD_FRONTDOOR_STRICT_INSTANCE_INIT:-false}" | tr '[:upper:]' '[:lower:]')"
require_strict_init=0
case "$strict_instance_init" in
  true|1|yes|on) require_strict_init=1 ;;
esac

seeded=0
if [[ -n "$gateway_url" ]]; then
  log_phase "ironclaw runtime health probe start app_id=${app_id}"
  health_url="http://${instance_ip}:${instance_port}/api/health"
  import_url="http://${instance_ip}:${instance_port}/api/settings/import"
  healthy=0
  for _attempt in $(seq 1 30); do
    if curl -fsS --max-time 8 "$health_url" >/dev/null 2>&1; then
      healthy=1
      break
    fi
    sleep 2
  done

  if [[ "$healthy" -eq 1 ]]; then
    log_phase "ironclaw runtime health probe passed app_id=${app_id}; importing session settings"
    imported=0
    for _attempt in $(seq 1 20); do
      if curl -fsS --max-time 12 \
        -H "Authorization: Bearer ${gateway_auth_key}" \
        -H "Content-Type: application/json" \
        --data "$settings_payload" \
        "$import_url" >/dev/null 2>&1; then
        imported=1
        break
      fi
      sleep 2
    done

    if [[ "$imported" -eq 1 ]]; then
      seeded=1
      instance_url="$gateway_url"
      log_phase "ironclaw runtime settings import succeeded app_id=${app_id}"
    else
      echo "warning: failed importing session settings into app ${app_id}; returning app/verify URL fallback" >&2
    fi
  else
    echo "warning: instance health check timed out at ${health_url}; returning app/verify URL fallback" >&2
  fi
else
  log_phase "ironclaw runtime instance IP unavailable app_id=${app_id}; using app/verify URL"
  echo "warning: no instance IP discovered for app ${app_id}; returning app/verify URL fallback" >&2
fi

if [[ "$require_strict_init" -eq 1 && "$seeded" -ne 1 ]]; then
  echo "strict instance init enabled and gateway seeding failed for app ${app_id}" >&2
  exit 1
fi

node -e '
const instanceUrl = process.argv[1];
const appUrl = process.argv[2] || "";
const verifyUrl = process.argv[3];
const appId = process.argv[4];
const gatewayUrl = process.argv[5] || "";
const sourceRepoUrl = process.argv[6] || "";
const sourceCommit = process.argv[7] || "";
const payload = {
  instance_url: instanceUrl,
  verify_url: verifyUrl,
  app_id: appId
};
if (appUrl) payload.app_url = appUrl;
if (gatewayUrl) payload.gateway_url = gatewayUrl;
if (sourceRepoUrl) payload.source_repo_url = sourceRepoUrl;
if (sourceCommit) payload.source_commit = sourceCommit;
process.stdout.write(JSON.stringify(payload));
' "$instance_url" "$app_url" "$verify_url" "$app_id" "$gateway_url" "$source_repo_url" "$source_commit"
printf '\n'
