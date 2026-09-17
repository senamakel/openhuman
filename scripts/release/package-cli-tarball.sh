#!/usr/bin/env bash
# Package the core CLI and terminal UI binaries into a release tarball and
# optionally upload it.
#
# Usage:
#   package-cli-tarball.sh <core_binary_path> <tui_binary_path> <version> <target>
#
# Environment:
#   GITHUB_TOKEN  — if set, uploads tarball + sha256 to the GitHub release
#   UPLOAD_REPO   — GitHub repo slug (default: tinyhumansai/openhuman)
#
# Example:
#   package-cli-tarball.sh target/release/openhuman-core target/release/openhuman-tui 0.5.0 aarch64-apple-darwin
set -euo pipefail

CORE_BIN_PATH="${1:?Usage: package-cli-tarball.sh <core_binary_path> <tui_binary_path> <version> <target>}"
TUI_BIN_PATH="${2:?}"
VERSION="${3:?}"
TARGET="${4:?}"
UPLOAD_REPO="${UPLOAD_REPO:-tinyhumansai/openhuman}"

TARBALL="openhuman-core-${VERSION}-${TARGET}.tar.gz"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

cp "$CORE_BIN_PATH" "$WORK/openhuman-core"
cp "$TUI_BIN_PATH" "$WORK/openhuman-tui"
chmod +x "$WORK/openhuman-core" "$WORK/openhuman-tui"
tar -czf "$TARBALL" -C "$WORK" openhuman-core openhuman-tui

# openssl dgst works on both macOS and Linux
openssl dgst -sha256 -r "$TARBALL" | awk '{print $1}' > "${TARBALL}.sha256"

echo "[package-cli] Created $TARBALL (sha256: $(cat "${TARBALL}.sha256"))"

# ── Optional upload ──────────────────────────────────────────────────────────
if [[ -n "${GITHUB_TOKEN:-}" ]]; then
  gh release upload "v${VERSION}" \
    "$TARBALL" "${TARBALL}.sha256" \
    --repo "$UPLOAD_REPO" --clobber
  echo "[package-cli] Uploaded $TARBALL to v${VERSION}"
fi
