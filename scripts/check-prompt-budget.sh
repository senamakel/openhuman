#!/usr/bin/env bash
# Enforce the fixed-prefix ratchet declared in `scripts/prompt-budget.limits`.
#
# Every turn ships a system prompt plus the schemas of every advertised tool,
# before the user has said anything. On this codebase that prefix reached ~37k
# tokens for the orchestrator, ~44k of which was tool schema that nothing in the
# repo measured — the whole of the prior discussion had been about prose,
# because prose was the half that was visible. This lane is the number.
#
# Like `check-kernel-floor.sh`, the ratchet only goes DOWN: it fails on growth,
# and it fails when a profile comes in *under* its limit by more than a slack
# margin without the limit being lowered, because a saving nobody ratchets is a
# saving that silently grows back.
#
# Usage: scripts/check-prompt-budget.sh [--verbose] [--write]
#
#   --write   Rewrite the limits file's numbers to the measured values. For
#             landing a deliberate reduction; never run it to make CI green.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

LIMITS="scripts/prompt-budget.limits"
VERBOSE=0
WRITE=0
for arg in "$@"; do
  case "$arg" in
    --verbose) VERBOSE=1 ;;
    --write) WRITE=1 ;;
    *) echo "unknown flag: $arg" >&2; exit 64 ;;
  esac
done

# Slack in bytes, per measured field.
#
# Prompt text moves by a few bytes for reasons nobody should have to ratchet —
# a typo fix, a renamed heading. Tool schemas do not move at all unless a schema
# changed. 512 absorbs ordinary copy-editing (~128 tokens) while still forcing a
# ratchet update for any real reduction, which starts in the thousands.
SLACK=512

# The measurement must not depend on who is logged in.
#
# `prompt-size` renders through the real session path, which injects the
# workspace's PROFILE.md / MEMORY.md / AGENTS.md and fetches live Composio
# connections. Run against a developer's own workspace the numbers move with
# their memory tree, which would make this lane fail for reasons unrelated to
# any change. A fresh empty workspace is the only reproducible baseline; it also
# means `integrations_agent` has no connected toolkit and is skipped, which is
# correct — its size is a property of the user's account, not of this repo.
WORKSPACE="$(mktemp -d "${TMPDIR:-/tmp}/openhuman-prompt-budget.XXXXXX")"
cleanup() { rm -rf "$WORKSPACE"; }
trap cleanup EXIT

BIN="target/debug/openhuman-core"
if [[ ! -x "$BIN" ]]; then
  echo "[prompt-budget] building openhuman-core …" >&2
  cargo build --manifest-path Cargo.toml --bin openhuman-core \
    --features "$(bash scripts/ci/product-features.sh)" >&2
fi

echo "[prompt-budget] measuring against hermetic workspace $WORKSPACE" >&2
if ! measured="$(RUST_LOG=error "$BIN" agent prompt-size --workspace "$WORKSPACE" --json)"; then
  echo "::error::prompt-size failed to measure" >&2
  exit 1
fi

status=0
declare -a NEW_LINES=()

while IFS= read -r raw; do
  line="${raw%%#*}"
  line="$(echo "$line" | tr -d '[:space:]')"
  if [[ -z "$line" ]]; then
    NEW_LINES+=("$raw")
    continue
  fi

  IFS=: read -r agent max_prompt max_tools extra <<< "$line"
  if [[ -n "${extra:-}" || -z "$agent" || -z "$max_prompt" || -z "$max_tools" ]]; then
    echo "::error::invalid prompt-budget limit entry: '$line'" >&2
    exit 1
  fi

  if ! read -r prompt_bytes tool_bytes tool_count max_tool_bytes max_tool_name <<< "$(
    AGENT="$agent" python3 - "$measured" <<'PY'
import json, os, sys
d = json.loads(sys.argv[1])
agent = os.environ["AGENT"]
for r in d["agents"]:
    if r["agent"] == agent and r.get("toolkit") is None:
        worst = max(r["tools"], key=lambda t: t["bytes"], default=None)
        print(r["prompt_bytes"], r["tool_bytes"], r["tool_count"],
              worst["bytes"] if worst else 0, worst["name"] if worst else "-")
        break
else:
    sys.exit(f"agent '{agent}' is in {os.path.basename('prompt-budget.limits')} "
             f"but was not measured — was it renamed or removed?")
PY
  )"; then
    status=1
    NEW_LINES+=("$raw")
    continue
  fi

  (( VERBOSE )) && echo "$agent prompt=$prompt_bytes/$max_prompt tools=$tool_bytes/$max_tools ($tool_count tools, worst $max_tool_name $max_tool_bytes B)"

  if (( prompt_bytes > max_prompt )); then
    echo "::error::prompt budget REGRESSED: '$agent' renders $prompt_bytes B of" \
         "system prompt, limit is $max_prompt. Every turn pays this." >&2
    status=1
  elif (( max_prompt - prompt_bytes > SLACK )); then
    echo "::error::prompt budget IMPROVED but was not ratcheted: '$agent' renders" \
         "$prompt_bytes B, limit is still $max_prompt. Lower it in this PR" \
         "(scripts/check-prompt-budget.sh --write) or the saving grows back." >&2
    status=1
  fi

  if (( tool_bytes > max_tools )); then
    echo "::error::tool-schema budget REGRESSED: '$agent' advertises $tool_count" \
         "tools worth $tool_bytes B, limit is $max_tools. Consider deferring the" \
         "tool rather than trimming its description." >&2
    status=1
  elif (( max_tools - tool_bytes > SLACK )); then
    echo "::error::tool-schema budget IMPROVED but was not ratcheted: '$agent' is" \
         "at $tool_bytes B against a limit of $max_tools. Lower it in this PR." >&2
    status=1
  fi

  NEW_LINES+=("$agent:$prompt_bytes:$tool_bytes")
done < "$LIMITS"

if (( WRITE )); then
  printf '%s\n' "${NEW_LINES[@]}" > "$LIMITS.tmp"
  mv "$LIMITS.tmp" "$LIMITS"
  echo "[prompt-budget] rewrote $LIMITS to measured values" >&2
  exit 0
fi

if (( status == 0 )); then
  echo "[prompt-budget] OK — every agent is within its limit" >&2
fi
exit "$status"
