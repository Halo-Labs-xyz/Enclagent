#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

ENV_INPUT=".env.ecloud"
ALLOW_SKIP=0

usage() {
  cat <<'USAGE'
usage: verify-frontdoor-railway-signed-e2e.sh [--allow-skip] [env-file]

Runs a signed frontdoor E2E flow against Railway staging and production:
challenge -> verify -> poll until terminal status.

Required env contract:
  FRONTDOOR_E2E_STAGING_BASE_URL
  FRONTDOOR_E2E_PRODUCTION_BASE_URL
  FRONTDOOR_E2E_PRIVATE_KEY
  FRONTDOOR_E2E_WALLET_ADDRESS

Optional env contract:
  FRONTDOOR_E2E_CHAIN_ID                           (default: 1)
  FRONTDOOR_E2E_PRIVY_USER_ID                      (default: empty)
  FRONTDOOR_E2E_DOMAIN                             (default: general)
  FRONTDOOR_E2E_POLL_TIMEOUT_SECS                  (default: 240)
  FRONTDOOR_E2E_POLL_INTERVAL_SECS                 (default: 3)
  FRONTDOOR_E2E_REQUIRED_PROVISIONING_SOURCE       (default: command; set any to disable)
  FRONTDOOR_E2E_REQUIRE_DEDICATED_INSTANCE         (default: true)
  FRONTDOOR_E2E_REQUIRE_LAUNCHED_ON_EIGENCLOUD     (default: true)
  FRONTDOOR_E2E_FAILED_SESSION_ALERT_THRESHOLD     (default: 3; 0 disables threshold)
  FRONTDOOR_E2E_FAILED_SESSION_ALERT_LIMIT         (default: 25)

Exit codes:
  0   checks passed
  1   runtime/API/assertion failure
  2   env contract failure
  4   skipped (only when --allow-skip is set and contract is incomplete)
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --allow-skip)
      ALLOW_SKIP=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      if [[ "${ENV_INPUT}" != ".env.ecloud" ]]; then
        echo "[FAIL] unexpected extra argument: $1" >&2
        usage >&2
        exit 2
      fi
      ENV_INPUT="$1"
      shift
      ;;
  esac
done

