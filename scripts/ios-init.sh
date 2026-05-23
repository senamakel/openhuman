#!/usr/bin/env bash
# scripts/ios-init.sh
#
# Scaffolds the Xcode project for the iOS client via `tauri ios init`.
# Run from the repo root.
#
# The iOS host lives in `app/src-tauri-mobile/` (separate Cargo crate from
# the desktop host at `app/src-tauri/`) because the desktop crate is pinned
# to a vendored CEF Tauri fork that does not support iOS.
#
# After this script completes:
#   1. Open the generated .xcodeproj in Xcode and set your Development Team
#      (Signing & Capabilities tab).
#   2. Run `pnpm tauri:ios:dev` to start a hot-reload dev session.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MOBILE_DIR="$REPO_ROOT/app/src-tauri-mobile"

echo "[ios-init] Running tauri ios init from $MOBILE_DIR ..."
cd "$MOBILE_DIR"
# IPHONEOS_DEPLOYMENT_TARGET pins the Swift compiler target version; the PTT
# plugin (packages/tauri-plugin-ptt/) uses iOS 14+ APIs (OSLogMessage), so we
# match the Package.swift declaration of iOS 16.
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-16.0}"
npx --package=@tauri-apps/cli@^2 tauri ios init

echo ""
echo "[ios-init] Done. Next steps:"
echo ""
echo "  1. Open Xcode project:"
echo "     open app/src-tauri-mobile/gen/apple/*.xcodeproj"
echo "     Set Development Team under Signing & Capabilities."
echo ""
echo "  2. Start dev session:"
echo "     pnpm tauri:ios:dev"
echo ""
echo "See docs/ios/SETUP.md for full documentation."
