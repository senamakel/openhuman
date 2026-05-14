#!/usr/bin/env bash
#
# Unified WDIO E2E runner — one Appium session, one CEF app launch, all specs.
#
# Architecture:
#   1. Build artefacts must exist (run `pnpm test:e2e:build` first).
#   2. Clean cached app data + write a fresh E2E config.toml pointing at the
#      shared mock backend.
#   3. Launch the built CEF binary directly (cross-platform).
#   4. Wait for the CEF process to expose CDP on 127.0.0.1:19222.
#   5. Start Appium with the `chromium` driver.
#   6. Run wdio against `test/wdio.conf.ts`, which attaches to the running
#      CEF via Appium's Chromium driver. All specs share one session.
#   7. Tear everything down (Appium → CEF → workspace).
#
# Usage:
#   ./app/scripts/e2e-run-session.sh                          # whole suite
#   ./app/scripts/e2e-run-session.sh test/e2e/specs/foo.spec.ts  # single spec
#
set -euo pipefail

SPEC_ARG="${1:-}"
LOG_SUFFIX="${2:-session}"

E2E_MOCK_PORT="${E2E_MOCK_PORT:-18473}"
CEF_CDP_PORT="${CEF_CDP_PORT:-19222}"
APPIUM_PORT="${APPIUM_PORT:-4723}"
OS="$(uname)"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$APP_DIR/.." && pwd)"
cd "$APP_DIR"

CREATED_TEMP_WORKSPACE=""
APPIUM_PID=""
APP_PID=""
E2E_CONFIG_BACKUP=""
E2E_CONFIG_FILE=""

# ------------------------------------------------------------------------------
# Workspace + config
# ------------------------------------------------------------------------------
if [ -z "${OPENHUMAN_WORKSPACE:-}" ]; then
  OPENHUMAN_WORKSPACE="$(mktemp -d)"
  CREATED_TEMP_WORKSPACE="$OPENHUMAN_WORKSPACE"
  export OPENHUMAN_WORKSPACE
  echo "[runner] Using temporary OPENHUMAN_WORKSPACE: $OPENHUMAN_WORKSPACE"
else
  echo "[runner] Using OPENHUMAN_WORKSPACE from environment: $OPENHUMAN_WORKSPACE"
fi

if [ "${OPENHUMAN_SERVICE_MOCK:-0}" = "1" ] && [ -z "${OPENHUMAN_SERVICE_MOCK_STATE_FILE:-}" ]; then
  OPENHUMAN_SERVICE_MOCK_STATE_FILE="$OPENHUMAN_WORKSPACE/service-mock-state.json"
  export OPENHUMAN_SERVICE_MOCK_STATE_FILE
fi

cleanup() {
  if [ -n "$APPIUM_PID" ]; then
    echo "[runner] Stopping Appium (pid $APPIUM_PID)..."
    kill "$APPIUM_PID" 2>/dev/null || true
    wait "$APPIUM_PID" 2>/dev/null || true
  fi
  if [ -n "$APP_PID" ]; then
    echo "[runner] Stopping CEF app (pid $APP_PID)..."
    kill "$APP_PID" 2>/dev/null || true
    wait "$APP_PID" 2>/dev/null || true
  fi
  if [ -n "$CREATED_TEMP_WORKSPACE" ]; then
    rm -rf "$CREATED_TEMP_WORKSPACE"
  fi
  if [ -n "$E2E_CONFIG_BACKUP" ] && [ -f "$E2E_CONFIG_BACKUP" ]; then
    mv "$E2E_CONFIG_BACKUP" "$E2E_CONFIG_FILE"
  elif [ -n "$E2E_CONFIG_FILE" ] && [ -f "$E2E_CONFIG_FILE" ]; then
    rm -f "$E2E_CONFIG_FILE"
  fi
}
trap cleanup EXIT

export VITE_BACKEND_URL="http://127.0.0.1:${E2E_MOCK_PORT}"
export BACKEND_URL="http://127.0.0.1:${E2E_MOCK_PORT}"
export APPIUM_PORT
export CEF_CDP_PORT

