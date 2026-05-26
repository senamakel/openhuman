#!/usr/bin/env bash
# Launch a local staging build of the Tauri app.
#
# Loads signing credentials from scripts/ci-secrets.json and sets
# OPENHUMAN_APP_ENV=staging so the encrypted-file keyring backend is used.
#
# Usage:
#   bash scripts/dev-staging.sh
#   pnpm dev:staging

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
SECRETS_FILE="$ROOT_DIR/scripts/ci-secrets.json"

if [[ ! -f "$SECRETS_FILE" ]]; then
  echo "[dev-staging] $SECRETS_FILE not found — cannot load signing credentials" >&2
  exit 1
fi

# Load secrets + vars from the CI secrets file
source "$SCRIPT_DIR/load-env-json.sh" "$SECRETS_FILE" '.secrets + .vars'

# Ensure staging env
export OPENHUMAN_APP_ENV=staging
export VITE_OPENHUMAN_APP_ENV=staging

# Load the regular .env (secrets take precedence since they're already set)
source "$SCRIPT_DIR/load-dotenv.sh"

export CEF_PATH="$HOME/Library/Caches/tauri-cef"

# Chromium safe storage setup
bash "$SCRIPT_DIR/setup-chromium-safe-storage.sh"

# Ensure vendored tauri-cli
cd "$ROOT_DIR/app"
pnpm tauri:ensure

echo "[dev-staging] APPLE_SIGNING_IDENTITY=$APPLE_SIGNING_IDENTITY"
echo "[dev-staging] OPENHUMAN_APP_ENV=$OPENHUMAN_APP_ENV"
echo "[dev-staging] starting cargo tauri dev..."

cargo tauri dev
