#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PACKAGE_NAME="daily_git"
VERSION="$(sed -nE 's/^version = "([^"]+)"/\1/p' Cargo.toml | head -n1)"
TARGET_TRIPLE="${1:-$(rustc -vV | sed -n 's/^host: //p')}"

if [[ -z "$VERSION" ]]; then
  echo "failed to detect package version from Cargo.toml" >&2
  exit 1
fi

if [[ -z "$TARGET_TRIPLE" ]]; then
  echo "failed to detect Rust target triple" >&2
  exit 1
fi

if [[ "$TARGET_TRIPLE" == *windows* ]]; then
  echo "windows packaging is handled by the GitHub Actions workflow" >&2
  exit 1
fi

ARCHIVE_BASENAME="${PACKAGE_NAME}-${VERSION}-${TARGET_TRIPLE}"
STAGE_ROOT="$ROOT_DIR/target/package"
STAGE_DIR="$STAGE_ROOT/$ARCHIVE_BASENAME"
ARCHIVE_PATH="$ROOT_DIR/target/packages/${ARCHIVE_BASENAME}.tar.gz"
INSTALLER_PATH="$ROOT_DIR/target/packages/daily_git-installer.sh"

rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR/templates" "$(dirname "$ARCHIVE_PATH")"

cargo build --locked --release --target "$TARGET_TRIPLE"

cp "$ROOT_DIR/target/$TARGET_TRIPLE/release/$PACKAGE_NAME" "$STAGE_DIR/"
cp "$ROOT_DIR/README.md" "$ROOT_DIR/LICENSE" "$STAGE_DIR/"
cp "$ROOT_DIR/config.yaml" "$STAGE_DIR/config.example.yaml"
cp "$ROOT_DIR/templates/"* "$STAGE_DIR/templates/"
cp "$ROOT_DIR/scripts/install.sh" "$INSTALLER_PATH"
chmod +x "$INSTALLER_PATH"

tar -C "$STAGE_ROOT" -czf "$ARCHIVE_PATH" "$ARCHIVE_BASENAME"

echo "$ARCHIVE_PATH"
echo "$INSTALLER_PATH"
