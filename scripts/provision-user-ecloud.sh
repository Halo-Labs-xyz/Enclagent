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
name="${prefix}-${wallet#0x}"
name="${name:0:42}-${session:0:8}"
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

if [[ -n "${ECLOUD_FRONTDOOR_ENV_FILE:-}" ]]; then
  if [[ ! -f "${ECLOUD_FRONTDOOR_ENV_FILE}" ]]; then
    echo "ECLOUD_FRONTDOOR_ENV_FILE does not exist: ${ECLOUD_FRONTDOOR_ENV_FILE}" >&2
    exit 2
  fi
  cp "${ECLOUD_FRONTDOOR_ENV_FILE}" "$effective_env_file"
fi

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

deploy_output="$(ecloud "${deploy_args[@]}" 2>&1)" || {
  echo "$deploy_output" >&2
  exit 1
}

app_id="$(printf '%s\n' "$deploy_output" | sed -n 's/^[[:space:]]*App ID:[[:space:]]*//p' | tail -n1)"
if [[ -z "$app_id" ]]; then
  app_id="$(printf '%s\n' "$deploy_output" | sed -n 's/.*\(0x[a-fA-F0-9]\{40\}\).*/\1/p' | tail -n1)"
fi
if [[ -z "$app_id" ]]; then
  echo "failed to determine app id from deploy output" >&2
  echo "$deploy_output" >&2
  exit 1
fi

instance_ip="$(printf '%s\n' "$deploy_output" | sed -n 's/^[[:space:]]*Instance IP:[[:space:]]*//p' | tail -n1)"
if [[ -z "$instance_ip" ]]; then
  info_output="$(ecloud compute app info "$app_id" --environment "$env_name" 2>&1 || true)"
  instance_ip="$(printf '%s\n' "$info_output" | sed -n 's/^[[:space:]]*Instance IP:[[:space:]]*//p' | tail -n1)"
fi
if [[ -z "$instance_ip" ]]; then
  echo "failed to determine instance IP for app $app_id" >&2
  exit 1
fi

instance_url="http://${instance_ip}:${instance_port}/gateway"
if [[ -n "$gateway_auth_key" ]]; then
  encoded_gateway_auth_key="$(node -e 'process.stdout.write(encodeURIComponent(process.argv[1] || ""));' "$gateway_auth_key")"
  # Use URL fragment to avoid sending gateway auth token via HTTP request line/referrer.
  instance_url="${instance_url}#token=${encoded_gateway_auth_key}"
fi

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
if [[ "$healthy" -ne 1 ]]; then
  echo "instance did not become healthy at ${health_url}" >&2
  exit 1
fi

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
if [[ "$imported" -ne 1 ]]; then
  echo "failed importing session settings into app ${app_id}" >&2
  exit 1
fi

verify_base="${GATEWAY_FRONTDOOR_VERIFY_APP_BASE_URL:-https://verify-sepolia.eigencloud.xyz/app}"
verify_url="${verify_base%/}/${app_id}"

node -e '
const payload = {
  instance_url: process.argv[1],
  verify_url: process.argv[2],
  app_id: process.argv[3]
};
process.stdout.write(JSON.stringify(payload));
' "$instance_url" "$verify_url" "$app_id"
printf '\n'
