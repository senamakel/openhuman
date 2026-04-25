#!/usr/bin/env bash
# Local end-to-end test of the Tauri shell auto-updater.
#
# Builds two versions of OpenHuman, generates a signed `latest.json`, hosts
# them on `localhost:8000` via `python3 -m http.server`, and points the older
# build at the local server. Run the older build to watch its in-app updater
# detect the newer release, download it, verify the signature, install it, and
# relaunch as the newer build.
#
# Prereqs:
#   - Working tree on a branch that has the Tauri shell updater plugin wired
#     (`plugins.updater.active: true` in `app/src-tauri/tauri.conf.json` and
#     `tauri-plugin-updater` registered in `lib.rs`) AND a pubkey whose
#     matching private key passphrase you actually have.
#   - Env vars (the CI release pipeline reads the same names):
#       TAURI_SIGNING_PRIVATE_KEY            — full minisign secret-key file contents
#       TAURI_SIGNING_PRIVATE_KEY_PASSWORD   — its passphrase
#   - `python3` and `pnpm` in PATH.
#
# Each Tauri build pulls in CEF + the vendored runtime fork; expect ~5–15 min
# per build the first time, faster on warm caches.
#
# Usage:
#   TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/myapp.key)" \
#   TAURI_SIGNING_PRIVATE_KEY_PASSWORD='your-passphrase' \
#     bash scripts/test-app-update-local.sh
#
#   # Override defaults:
#   OLD_VERSION=0.52.27 NEW_VERSION=0.52.28 PORT=8000 TEST_DIR=/tmp/oh-update-test \
#     bash scripts/test-app-update-local.sh

set -euo pipefail

OLD_VERSION="${OLD_VERSION:-0.52.27}"
PORT="${PORT:-8000}"
# Default to a real (non-symlinked) path. macOS resolves /tmp → /private/tmp,
# and Tauri's updater refuses to install into a symlinked path
# (`StartingBinary found current_exe() that contains a symlink…`).
TEST_DIR="${TEST_DIR:-$HOME/.openhuman-update-test}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONF="$REPO_ROOT/app/src-tauri/tauri.conf.json"
PKG="$REPO_ROOT/app/package.json"

# Default NEW_VERSION = whatever's in tauri.conf.json today, so the script's
# "newer" build is the build the rest of the repo treats as current.
NEW_VERSION="${NEW_VERSION:-$(python3 -c "import json; print(json.load(open('$CONF'))['version'])")}"

# Validate signing env. The `.app.tar.gz.sig` file is what the updater verifies
# against the embedded pubkey, so without these the test cannot proceed.
: "${TAURI_SIGNING_PRIVATE_KEY:?TAURI_SIGNING_PRIVATE_KEY not set — pass via env}"
: "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:?TAURI_SIGNING_PRIVATE_KEY_PASSWORD not set — pass via env}"

# Detect host triple → (latest.json platform key, asset arch tag).
TRIPLE="$(rustc -vV | awk '/host:/ {print $2}')"
case "$TRIPLE" in
  aarch64-apple-darwin)     PLATFORM_KEY=darwin-aarch64 ; OS=macos ;;
  x86_64-apple-darwin)      PLATFORM_KEY=darwin-x86_64  ; OS=macos ;;
  x86_64-unknown-linux-gnu) PLATFORM_KEY=linux-x86_64   ; OS=linux ;;
  *) echo "[update-test] unsupported triple: $TRIPLE"; exit 1 ;;
esac

ENDPOINT_URL="http://localhost:$PORT/latest.json"

echo "[update-test] config:"
echo "  triple        = $TRIPLE ($PLATFORM_KEY)"
echo "  OLD_VERSION   = $OLD_VERSION"
echo "  NEW_VERSION   = $NEW_VERSION"
echo "  endpoint      = $ENDPOINT_URL"
echo "  TEST_DIR      = $TEST_DIR"
echo

if [ "$OLD_VERSION" = "$NEW_VERSION" ]; then
  echo "[update-test] OLD_VERSION must differ from NEW_VERSION — bump one."
  exit 1
