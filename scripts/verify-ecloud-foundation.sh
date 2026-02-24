#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
ENV_INPUT="${1:-.env.ecloud}"

if [[ "${ENV_INPUT}" = /* ]]; then
  ENV_FILE="${ENV_INPUT}"
else
  ENV_FILE="${REPO_ROOT}/${ENV_INPUT}"
fi

if [[ ! -f "${ENV_FILE}" ]]; then
  echo "env file not found: ${ENV_FILE}" >&2
  exit 2
fi

set -a
# shellcheck disable=SC1090
source "${ENV_FILE}"
set +a

failures=0

require_non_empty() {
  local key="$1"
  if [[ -z "${!key:-}" ]]; then
    echo "[FAIL] missing required env: ${key}" >&2
    failures=$((failures + 1))
  fi
}

require_wallet_address() {
  local key="$1"
  local value="${!key:-}"
  if [[ -z "${value}" ]]; then
    echo "[FAIL] missing required wallet address: ${key}" >&2
    failures=$((failures + 1))
    return
  fi
  if [[ ! "${value}" =~ ^0x[0-9a-fA-F]{40}$ ]]; then
    echo "[FAIL] invalid wallet address format for ${key}: expected 0x + 40 hex chars" >&2
    failures=$((failures + 1))
  fi
}

require_not_placeholder() {
  local key="$1"
  local placeholder="$2"
  local value="${!key:-}"
  if [[ "${value}" == "${placeholder}" ]]; then
    echo "[FAIL] ${key} still uses placeholder value" >&2
    failures=$((failures + 1))
  fi
}

require_supported_minimax_model() {
  local model="${ANTHROPIC_MODEL:-}"
  case "${model}" in
    MiniMax-M2.5|MiniMax-M2.5-highspeed|MiniMax-M2.1|MiniMax-M2.1-highspeed|MiniMax-M2)
      ;;
    *)
      echo "[FAIL] unsupported MiniMax model in ANTHROPIC_MODEL: ${model}" >&2
      echo "[FAIL] supported values: MiniMax-M2.5, MiniMax-M2.5-highspeed, MiniMax-M2.1, MiniMax-M2.1-highspeed, MiniMax-M2" >&2
      failures=$((failures + 1))
      ;;
  esac
}

require_non_empty DATABASE_BACKEND
if [[ "${DATABASE_BACKEND}" == "libsql" || "${DATABASE_BACKEND}" == "turso" || "${DATABASE_BACKEND}" == "sqlite" ]]; then
  require_non_empty LIBSQL_PATH
else
  require_non_empty DATABASE_URL
fi

require_non_empty LLM_BACKEND
case "${LLM_BACKEND}" in
  anthropic)
    require_non_empty ANTHROPIC_API_KEY
    require_non_empty ANTHROPIC_BASE_URL
    require_non_empty ANTHROPIC_MODEL
    if [[ "${ANTHROPIC_BASE_URL}" != "https://api.minimax.io/anthropic" ]]; then
      echo "[FAIL] ANTHROPIC_BASE_URL must be https://api.minimax.io/anthropic for production MiniMax deployment" >&2
      failures=$((failures + 1))
    else
      require_supported_minimax_model
    fi
    ;;
esac
require_non_empty GATEWAY_AUTH_TOKEN
require_non_empty HYPERLIQUID_NETWORK
require_non_empty HYPERLIQUID_PAPER_LIVE_POLICY
require_non_empty HYPERLIQUID_CUSTODY_MODE
require_non_empty HYPERLIQUID_MAX_POSITION_SIZE_USD
require_non_empty HYPERLIQUID_LEVERAGE_CAP
require_non_empty HYPERLIQUID_KILL_SWITCH_ENABLED
require_non_empty HYPERLIQUID_KILL_SWITCH_BEHAVIOR
require_non_empty VERIFICATION_BACKEND
require_non_empty VERIFICATION_FALLBACK_ENABLED
require_non_empty VERIFICATION_FALLBACK_REQUIRE_SIGNED_RECEIPTS

case "${HYPERLIQUID_CUSTODY_MODE}" in
  operator_wallet)
    require_wallet_address HYPERLIQUID_OPERATOR_WALLET_ADDRESS
    ;;
  user_wallet)
    require_wallet_address HYPERLIQUID_USER_WALLET_ADDRESS
    ;;
  dual_mode)
    require_wallet_address HYPERLIQUID_OPERATOR_WALLET_ADDRESS
    require_wallet_address HYPERLIQUID_USER_WALLET_ADDRESS
    ;;
  *)
    echo "[FAIL] unsupported HYPERLIQUID_CUSTODY_MODE: ${HYPERLIQUID_CUSTODY_MODE}" >&2
    failures=$((failures + 1))
    ;;
esac

require_wallet_address HYPERLIQUID_VAULT_ADDRESS

case "${VERIFICATION_BACKEND}" in
  eigencloud_primary)
    require_non_empty EIGENCLOUD_ENDPOINT
    require_non_empty EIGENCLOUD_AUTH_SCHEME
    require_non_empty EIGENCLOUD_AUTH_TOKEN
    ;;
  fallback_only)
    if [[ "${VERIFICATION_FALLBACK_ENABLED}" != "true" && "${VERIFICATION_FALLBACK_ENABLED}" != "1" ]]; then
      echo "[FAIL] VERIFICATION_FALLBACK_ENABLED must be true when VERIFICATION_BACKEND=fallback_only" >&2
      failures=$((failures + 1))
    fi
    ;;
  *)
    echo "[FAIL] unsupported VERIFICATION_BACKEND: ${VERIFICATION_BACKEND}" >&2
    failures=$((failures + 1))
    ;;
esac

require_non_empty VERIFICATION_FALLBACK_CHAIN_PATH

require_not_placeholder GATEWAY_AUTH_TOKEN enclagent-tee-access-token
require_not_placeholder EIGENCLOUD_AUTH_TOKEN replace-with-eigencloud-token
if [[ "${LLM_BACKEND}" == "anthropic" ]]; then
  require_not_placeholder ANTHROPIC_API_KEY replace-with-minimax-api-key
fi

if (( failures > 0 )); then
  echo "ecloud foundation verification failed with ${failures} issue(s)" >&2
  exit 1
fi

echo "[PASS] env contract checks"

cd "${REPO_ROOT}"

cargo run -- doctor --strict startup
cargo run -- doctor --strict

echo "[PASS] ecloud doctor strict checks"