echo "[runner] Killing any running OpenHuman instances..."
case "$OS" in
  Darwin) pkill -f "OpenHuman" 2>/dev/null || true ;;
  Linux)  pkill -f "OpenHuman" 2>/dev/null || true ;;
esac
sleep 1

echo "[runner] Cleaning cached app data..."
case "$OS" in
  Darwin)
    rm -rf ~/Library/WebKit/com.openhuman.app
    rm -rf ~/Library/Caches/com.openhuman.app
    rm -rf "$HOME/Library/Application Support/com.openhuman.app"
    rm -rf "$HOME/Library/Saved Application State/com.openhuman.app.savedState"
    ;;
  Linux)
    rm -rf "$HOME/.local/share/com.openhuman.app" 2>/dev/null || true
    rm -rf "$HOME/.cache/com.openhuman.app" 2>/dev/null || true
    rm -rf "$HOME/.config/com.openhuman.app" 2>/dev/null || true
    ;;
  MINGW*|MSYS*|CYGWIN*|Windows_NT)
    rm -rf "${APPDATA:-$HOME/AppData/Roaming}/com.openhuman.app" 2>/dev/null || true
    rm -rf "${LOCALAPPDATA:-$HOME/AppData/Local}/com.openhuman.app" 2>/dev/null || true
    ;;
esac

# Mock URL must reach the core sidecar — XCUITest doesn't inherit env,
# and CEF child processes won't either. Pinning via config.toml works
# on every platform.
E2E_CONFIG_DIR="$HOME/.openhuman"
E2E_CONFIG_FILE="$E2E_CONFIG_DIR/config.toml"
mkdir -p "$E2E_CONFIG_DIR"
if [ -f "$E2E_CONFIG_FILE" ]; then
  E2E_CONFIG_BACKUP="$E2E_CONFIG_FILE.e2e-backup.$$"
  cp "$E2E_CONFIG_FILE" "$E2E_CONFIG_BACKUP"
  sed -i.bak '/^api_url[[:space:]]*=/d' "$E2E_CONFIG_FILE" && rm -f "$E2E_CONFIG_FILE.bak"
  EXISTING_CONTENT="$(cat "$E2E_CONFIG_FILE")"
  printf 'api_url = "http://127.0.0.1:%s"\n%s\n' "${E2E_MOCK_PORT}" "$EXISTING_CONTENT" > "$E2E_CONFIG_FILE"
else
  printf 'api_url = "http://127.0.0.1:%s"\n' "${E2E_MOCK_PORT}" > "$E2E_CONFIG_FILE"
fi
echo "[runner] Wrote E2E config.toml with api_url=http://127.0.0.1:${E2E_MOCK_PORT}"

DIST_JS="$(ls dist/assets/index-*.js 2>/dev/null | head -1)"
if [ -z "$DIST_JS" ]; then
  echo "ERROR: No frontend bundle found at dist/assets/index-*.js." >&2
  echo "       Run 'pnpm test:e2e:build' first." >&2
  exit 1
fi
if ! grep -q "127.0.0.1:${E2E_MOCK_PORT}" "$DIST_JS"; then
  echo "ERROR: frontend bundle does NOT contain mock server URL (127.0.0.1:${E2E_MOCK_PORT})." >&2
  echo "       Run 'pnpm test:e2e:build' to rebuild." >&2
  exit 1
fi

# ------------------------------------------------------------------------------
# Resolve the built CEF binary for this platform
# ------------------------------------------------------------------------------
resolve_app_binary() {
  case "$OS" in
    Darwin)
      for base in \
        "$APP_DIR/src-tauri/target/debug/bundle/macos/OpenHuman.app/Contents/MacOS/OpenHuman" \
        "$REPO_ROOT/target/debug/bundle/macos/OpenHuman.app/Contents/MacOS/OpenHuman"; do
        if [ -x "$base" ]; then echo "$base"; return; fi
      done
      ;;
    Linux)
      for candidate in \
        "$APP_DIR/src-tauri/target/debug/OpenHuman" \
        "$REPO_ROOT/target/debug/OpenHuman"; do
        if [ -x "$candidate" ]; then echo "$candidate"; return; fi
      done
      ;;
    MINGW*|MSYS*|CYGWIN*|Windows_NT)
      for candidate in \
        "$APP_DIR/src-tauri/target/debug/OpenHuman.exe" \
        "$REPO_ROOT/target/debug/OpenHuman.exe"; do
        if [ -x "$candidate" ]; then echo "$candidate"; return; fi
      done
      ;;
  esac
}