if [[ "${ENV_INPUT}" = /* ]]; then
  ENV_FILE="${ENV_INPUT}"
else
  ENV_FILE="${REPO_ROOT}/${ENV_INPUT}"
fi

if [[ ! -f "${ENV_FILE}" ]]; then
  echo "[FAIL] env file not found: ${ENV_FILE}" >&2
  exit 2
fi

set -a
# shellcheck disable=SC1090
source "${ENV_FILE}"
set +a

fail() {
  echo "[FAIL] $*" >&2
  exit 1
}

contract_fail_or_skip() {
  local message="$1"
  if [[ "${ALLOW_SKIP}" == "1" ]]; then
    echo "[SKIP] ${message}" >&2
    exit 4
  fi
  echo "[FAIL] ${message}" >&2
  exit 2
}

require_command() {
  local command_name="$1"
  if ! command -v "${command_name}" >/dev/null 2>&1; then
    fail "required command is missing: ${command_name}"
  fi
}

parse_positive_int() {
  local key="$1"
  local default_value="$2"
  local raw="${!key:-$default_value}"
  if [[ ! "${raw}" =~ ^[0-9]+$ ]] || (( raw <= 0 )); then
    contract_fail_or_skip "${key} must be a positive integer (current: ${raw})"
  fi
  printf '%s' "${raw}"
}

parse_non_negative_int() {
  local key="$1"
  local default_value="$2"
  local raw="${!key:-$default_value}"
  if [[ ! "${raw}" =~ ^[0-9]+$ ]]; then
    contract_fail_or_skip "${key} must be a non-negative integer (current: ${raw})"
  fi
  printf '%s' "${raw}"
}

parse_bool() {
  local raw="${1:-}"
  local normalized
  normalized="$(printf '%s' "${raw}" | tr '[:upper:]' '[:lower:]')"
  case "${normalized}" in
    true|1|yes|on) printf '%s' "true" ;;
    false|0|no|off) printf '%s' "false" ;;
    *) contract_fail_or_skip "invalid boolean value: ${raw}" ;;
  esac
}

normalize_wallet() {
  local value="$1"
  printf '%s' "${value}" | tr '[:upper:]' '[:lower:]'
}

ensure_wallet_format() {
  local key="$1"
  local value="$2"
  if [[ ! "${value}" =~ ^0x[0-9a-fA-F]{40}$ ]]; then
    contract_fail_or_skip "${key} must be 0x + 40 hex chars (current: ${value})"
  fi
}

ensure_http_url() {
  local key="$1"
  local value="$2"
  if [[ ! "${value}" =~ ^https?://.+ ]]; then
    contract_fail_or_skip "${key} must be an http(s) URL (current: ${value})"
  fi
}

url_encode() {
  jq -nr --arg value "$1" '$value|@uri'
}

api_request() {
  local method="$1"
  local url="$2"
  local body="${3:-}"
  local response
  local response_body
  local status_code

  if [[ -n "${body}" ]]; then
    if ! response="$(
      curl -sS \
        --connect-timeout 10 \
        --max-time 30 \
        -X "${method}" \
        -H "Content-Type: application/json" \
        --data "${body}" \
        -w $'\n%{http_code}' \
        "${url}"
    )"; then
      fail "request failed: ${method} ${url}"
    fi
  else
    if ! response="$(
      curl -sS \
        --connect-timeout 10 \
        --max-time 30 \
        -X "${method}" \
        -w $'\n%{http_code}' \
        "${url}"
    )"; then
      fail "request failed: ${method} ${url}"
    fi
  fi

  status_code="${response##*$'\n'}"
  response_body="${response%$'\n'*}"

  if [[ ! "${status_code}" =~ ^2[0-9][0-9]$ ]]; then
    fail "non-2xx response (${status_code}) from ${url}: ${response_body}"
  fi

  if ! jq -e . >/dev/null 2>&1 <<<"${response_body}"; then
    fail "non-JSON response from ${url}: ${response_body}"
  fi

  printf '%s' "${response_body}"
}

check_failed_session_alert_threshold() {
  local environment_name="$1"
  local base_url="$2"
  local wallet_address="$3"

  if (( FRONTDOOR_E2E_FAILED_SESSION_ALERT_THRESHOLD == 0 )); then
    echo "[${environment_name}] failed-session alert threshold disabled"
    return
  fi

  local encoded_wallet
  local sessions_json
  local failed_count

  encoded_wallet="$(url_encode "${wallet_address}")"
  sessions_json="$(api_request GET "${base_url}/api/frontdoor/sessions?wallet_address=${encoded_wallet}&limit=${FRONTDOOR_E2E_FAILED_SESSION_ALERT_LIMIT}")"
  failed_count="$(jq '[.sessions[]? | select(.status == "failed")] | length' <<<"${sessions_json}")"

  if [[ ! "${failed_count}" =~ ^[0-9]+$ ]]; then
    fail "[${environment_name}] unable to parse failed session count"
  fi

  if (( failed_count >= FRONTDOOR_E2E_FAILED_SESSION_ALERT_THRESHOLD )); then
    fail "[${environment_name}] failed-session alert triggered: failed=${failed_count}, threshold=${FRONTDOOR_E2E_FAILED_SESSION_ALERT_THRESHOLD}, sample_limit=${FRONTDOOR_E2E_FAILED_SESSION_ALERT_LIMIT}"
  fi

  echo "[${environment_name}] failed-session alert check passed: failed=${failed_count}, threshold=${FRONTDOOR_E2E_FAILED_SESSION_ALERT_THRESHOLD}, sample_limit=${FRONTDOOR_E2E_FAILED_SESSION_ALERT_LIMIT}"
}

run_environment_e2e() {
  local environment_name="$1"
  local base_url="${2%/}"

  local bootstrap_json
  local bootstrap_enabled
  local dynamic_provisioning_enabled
  local challenge_request
  local challenge_json
  local session_id
  local challenge_message
  local suggest_request
  local suggest_json
  local config_json
  local gateway_auth_key
  local signature
  local verify_request
  local verify_json
  local verify_status
  local session_json
  local session_status
  local session_detail
  local session_error
  local provisioning_source
  local dedicated_instance
  local launched_on_eigencloud
  local deadline_epoch
  local now_epoch

  echo "== [${environment_name}] frontdoor signed e2e =="
  bootstrap_json="$(api_request GET "${base_url}/api/frontdoor/bootstrap")"
  bootstrap_enabled="$(jq -r '.enabled // false' <<<"${bootstrap_json}")"
  dynamic_provisioning_enabled="$(jq -r '.dynamic_provisioning_enabled // false' <<<"${bootstrap_json}")"

  if [[ "${bootstrap_enabled}" != "true" ]]; then
    fail "[${environment_name}] frontdoor bootstrap reports enabled=false"
  fi

  if [[ "${REQUIRED_PROVISIONING_SOURCE}" == "command" && "${dynamic_provisioning_enabled}" != "true" ]]; then
    fail "[${environment_name}] command provisioning required but dynamic_provisioning_enabled=false"
  fi

  challenge_request="$(jq -cn \
    --arg wallet "${EXPECTED_WALLET_ADDRESS}" \
    --arg privy "${FRONTDOOR_E2E_PRIVY_USER_ID:-}" \
    --argjson chain_id "${FRONTDOOR_E2E_CHAIN_ID}" \
    'if $privy == "" then
       {wallet_address: $wallet, chain_id: $chain_id}
     else
       {wallet_address: $wallet, privy_user_id: $privy, chain_id: $chain_id}
     end'
  )"
  challenge_json="$(api_request POST "${base_url}/api/frontdoor/challenge" "${challenge_request}")"
  session_id="$(jq -r '.session_id // empty' <<<"${challenge_json}")"
  challenge_message="$(jq -r '.message // empty' <<<"${challenge_json}")"

  if [[ -z "${session_id}" || -z "${challenge_message}" ]]; then
    fail "[${environment_name}] challenge response missing session_id or message"
  fi

  gateway_auth_key="$(printf 'fd-e2e-%s' "${session_id//-/}" | cut -c1-32)"
  suggest_request="$(jq -cn \
    --arg wallet "${EXPECTED_WALLET_ADDRESS}" \
    --arg domain "${FRONTDOOR_E2E_DOMAIN}" \
    --arg gateway_auth_key "${gateway_auth_key}" \
    --arg intent "railway frontdoor signed e2e verification" \
    '{wallet_address: $wallet, domain: $domain, gateway_auth_key: $gateway_auth_key, intent: $intent}'
  )"
  suggest_json="$(api_request POST "${base_url}/api/frontdoor/suggest-config" "${suggest_request}")"
  config_json="$(jq -ce '.config' <<<"${suggest_json}")"

  if [[ -z "${config_json}" || "${config_json}" == "null" ]]; then
    fail "[${environment_name}] suggest-config did not return a config payload"
  fi

  signature="$(cast wallet sign --private-key "${FRONTDOOR_E2E_PRIVATE_KEY}" "${challenge_message}" | tr -d '\r\n')"
  if [[ ! "${signature}" =~ ^0x[0-9a-fA-F]{130}$ ]]; then
    fail "[${environment_name}] cast returned malformed signature"
  fi

  if ! cast wallet verify --address "${EXPECTED_WALLET_ADDRESS}" "${challenge_message}" "${signature}" >/dev/null 2>&1; then
    fail "[${environment_name}] local signature verification failed before API verify call"
  fi

  verify_request="$(jq -cn \
    --arg session_id "${session_id}" \
    --arg wallet "${EXPECTED_WALLET_ADDRESS}" \
    --arg privy "${FRONTDOOR_E2E_PRIVY_USER_ID:-}" \
    --arg message "${challenge_message}" \
    --arg signature "${signature}" \
    --argjson config "${config_json}" \
    '{
      session_id: $session_id,
      wallet_address: $wallet,
      message: $message,
      signature: $signature,
      config: $config
    } + (if $privy == "" then {} else {privy_user_id: $privy} end)'
  )"
  verify_json="$(api_request POST "${base_url}/api/frontdoor/verify" "${verify_request}")"
  verify_status="$(jq -r '.status // empty' <<<"${verify_json}")"

  if [[ "${verify_status}" != "provisioning" && "${verify_status}" != "ready" ]]; then
    fail "[${environment_name}] verify returned unexpected status: ${verify_status}"
  fi

  deadline_epoch=$(( $(date +%s) + FRONTDOOR_E2E_POLL_TIMEOUT_SECS ))
  while true; do
    session_json="$(api_request GET "${base_url}/api/frontdoor/session/${session_id}")"
    session_status="$(jq -r '.status // empty' <<<"${session_json}")"
    session_detail="$(jq -r '.detail // ""' <<<"${session_json}")"
    session_error="$(jq -r '.error // ""' <<<"${session_json}")"
    provisioning_source="$(jq -r '.provisioning_source // "unknown"' <<<"${session_json}")"
    dedicated_instance="$(jq -r '.dedicated_instance // false' <<<"${session_json}")"
    launched_on_eigencloud="$(jq -r '.launched_on_eigencloud // false' <<<"${session_json}")"

    echo "[${environment_name}] session=${session_id} status=${session_status} source=${provisioning_source} dedicated=${dedicated_instance}"

    case "${session_status}" in
      ready)
        break
        ;;
      failed|expired)
        fail "[${environment_name}] session ended in ${session_status}: detail=${session_detail}; error=${session_error}"
        ;;
      awaiting_signature|provisioning)
        ;;
      *)
        fail "[${environment_name}] unexpected terminal status: ${session_status}"
        ;;
    esac

    now_epoch="$(date +%s)"
    if (( now_epoch >= deadline_epoch )); then
      fail "[${environment_name}] polling timeout after ${FRONTDOOR_E2E_POLL_TIMEOUT_SECS}s: last_status=${session_status}; detail=${session_detail}"
    fi

    sleep "${FRONTDOOR_E2E_POLL_INTERVAL_SECS}"
  done

  if [[ "${REQUIRED_PROVISIONING_SOURCE}" != "any" && "${provisioning_source}" != "${REQUIRED_PROVISIONING_SOURCE}" ]]; then
    fail "[${environment_name}] provisioning_source expected ${REQUIRED_PROVISIONING_SOURCE}, got ${provisioning_source}"
  fi

  if [[ "${REQUIRE_DEDICATED_INSTANCE}" == "true" && "${dedicated_instance}" != "true" ]]; then
    fail "[${environment_name}] dedicated_instance=false but FRONTDOOR_E2E_REQUIRE_DEDICATED_INSTANCE=true"
  fi

  if [[ "${REQUIRE_LAUNCHED_ON_EIGENCLOUD}" == "true" && "${launched_on_eigencloud}" != "true" ]]; then
    fail "[${environment_name}] launched_on_eigencloud=false but FRONTDOOR_E2E_REQUIRE_LAUNCHED_ON_EIGENCLOUD=true"
  fi

  check_failed_session_alert_threshold "${environment_name}" "${base_url}" "${EXPECTED_WALLET_ADDRESS}"
  echo "[PASS] [${environment_name}] frontdoor signed e2e flow reached ready status"
}

require_command curl
require_command jq
require_command cast

missing_contract_keys=()
for key in \
  FRONTDOOR_E2E_STAGING_BASE_URL \
  FRONTDOOR_E2E_PRODUCTION_BASE_URL \
  FRONTDOOR_E2E_PRIVATE_KEY \
  FRONTDOOR_E2E_WALLET_ADDRESS; do
  if [[ -z "${!key:-}" ]]; then
    missing_contract_keys+=("${key}")
  fi
done

if (( ${#missing_contract_keys[@]} > 0 )); then
  contract_fail_or_skip "missing required env contract key(s): ${missing_contract_keys[*]}"
fi

ensure_http_url FRONTDOOR_E2E_STAGING_BASE_URL "${FRONTDOOR_E2E_STAGING_BASE_URL}"
ensure_http_url FRONTDOOR_E2E_PRODUCTION_BASE_URL "${FRONTDOOR_E2E_PRODUCTION_BASE_URL}"
ensure_wallet_format FRONTDOOR_E2E_WALLET_ADDRESS "${FRONTDOOR_E2E_WALLET_ADDRESS}"

EXPECTED_WALLET_ADDRESS="$(normalize_wallet "${FRONTDOOR_E2E_WALLET_ADDRESS}")"
DERIVED_WALLET_ADDRESS="$(normalize_wallet "$(cast wallet address --private-key "${FRONTDOOR_E2E_PRIVATE_KEY}")")"
if [[ "${DERIVED_WALLET_ADDRESS}" != "${EXPECTED_WALLET_ADDRESS}" ]]; then
  contract_fail_or_skip "FRONTDOOR_E2E_PRIVATE_KEY does not match FRONTDOOR_E2E_WALLET_ADDRESS (derived ${DERIVED_WALLET_ADDRESS})"
fi

FRONTDOOR_E2E_CHAIN_ID="$(parse_positive_int FRONTDOOR_E2E_CHAIN_ID 1)"
FRONTDOOR_E2E_POLL_TIMEOUT_SECS="$(parse_positive_int FRONTDOOR_E2E_POLL_TIMEOUT_SECS 240)"
FRONTDOOR_E2E_POLL_INTERVAL_SECS="$(parse_positive_int FRONTDOOR_E2E_POLL_INTERVAL_SECS 3)"
FRONTDOOR_E2E_FAILED_SESSION_ALERT_THRESHOLD="$(parse_non_negative_int FRONTDOOR_E2E_FAILED_SESSION_ALERT_THRESHOLD 3)"
FRONTDOOR_E2E_FAILED_SESSION_ALERT_LIMIT="$(parse_positive_int FRONTDOOR_E2E_FAILED_SESSION_ALERT_LIMIT 25)"
FRONTDOOR_E2E_DOMAIN="${FRONTDOOR_E2E_DOMAIN:-general}"
REQUIRED_PROVISIONING_SOURCE="${FRONTDOOR_E2E_REQUIRED_PROVISIONING_SOURCE:-command}"
REQUIRE_DEDICATED_INSTANCE="$(parse_bool "${FRONTDOOR_E2E_REQUIRE_DEDICATED_INSTANCE:-true}")"
REQUIRE_LAUNCHED_ON_EIGENCLOUD="$(parse_bool "${FRONTDOOR_E2E_REQUIRE_LAUNCHED_ON_EIGENCLOUD:-true}")"

case "${REQUIRED_PROVISIONING_SOURCE}" in
  command|default_instance_url|unknown|unconfigured|any)
    ;;
  *)
    contract_fail_or_skip "FRONTDOOR_E2E_REQUIRED_PROVISIONING_SOURCE must be one of: command, default_instance_url, unknown, unconfigured, any"
    ;;
esac

if (( FRONTDOOR_E2E_FAILED_SESSION_ALERT_THRESHOLD > FRONTDOOR_E2E_FAILED_SESSION_ALERT_LIMIT )); then
  contract_fail_or_skip "FRONTDOOR_E2E_FAILED_SESSION_ALERT_THRESHOLD cannot exceed FRONTDOOR_E2E_FAILED_SESSION_ALERT_LIMIT"
fi

echo "== Railway frontdoor signed E2E verification =="
echo "env file: ${ENV_FILE}"
echo "wallet: ${EXPECTED_WALLET_ADDRESS}"
echo "required provisioning_source: ${REQUIRED_PROVISIONING_SOURCE}"

run_environment_e2e "staging" "${FRONTDOOR_E2E_STAGING_BASE_URL}"
run_environment_e2e "production" "${FRONTDOOR_E2E_PRODUCTION_BASE_URL}"

echo "[PASS] Railway frontdoor signed E2E checks passed (staging + production)"
