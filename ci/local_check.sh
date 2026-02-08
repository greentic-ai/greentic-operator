#!/usr/bin/env bash
if [ -z "${BASH_VERSION:-}" ]; then
  exec bash "$0" "$@"
fi
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PUBLISH_DRY_RUN_TIMEOUT_SEC="${PUBLISH_DRY_RUN_TIMEOUT_SEC:-120}"

pushd "$ROOT_DIR" >/dev/null

echo "[local_check] cargo fmt --check"
cargo fmt --check
echo "[local_check] cargo clippy"
cargo clippy
echo "[local_check] cargo test"
cargo test

if command -v rg >/dev/null 2>&1; then
  HAS_PATH_DEPS="$(rg -n "path\\s*=" -g "Cargo.toml" "$ROOT_DIR" || true)"
else
  HAS_PATH_DEPS="$(grep -R -n --include "Cargo.toml" -E "path\\s*=" "$ROOT_DIR" || true)"
fi
if [[ -n "$HAS_PATH_DEPS" ]]; then
  echo "path dependencies present; skipping publish dry-run"
  PUBLISH_WORKSPACE=""
else
  echo "[local_check] prepare publish workspace (dry-run)"
  PUBLISH_WORKSPACE="$("$ROOT_DIR/ci/prepare_publish_workspace.sh" --dry-run)"
  pushd "$PUBLISH_WORKSPACE" >/dev/null
  echo "[local_check] cargo publish --dry-run -p greentic-operator"
  set +e
  if command -v timeout >/dev/null 2>&1; then
    timeout_cmd=(timeout "$PUBLISH_DRY_RUN_TIMEOUT_SEC")
  elif command -v gtimeout >/dev/null 2>&1; then
    timeout_cmd=(gtimeout "$PUBLISH_DRY_RUN_TIMEOUT_SEC")
  else
    timeout_cmd=()
  fi
  publish_output="$("${timeout_cmd[@]}" cargo publish --dry-run -p greentic-operator 2>&1)"
  publish_status=$?
  set -e
  if [[ -n "$publish_output" ]]; then
    echo "$publish_output"
  fi
  if [[ "$publish_status" -ne 0 ]]; then
    if [[ "$publish_status" -eq 124 ]]; then
      echo "Warning: publish dry-run timed out after ${PUBLISH_DRY_RUN_TIMEOUT_SEC}s."
      publish_status=0
    fi
    if echo "$publish_output" | grep -E -q "Could not resolve host|download of config.json failed|failed to download"; then
      echo "Warning: skipping publish dry-run due to missing network access."
    else
      if [[ "$publish_status" -ne 0 ]]; then
        echo "$publish_output" >&2
        exit "$publish_status"
      fi
    fi
  fi
  popd >/dev/null
fi

PACKAGE_OUT="$(mktemp -d)"
HOST_TARGET="$(rustc -vV | rg "^host:" | awk '{print $2}')"
VERSION="$(python3 - <<'PY'
import tomllib
with open("Cargo.toml", "rb") as f:
    data = tomllib.load(f)
print(data["package"]["version"])
PY
)"
echo "[local_check] package binstall artifact for $HOST_TARGET (version=$VERSION)"
"$ROOT_DIR/ci/package_binstall.sh" --target "$HOST_TARGET" --out "$PACKAGE_OUT" --version "$VERSION"

if ! ls "$PACKAGE_OUT"/greentic-operator-"$HOST_TARGET"* >/dev/null 2>&1; then
  echo "Package artifact not created." >&2
  exit 1
fi

popd >/dev/null

rm -rf "$PACKAGE_OUT"
if [[ -n "${PUBLISH_WORKSPACE:-}" ]]; then
  rm -rf "$PUBLISH_WORKSPACE"
fi
