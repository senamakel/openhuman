#!/usr/bin/env bash
# scripts/ios-init.sh
#
# Scaffolds the Xcode project for the iOS client via `tauri ios init`.
# Run from the repo root.
#
# After this script completes:
#   1. Copy privacy keys from app/src-tauri/Info.ios.plist into the generated
#      app/src-tauri/gen/apple/<bundle-id>_iOS/Info.plist
#   2. Open the generated .xcodeproj in Xcode and set your Development Team
#      (Signing & Capabilities tab).
#   3. Run `pnpm tauri:ios:dev` to start a hot-reload dev session.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_DIR="$REPO_ROOT/app"

echo "[ios-init] Running tauri ios init from $APP_DIR ..."
cd "$APP_DIR"
npx --package=@tauri-apps/cli@^2 tauri ios init

echo ""
echo "[ios-init] Done. Next steps:"
echo ""
echo "  1. Copy privacy keys:"
echo "     From: app/src-tauri/Info.ios.plist"
echo "     Into: app/src-tauri/gen/apple/<bundle-id>_iOS/Info.plist"
echo "     Keys: NSCameraUsageDescription, NSMicrophoneUsageDescription,"
echo "            NSSpeechRecognitionUsageDescription"
echo ""
echo "  2. Open Xcode project:"
echo "     open app/src-tauri/gen/apple/*.xcodeproj"
echo "     Set Development Team under Signing & Capabilities."
echo ""
echo "  3. Start dev session:"
echo "     pnpm tauri:ios:dev"
echo ""
echo "See docs/ios/SETUP.md for full documentation."
