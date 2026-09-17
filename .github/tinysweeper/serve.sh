#!/usr/bin/env bash
# Serve one checkout of the web app for tinysweeper's UI preview.
#
# Called by `actions/ui-preview` (tinyhumansai/tinysweeper) with:
#   TS_CHECKOUT  the checkout to serve — the pull request head, or the merge-base
#   TS_PORT      the port the web bundle must answer on
# Two copies run at once (head and base), so every port here is derived from
# TS_PORT and nothing assumes a shared build directory.
#
# This is the first half of app/scripts/e2e-web-session.sh — mock backend,
# standalone core, static web host — without the spec run. The web bundle is
# built per checkout because it bakes the core's RPC URL in at build time.
# The core is built *once*, for the head, by the workflow's setup step and
# handed to both sides through TS_SHARED_CORE_BIN: building it twice is ten
# minutes of CI for a binary that is identical whenever the pull request
# touches only the frontend, and a core-only change shows only its frontend
# effect in the preview, which is documented and accepted.
set -euo pipefail

cd "$TS_CHECKOUT"
REPO_ROOT="$(pwd)"
APP_DIR="$REPO_ROOT/app"

CORE_PORT=$((TS_PORT + 10000))
MOCK_PORT=$((TS_PORT + 20000))
CORE_TOKEN="ui-preview-core-token"
WORKSPACE="$(mktemp -d)"

export VITE_BACKEND_URL="http://127.0.0.1:${MOCK_PORT}"
export VITE_OPENHUMAN_TARGET="web"
export VITE_OPENHUMAN_E2E_DEFAULT_CORE_MODE="cloud"
export VITE_OPENHUMAN_E2E_RESTART_APP_AS_RELOAD="true"
export VITE_OPENHUMAN_CORE_RPC_URL="http://127.0.0.1:${CORE_PORT}/rpc"
export VITE_CHAT_ATTACHMENTS="true"

if [ -f "$REPO_ROOT/.env" ]; then
  # shellcheck source=/dev/null
  source "$REPO_ROOT/scripts/load-dotenv.sh"
fi

echo "[ui-preview :$TS_PORT] building the web bundle"
(cd "$APP_DIR" && pnpm run build:web)

# The bootstrap page the action opens first (`auth.visit` in
# .github/tinysweeper/ui-preview.json). Same job as the dev server's `/__dev-connect`
# route: seed the runtime endpoint, its bearer and the cloud mode into
# localStorage, then go to the app. Written here because the RPC URL depends
# on this side's core port, which only this script knows.
cat > "$APP_DIR/dist-web/__preview-connect.html" <<CONNECT
<!doctype html><meta charset="utf-8"><title>connecting</title><script>
localStorage.setItem("openhuman_core_rpc_url", "http://127.0.0.1:${CORE_PORT}/rpc");
localStorage.setItem("openhuman_core_rpc_token", "${CORE_TOKEN}");
localStorage.setItem("openhuman_core_mode", "cloud");
localStorage.setItem("openhuman:walkthrough_completed", "true");
localStorage.removeItem("openhuman:walkthrough_pending");
location.replace("/#/chat");
</script>
CONNECT

if [ -n "${TS_SHARED_CORE_BIN:-}" ] && [ -x "$TS_SHARED_CORE_BIN" ]; then
  CORE_BIN="$TS_SHARED_CORE_BIN"
else
  echo "[ui-preview :$TS_PORT] no shared core; building one"
  RUST_HOST_TRIPLE="$(rustc -vV | awk '/^host: / { print $2 }')"
  TARGET_DIR="$REPO_ROOT/target/ui-preview-${RUST_HOST_TRIPLE}"
  PRODUCT_FEATURES="$(bash "$REPO_ROOT/scripts/ci/product-features.sh")"
  CARGO_TARGET_DIR="$TARGET_DIR" cargo build --manifest-path "$REPO_ROOT/Cargo.toml" --bin openhuman-core --features "$PRODUCT_FEATURES"
  CORE_BIN="$TARGET_DIR/debug/openhuman-core"
fi

cleanup() {
  local status=$?
  set +e
  [ -n "${WEB_PID:-}" ] && kill "$WEB_PID" 2>/dev/null
  [ -n "${CORE_PID:-}" ] && kill "$CORE_PID" 2>/dev/null
  [ -n "${MOCK_PID:-}" ] && kill "$MOCK_PID" 2>/dev/null
  rm -rf "$WORKSPACE"
  return "$status"
}
trap cleanup EXIT INT TERM

