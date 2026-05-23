#!/usr/bin/env bash
# Dispatcher for `pnpm rabbit <cmd>`.
# Commands: run (default) | list

set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
  cat <<EOF
Usage: pnpm rabbit [command] [args]

Commands:
  run           Scan open PRs, retrigger CodeRabbit on any whose rate-limit
                window has elapsed. Default command.
                Flags:
                  --max N        Cap retriggers this run (default: 5).
                  --dry-run      Print what would be done; post nothing.
                  --pr <num>     Only consider this PR.
                  --grace <sec>  Extra seconds past CR's stated wait before
                                 retriggering (default: 30).
  list          Print rate-limit status for each open PR; post nothing.

Env:
  RABBIT_REPO=owner/name        Override target repo (default: upstream remote).

CodeRabbit Pro reviews 5 PRs/hr — keep --max in line with your plan.
EOF
}

cmd="${1:-run}"
case "$cmd" in
  -h|--help) usage; exit 0 ;;
  run|list) shift || true ;;
  *)
    # Treat unknown first arg as flags to `run`.
    cmd="run"
    ;;
esac

exec node "$here/cli.mjs" "$cmd" "$@"
