#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_INPUT="${1:-.env.ecloud}"

if [[ "${ENV_INPUT}" = /* ]]; then
  ENV_FILE="${ENV_INPUT}"
else
  ENV_FILE="${REPO_ROOT}/${ENV_INPUT}"
fi

echo "== Enclagent demo production readiness =="
echo "repo: ${REPO_ROOT}"
echo "env:  ${ENV_FILE}"

echo "[1/3] rust verification gates"
"${REPO_ROOT}/scripts/verify-local.sh"

echo "[2/3] eigencloud env + strict doctor"
"${REPO_ROOT}/scripts/verify-ecloud-foundation.sh" "${ENV_FILE}"

echo "[3/3] demo readiness gates complete"

echo "all demo production readiness gates passed"
