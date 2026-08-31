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
#
# Two entry shapes, both ratchets:
#   <agent>:<max prompt bytes>:<max tool-schema bytes>
#   tool:<name>:<max bytes>     - one tool's own schema
#
# The per-tool ratchet exists because the per-agent number hides its own
# causes: `workflow_builder` sat at 33,948 B with a single tool accounting for
# 7,415 of it. A tool over TOOL_ATTENTION_BYTES must be recorded in the limits
# file, so growth in one schema cannot hide inside an agent total that is still
# under its own limit.
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

# The size at which one tool's schema is worth an explicit decision.
#
# 1,600 bytes is ~400 tokens, the point at which a schema costs more than most
# prose sections of the system prompt. Crossing it is not a bug; crossing it
# without anyone noticing is. A tool above this line must appear in the limits
# file, which is where the reason goes.
#
# Deliberately a ratchet rather than a hard cap. A cap failing above ~800
# tokens would reject `memory` (3,788 B) and `cron` (3,228 B), which are that
# size precisely because they replaced families of eleven and six - the
# consolidation this budget exists to encourage. What matters is that a number
# moves and someone looks, not that every tool fits one shape.
TOOL_ATTENTION_BYTES=1600

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
cleanup() { rm -rf "$WORKSPACE" "${TOOL_LOOKUP:-}" "${TOOL_OVERSIZE:-}"; }
trap cleanup EXIT

BIN="target/debug/openhuman-core"
if [[ ! -x "$BIN" ]]; then
  echo "[prompt-budget] building openhuman-core …" >&2
  cargo build --manifest-path Cargo.toml --bin openhuman-core \
    --features "$(bash scripts/ci/product-features.sh)" >&2
fi

echo "[prompt-budget] measuring against hermetic workspace $WORKSPACE" >&2
if ! measured="$(RUST_LOG=error "$BIN" agent prompt-size --workspace "$WORKSPACE/workspace" --hermetic --json)"; then
  echo "::error::prompt-size failed to measure" >&2
  exit 1
fi

# Helper python written to temp files rather than inline heredocs: a heredoc
# inside the `while read` loop would consume the loop's own stdin.
TOOL_LOOKUP="$(mktemp)"
TOOL_OVERSIZE="$(mktemp)"
cat > "$TOOL_LOOKUP" <<'PYLOOKUP'
import json, os, sys
d = json.loads(sys.argv[1])
tool = os.environ["TOOL"]
for agent in d["agents"]:
    for t in agent["tools"]:
        if t["name"] == tool:
            print(t["bytes"])
            sys.exit(0)
sys.exit(f"tool '{tool}' is in prompt-budget.limits but no agent advertises it - "
         f"was it renamed, removed or deferred? Drop the line if so.")
PYLOOKUP
cat > "$TOOL_OVERSIZE" <<'PYOVER'
import json, os, sys
d = json.loads(sys.argv[1])
listed = set(filter(None, os.environ["LISTED"].split()))
threshold = int(os.environ["THRESHOLD"])
seen = {}
for agent in d["agents"]:
    for t in agent["tools"]:
        seen[t["name"]] = t["bytes"]
for name, size in sorted(
    ((n, b) for n, b in seen.items() if b > threshold and n not in listed),
    key=lambda kv: -kv[1],
):
    print(f"{name} {size}")
PYOVER

status=0
declare -a NEW_LINES=()

while IFS= read -r raw; do
  line="${raw%%#*}"
  line="$(echo "$line" | tr -d '[:space:]')"
  if [[ -z "$line" ]]; then
    NEW_LINES+=("$raw")
    continue
  fi

  IFS=: read -r first second third extra <<< "$line"
  if [[ -n "${extra:-}" || -z "$first" || -z "$second" || -z "$third" ]]; then
    echo "::error::invalid prompt-budget limit entry: '$line'" >&2
    exit 1
  fi

  if [[ "$first" == "tool" ]]; then
    tool_name="$second"
    max_tool="$third"
    if ! measured_bytes="$(TOOL="$tool_name" python3 "$TOOL_LOOKUP" "$measured")"; then
      status=1
      NEW_LINES+=("$raw")
      continue
    fi
    if (( VERBOSE )); then echo "tool $tool_name $measured_bytes/$max_tool"; fi
    if (( measured_bytes > max_tool )); then
      echo "::error::tool schema REGRESSED: '$tool_name' is $measured_bytes B, limit is" \
           "$max_tool. Every agent holding it pays this on every turn." >&2
      status=1
    elif (( max_tool - measured_bytes > SLACK )); then
      echo "::error::tool schema IMPROVED but was not ratcheted: '$tool_name' is" \
           "$measured_bytes B against a limit of $max_tool. Lower it in this PR." >&2
      status=1
    fi
    NEW_LINES+=("tool:$tool_name:$measured_bytes")
    continue
  fi

  agent="$first"
  max_prompt="$second"
  max_tools="$third"

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

# Any tool over the attention threshold must be recorded above. Checked after
# the loop so the message names every offender at once rather than failing on
# the first.
listed_tools="$(grep -oE '^tool:[^:]+' "$LIMITS" | cut -d: -f2 | sort -u || true)"
unlisted="$(LISTED="$listed_tools" THRESHOLD="$TOOL_ATTENTION_BYTES" \
  python3 "$TOOL_OVERSIZE" "$measured")"
while read -r name size; do
  [[ -z "$name" ]] && continue
  if (( WRITE )); then
    NEW_LINES+=("tool:$name:$size")
    echo "[prompt-budget] recording tool:$name:$size" >&2
  else
    echo "::error::'$name' is $size B, over the $TOOL_ATTENTION_BYTES B attention" \
         "threshold, and is not in $LIMITS. Add 'tool:$name:$size' with a comment" \
         "saying why it needs that much, or trim it." >&2
    status=1
  fi
done <<< "$unlisted"

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
