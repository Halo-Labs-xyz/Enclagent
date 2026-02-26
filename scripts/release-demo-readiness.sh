#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_INPUT="${1:-.env.ecloud}"
RELEASE_FRONTDOOR_E2E_MODE="${RELEASE_FRONTDOOR_E2E_MODE:-auto}"

if [[ "${ENV_INPUT}" = /* ]]; then
  ENV_FILE="${ENV_INPUT}"
else
  ENV_FILE="${REPO_ROOT}/${ENV_INPUT}"
fi

case "${RELEASE_FRONTDOOR_E2E_MODE}" in
  auto|required|off)
    ;;
  *)
    echo "[FAIL] RELEASE_FRONTDOOR_E2E_MODE must be one of: auto, required, off" >&2
    exit 2
    ;;
esac

echo "== Enclagent demo production readiness =="
echo "repo: ${REPO_ROOT}"
echo "env:  ${ENV_FILE}"
echo "frontdoor e2e gate mode: ${RELEASE_FRONTDOOR_E2E_MODE}"

echo "[1/4] rust verification gates"
"${REPO_ROOT}/scripts/verify-local.sh"

echo "[2/4] eigencloud env + strict doctor"
"${REPO_ROOT}/scripts/verify-ecloud-foundation.sh" "${ENV_FILE}"

echo "[3/4] frontdoor signed e2e gate"
case "${RELEASE_FRONTDOOR_E2E_MODE}" in
  off)
    echo "[skip] frontdoor signed e2e gate disabled (RELEASE_FRONTDOOR_E2E_MODE=off)"
    ;;
  auto)
    set +e
    "${REPO_ROOT}/scripts/verify-frontdoor-railway-signed-e2e.sh" --allow-skip "${ENV_FILE}"
    e2e_exit_code=$?
    set -e
    case "${e2e_exit_code}" in
      0)
        echo "[PASS] frontdoor signed e2e gate passed"
        ;;
      4)
        echo "[skip] frontdoor signed e2e gate skipped due to incomplete env contract"
        ;;
      *)
        echo "[FAIL] frontdoor signed e2e gate failed with exit code ${e2e_exit_code}" >&2
        exit "${e2e_exit_code}"
        ;;
    esac
    ;;
  required)
    "${REPO_ROOT}/scripts/verify-frontdoor-railway-signed-e2e.sh" "${ENV_FILE}"
    echo "[PASS] frontdoor signed e2e gate passed"
    ;;
esac

echo "[4/4] demo readiness gates complete"

echo "all demo production readiness gates passed"
