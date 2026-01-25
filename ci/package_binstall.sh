#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET=""
OUT_DIR=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      TARGET="$2"
      shift 2
      ;;
    --out)
      OUT_DIR="$2"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$OUT_DIR" ]]; then
  echo "--out is required" >&2
  exit 1
fi

HOST_TARGET="$(rustc -vV | awk '/^host:/ {print $2}')"
if [[ -z "$TARGET" ]]; then
  TARGET="$HOST_TARGET"
fi

mkdir -p "$OUT_DIR"
pushd "$ROOT_DIR" >/dev/null

if [[ "$TARGET" != "$HOST_TARGET" ]]; then
  rustup target add "$TARGET"
  cargo build --release --target "$TARGET"
  BIN_DIR="target/$TARGET/release"
else
  cargo build --release
  BIN_DIR="target/release"
fi

BIN_NAME="greentic-operator"
if [[ "$TARGET" == *windows* ]]; then
  BIN_NAME="${BIN_NAME}.exe"
fi

if [[ "$TARGET" == *windows* ]]; then
  ARCHIVE="$OUT_DIR/greentic-operator-$TARGET.zip"
  if command -v 7z >/dev/null 2>&1; then
    7z a -tzip "$ARCHIVE" "$BIN_DIR/$BIN_NAME" >/dev/null
  else
    echo "7z is required to package Windows artifacts." >&2
    exit 1
  fi
else
  ARCHIVE="$OUT_DIR/greentic-operator-$TARGET.tar.gz"
  tar -C "$BIN_DIR" -czf "$ARCHIVE" "$BIN_NAME"
fi

popd >/dev/null

echo "$ARCHIVE"