fi

mkdir -p "$TEST_DIR/build-a" "$TEST_DIR/serve"

# Backup configs and set up restore-on-exit. Also kill the server and any leftover
# OpenHuman processes so re-runs start clean.
SERVER_PID=""
cp "$CONF" "$CONF.bak"
cp "$PKG" "$PKG.bak"
cleanup() {
  echo
  echo "[update-test] cleanup: restoring tauri.conf.json + package.json"
  mv -f "$CONF.bak" "$CONF" 2>/dev/null || true
  mv -f "$PKG.bak"  "$PKG"  2>/dev/null || true
  if [ -n "$SERVER_PID" ]; then
    kill "$SERVER_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

# ── helpers ────────────────────────────────────────────────────────────────
patch_json() {
  # patch_json <file> <python expression mutating `d`>
  python3 - "$1" <<PY
import json, sys
p = sys.argv[1]
d = json.load(open(p))
$2
json.dump(d, open(p, "w"), indent=2)
open(p, "a").write("\n")
PY
}

set_version()        { patch_json "$CONF" "d['version'] = '$1'"; patch_json "$PKG" "d['version'] = '$1'"; }
# Tauri's updater plugin rejects non-https endpoints by default. The
# `dangerousInsecureTransportProtocol` flag bypasses that check — only safe
# for local testing, never set in committed config. Restored on exit.
set_endpoint_localhost() {
  patch_json "$CONF" "d['plugins']['updater']['endpoints'] = ['$1']
d['plugins']['updater']['dangerousInsecureTransportProtocol'] = True"
}

# ── build A (older, points at localhost) ───────────────────────────────────
echo "[update-test] === Build A (v$OLD_VERSION) — installed app under test ==="
set_version "$OLD_VERSION"
set_endpoint_localhost "$ENDPOINT_URL"

cd "$REPO_ROOT/app"
echo "[update-test] running pnpm tauri:ensure (vendored CEF-aware tauri-cli)"
pnpm tauri:ensure
echo "[update-test] running cargo tauri build (Build A, vendored CEF-aware CLI)"
CEF_PATH="$HOME/Library/Caches/tauri-cef" cargo tauri build -- --bin OpenHuman

# Locate Build A's output and copy it to TEST_DIR.
case "$OS" in
  macos)
    SRC_APP="$REPO_ROOT/app/src-tauri/target/release/bundle/macos/OpenHuman.app"
    if [ ! -d "$SRC_APP" ]; then
      echo "[update-test] expected $SRC_APP after Build A — missing. Aborting."
      exit 1
    fi
    rm -rf "$TEST_DIR/build-a/OpenHuman.app"
    cp -R "$SRC_APP" "$TEST_DIR/build-a/OpenHuman.app"
    BUILD_A_PATH="$TEST_DIR/build-a/OpenHuman.app"
    ;;
  linux)
    SRC_BIN="$REPO_ROOT/app/src-tauri/target/release/OpenHuman"
    cp "$SRC_BIN" "$TEST_DIR/build-a/OpenHuman"
    BUILD_A_PATH="$TEST_DIR/build-a/OpenHuman"
    ;;
esac
echo "[update-test] Build A staged at $BUILD_A_PATH"

# ── build B (newer, signed) ────────────────────────────────────────────────
echo
echo "[update-test] === Build B (v$NEW_VERSION) — update target, signed ==="
set_version "$NEW_VERSION"
# endpoint stays localhost — irrelevant for the target build.

# Rebuild. tauri picks up TAURI_SIGNING_PRIVATE_KEY from env automatically;
# `createUpdaterArtifacts: true` is already in tauri.conf.json on this branch.
echo "[update-test] running cargo tauri build (Build B, signed)"
CEF_PATH="$HOME/Library/Caches/tauri-cef" cargo tauri build -- --bin OpenHuman

