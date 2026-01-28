#!/usr/bin/env bash
if [ -z "${BASH_VERSION:-}" ]; then
  exec bash "$0" "$@"
fi
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

pushd "$ROOT_DIR" >/dev/null

cargo fmt --check
cargo clippy
cargo test

PUBLISH_WORKSPACE="$("$ROOT_DIR/ci/prepare_publish_workspace.sh" --dry-run)"
if [[ -f "$PUBLISH_WORKSPACE/.path-deps" ]]; then
  echo "path dependencies remain in publish workspace; skipping publish dry-run"
else
  pushd "$PUBLISH_WORKSPACE" >/dev/null
  cargo publish --dry-run -p greentic-operator
  popd >/dev/null
fi

PACKAGE_OUT="$(mktemp -d)"
HOST_TARGET="$(rustc -vV | rg "^host:" | awk '{print $2}')"
"$ROOT_DIR/ci/package_binstall.sh" --target "$HOST_TARGET" --out "$PACKAGE_OUT"

if ! ls "$PACKAGE_OUT"/greentic-operator-"$HOST_TARGET"* >/dev/null 2>&1; then
  echo "Package artifact not created." >&2
  exit 1
fi

popd >/dev/null

rm -rf "$PUBLISH_WORKSPACE" "$PACKAGE_OUT"
