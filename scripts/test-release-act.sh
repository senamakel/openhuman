#!/usr/bin/env bash
# Test the Release workflow locally using act.
#
# Defaults are safe:
# - Uses scripts/ci-secrets.example.json for secrets/vars.
# - Runs in dry-run mode unless --run is passed.
#
# Usage:
#   ./scripts/test-release-act.sh
#   ./scripts/test-release-act.sh --run
#   ./scripts/test-release-act.sh --list
#   ./scripts/test-release-act.sh --job prepare-release
#   ./scripts/test-release-act.sh --release-type minor
#   ./scripts/test-release-act.sh --secrets-json scripts/ci-secrets.json --run

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

WORKFLOW=".github/workflows/release.yml"
SECRETS_JSON="scripts/ci-secrets.json"
RELEASE_TYPE="patch"
RUN_MODE="dryrun"
JOB_NAME=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --run)
      RUN_MODE="run"
      shift
      ;;
    --dryrun)
      RUN_MODE="dryrun"
      shift
      ;;
    --list)
      RUN_MODE="list"
      shift
      ;;
    --job)
      JOB_NAME="${2:-}"
      shift 2
      ;;
    --release-type)
      RELEASE_TYPE="${2:-patch}"
      shift 2
      ;;
    --secrets-json)
      SECRETS_JSON="${2:-}"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ ! -f "$SECRETS_JSON" ]]; then
  echo "Secrets JSON not found: $SECRETS_JSON" >&2
  exit 1
fi

if ! command -v act >/dev/null 2>&1; then
  echo "act is required. Install with: brew install act" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required. Install with: brew install jq" >&2
  exit 1
fi

case "$RELEASE_TYPE" in
  major|minor|patch) ;;
  *)
    echo "--release-type must be one of: major, minor, patch" >&2
    exit 1
    ;;
esac

if [[ "$RUN_MODE" == "list" ]]; then
  act -W "$WORKFLOW" --list
  exit 0
fi

SECRETS_FILE="$(mktemp)"
VARS_FILE="$(mktemp)"
EVENT_JSON="$(mktemp)"
trap 'rm -f "$SECRETS_FILE" "$VARS_FILE" "$EVENT_JSON"' EXIT

jq -r '.secrets // {} | to_entries[] | "\(.key)=\(.value)"' "$SECRETS_JSON" > "$SECRETS_FILE"
jq -r '.vars // {} | to_entries[] | "\(.key)=\(.value)"' "$SECRETS_JSON" > "$VARS_FILE"

cat > "$EVENT_JSON" <<EOF
{
  "ref": "refs/heads/main",
  "inputs": {
    "release_type": "$RELEASE_TYPE"
  },
  "repository": {
    "full_name": "local/openhuman",
    "default_branch": "main",
    "name": "openhuman",
    "owner": { "login": "local" }
  },
  "sender": { "login": "local-dev" }
}
EOF

echo "Workflow: $WORKFLOW"
echo "Secrets:  $SECRETS_JSON"
echo "Input:    release_type=$RELEASE_TYPE"
echo "Mode:     $RUN_MODE"
if [[ -n "$JOB_NAME" ]]; then
  echo "Job:      $JOB_NAME"
fi
echo

ACT_ARGS=(
  workflow_dispatch
  -W "$WORKFLOW"
  --eventpath "$EVENT_JSON"
  --secret-file "$SECRETS_FILE"
  --var-file "$VARS_FILE"
  -P ubuntu-latest=catthehacker/ubuntu:act-latest
  -P ubuntu-22.04=catthehacker/ubuntu:act-22.04
  -P macos-latest=-self-hosted
)

if [[ -n "$JOB_NAME" ]]; then
  ACT_ARGS+=(-j "$JOB_NAME")
fi

if [[ "$RUN_MODE" == "dryrun" ]]; then
  echo "Dry-run only. Use --run to execute."
  act "${ACT_ARGS[@]}" -n
else
  act "${ACT_ARGS[@]}"
fi