APP_BIN="$(resolve_app_binary)"
if [ -z "${APP_BIN:-}" ] || [ ! -x "$APP_BIN" ]; then
  echo "ERROR: built OpenHuman binary not found. Run 'pnpm test:e2e:build' first." >&2
  exit 1
fi

# ------------------------------------------------------------------------------
# Launch CEF app — CDP on 19222 is already enabled in lib.rs (see CLAUDE.md).
# ------------------------------------------------------------------------------
APP_LOG="/tmp/openhuman-e2e-app-${LOG_SUFFIX}.log"
echo "[runner] Launching CEF app: $APP_BIN"
echo "[runner]   App logs: $APP_LOG"
"$APP_BIN" > "$APP_LOG" 2>&1 &
APP_PID=$!

echo "[runner] Waiting for CDP at http://127.0.0.1:${CEF_CDP_PORT}/json/version ..."
for i in $(seq 1 60); do
  if curl -sf "http://127.0.0.1:${CEF_CDP_PORT}/json/version" >/dev/null 2>&1; then
    echo "[runner] CDP is ready."
    break
  fi
  if ! kill -0 "$APP_PID" 2>/dev/null; then
    echo "ERROR: CEF app exited before CDP came up. See $APP_LOG" >&2
    exit 1
  fi
  if [ "$i" -eq 60 ]; then
    echo "ERROR: CDP did not come up within 60s. See $APP_LOG" >&2
    exit 1
  fi
  sleep 1
done

# Sanity-check that there's at least one page-type CDP target — chromedriver's
# debuggerAddress attach picks the first page target, and if the app hasn't
# opened a window yet we get a confusing "no such window" failure.
PAGE_TARGETS="$(curl -sf "http://127.0.0.1:${CEF_CDP_PORT}/json/list" 2>/dev/null | grep -c '"type": *"page"' || true)"
if [ "${PAGE_TARGETS:-0}" -lt 1 ]; then
  echo "[runner] Warning: no page-type CDP targets visible yet — Appium may have to wait."
fi

# ------------------------------------------------------------------------------
# Start Appium (chromium driver)
# ------------------------------------------------------------------------------
# shellcheck source=/dev/null
source "$SCRIPT_DIR/e2e-resolve-node-appium.sh"

# Make sure the chromium driver is installed. `appium driver list --installed`
# exits non-zero on parse errors in some Appium versions, so just attempt the
# install and ignore "already installed" output.
echo "[runner] Ensuring Appium chromium driver is installed..."
"$APPIUM_BIN" driver install --source=npm appium-chromium-driver >/dev/null 2>&1 || true

APPIUM_LOG="/tmp/appium-e2e-${LOG_SUFFIX}.log"
echo "[runner] Starting Appium on port $APPIUM_PORT"
echo "[runner]   Appium logs: $APPIUM_LOG"
"$APPIUM_BIN" --port "$APPIUM_PORT" --relaxed-security > "$APPIUM_LOG" 2>&1 &
APPIUM_PID=$!

for i in $(seq 1 30); do
  if curl -sf "http://127.0.0.1:$APPIUM_PORT/status" >/dev/null 2>&1; then
    echo "[runner] Appium is ready."
    break
  fi
  if [ "$i" -eq 30 ]; then
    echo "ERROR: Appium did not start within 30 seconds." >&2
    exit 1
  fi
  sleep 1
done

# ------------------------------------------------------------------------------
# Run WDIO
# ------------------------------------------------------------------------------
if [ -n "$SPEC_ARG" ]; then
  echo "[runner] Running single spec: $SPEC_ARG"
  pnpm exec wdio run test/wdio.conf.ts --spec "$SPEC_ARG"
else
  echo "[runner] Running full E2E suite (single shared session)..."
  pnpm exec wdio run test/wdio.conf.ts
fi