wait_for_http() {
  for _ in $(seq 1 90); do
    curl -fsS "$1" >/dev/null 2>&1 && return 0
    sleep 1
  done
  echo "[ui-preview :$TS_PORT] $2 did not become ready at $1" >&2
  return 1
}

# The same workspace config the web e2e suite uses: every provider points at
# the mock, so nothing the preview does can reach a real service.
cat > "$WORKSPACE/config.toml" <<EOF
api_url = "http://127.0.0.1:${MOCK_PORT}"
primary_cloud = "p_e2e_mock"
default_model = "e2e-mock-model"
chat_provider = "e2e:e2e-mock-model"
reasoning_provider = "e2e:e2e-mock-model"
agentic_provider = "e2e:e2e-mock-model"
coding_provider = "e2e:e2e-mock-model"

[update]
enabled = false

[[cloud_providers]]
id = "p_e2e_mock"
slug = "e2e"
label = "E2E Mock"
endpoint = "http://127.0.0.1:${MOCK_PORT}/openai/v1"
auth_style = "none"
default_model = "e2e-mock-model"

[[cloud_providers]]
id = "p_e2e_openhuman"
slug = "openhuman"
label = "OpenHuman"
endpoint = "http://127.0.0.1:${MOCK_PORT}/v1"
auth_style = "openhumanjwt"
EOF

node "$REPO_ROOT/scripts/mock-api-server.mjs" --port "$MOCK_PORT" >"$WORKSPACE/mock.log" 2>&1 &
MOCK_PID=$!
wait_for_http "http://127.0.0.1:${MOCK_PORT}/__admin/health" "mock backend"

export OPENHUMAN_WORKSPACE="$WORKSPACE"
export OPENHUMAN_KEYRING_BACKEND="${OPENHUMAN_KEYRING_BACKEND:-file}"
export OPENHUMAN_CORE_TOKEN="$CORE_TOKEN"
export OPENHUMAN_TELEGRAM_BOT_API_BASE="http://127.0.0.1:${MOCK_PORT}"
export OPENHUMAN_COMPOSIO_DIRECT_BASE_V2="http://127.0.0.1:${MOCK_PORT}"
export OPENHUMAN_COMPOSIO_DIRECT_BASE_V3="http://127.0.0.1:${MOCK_PORT}"
export RUST_MIN_STACK="${RUST_MIN_STACK:-16777216}"

"$CORE_BIN" run --host 127.0.0.1 --port "$CORE_PORT" >"$WORKSPACE/core.log" 2>&1 &
CORE_PID=$!
if ! wait_for_http "http://127.0.0.1:${CORE_PORT}/health" "standalone core"; then
  tail -50 "$WORKSPACE/core.log" >&2
  exit 1
fi

# Log a preview user in, the way app/test/playwright/helpers/core-rpc.ts
# does: the session token lives in the core, not the browser, so it is
# seeded here over RPC rather than through the action's cookie/localStorage
# auth. The JWT is unsigned (`alg: none`) and the mock backend never checks
# it; it unlocks nothing real.
rpc() {
  curl -fsS "http://127.0.0.1:${CORE_PORT}/rpc" \
    -H 'Content-Type: application/json' \
    -H "Authorization: Bearer ${CORE_TOKEN}" \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":$2}" >/dev/null
}
for _ in $(seq 1 30); do
  rpc "core.ping" '{}' 2>/dev/null && break
  sleep 1
done
PAYLOAD="$(printf '{"sub":"ui-preview-user","userId":"ui-preview-user","exp":%d}' $(( $(date +%s) + 7200 )) | base64 -w0 | tr '+/' '-_' | tr -d '=')"
rpc "openhuman.auth_clear_session" '{}'
rpc "openhuman.config_set_onboarding_completed" '{"value":true}'
rpc "openhuman.auth_store_session" "{\"token\":\"eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.${PAYLOAD}.sig\"}"

echo "[ui-preview :$TS_PORT] serving $APP_DIR/dist-web"
python3 -m http.server "$TS_PORT" --bind 127.0.0.1 --directory "$APP_DIR/dist-web" >"$WORKSPACE/web.log" 2>&1 &
WEB_PID=$!
wait "$WEB_PID"