case "$OS" in
  macos)
    APP_TARGZ=$(ls "$REPO_ROOT/app/src-tauri/target/release/bundle/macos/"*".app.tar.gz" 2>/dev/null | head -1 || true)
    SIG_FILE=$(ls  "$REPO_ROOT/app/src-tauri/target/release/bundle/macos/"*".app.tar.gz.sig" 2>/dev/null | head -1 || true)
    ;;
  linux)
    APP_TARGZ=$(ls "$REPO_ROOT/app/src-tauri/target/release/bundle/appimage/"*".AppImage.tar.gz" 2>/dev/null | head -1 || true)
    SIG_FILE=$(ls  "$REPO_ROOT/app/src-tauri/target/release/bundle/appimage/"*".AppImage.tar.gz.sig" 2>/dev/null | head -1 || true)
    ;;
esac

if [ -z "${APP_TARGZ:-}" ] || [ -z "${SIG_FILE:-}" ]; then
  echo "[update-test] FAILED to find signed updater artifacts after Build B."
  echo "             Expected .app.tar.gz + .sig. Did createUpdaterArtifacts=true and signing env make it through?"
  exit 1
fi

ASSET_NAME=$(basename "$APP_TARGZ")
cp "$APP_TARGZ" "$TEST_DIR/serve/$ASSET_NAME"
echo "[update-test] copied artifact: $TEST_DIR/serve/$ASSET_NAME"
echo "[update-test] signature      : $SIG_FILE"

# ── latest.json ────────────────────────────────────────────────────────────
SIG_PATH="$SIG_FILE" \
NEW_VERSION="$NEW_VERSION" \
ASSET_URL="http://localhost:$PORT/$ASSET_NAME" \
PLATFORM_KEY="$PLATFORM_KEY" \
LATEST_OUT="$TEST_DIR/serve/latest.json" \
python3 - <<'PY'
import json, os, datetime
sig_text = open(os.environ["SIG_PATH"]).read().strip()
data = {
    "version":  os.environ["NEW_VERSION"],
    "notes":    "Local end-to-end updater test build",
    "pub_date": datetime.datetime.utcnow().strftime("%Y-%m-%dT%H:%M:%SZ"),
    "platforms": {
        os.environ["PLATFORM_KEY"]: {
            "signature": sig_text,
            "url":       os.environ["ASSET_URL"],
        }
    },
}
with open(os.environ["LATEST_OUT"], "w") as f:
    json.dump(data, f, indent=2)
print("[update-test] wrote", os.environ["LATEST_OUT"])
PY

# ── serve + instructions ───────────────────────────────────────────────────
echo
echo "[update-test] starting python3 -m http.server $PORT in $TEST_DIR/serve"
cd "$TEST_DIR/serve"
python3 -m http.server "$PORT" >/dev/null 2>&1 &
SERVER_PID=$!
sleep 1
if ! kill -0 "$SERVER_PID" 2>/dev/null; then
  echo "[update-test] http.server failed to start. Port $PORT in use?"
  exit 1
fi

cat <<EOF

✅ Setup complete.

  Build A (v$OLD_VERSION) — the "installed" build, points at localhost:
    $BUILD_A_PATH

  Build B (v$NEW_VERSION) — signed update target, served from:
    http://localhost:$PORT/latest.json
    http://localhost:$PORT/$ASSET_NAME

To test the auto-update flow:

  1. Launch Build A:
       open '$BUILD_A_PATH'
     (Linux: '$BUILD_A_PATH')

  2. In its DevTools console (right-click → Inspect, or :9222 in dev), run:
       await window.__TAURI__.core.invoke('check_app_update')
     Expect:
       { current_version: '$OLD_VERSION',
         available: true,
         available_version: '$NEW_VERSION', ... }

  3. Trigger install + relaunch:
       await window.__TAURI__.core.invoke('apply_app_update')
     Watch the Build A terminal for [app-update] log lines and the python
     server log for GET /latest.json + GET /$ASSET_NAME.

  4. The app should download, verify the signature against the embedded
     pubkey, install in place (replacing the .app), and relaunch as v$NEW_VERSION.

Press Ctrl-C here when done — config files restore automatically.

EOF

# Block so the server stays alive and the trap fires on Ctrl-C / SIGTERM.
wait "$SERVER_PID"
