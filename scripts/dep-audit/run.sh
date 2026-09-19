#!/usr/bin/env bash
# Dependency audit across OpenHuman and every Cargo submodule under vendor/.
#
# Runs `tinyanalyzer` (https://github.com/tinyhumansai/tinyanalyzer) once per
# Cargo root — the root workspace, the out-of-workspace `crates/openhuman-app`
# host, and every recursive git submodule that has a Cargo.toml — then folds the
# per-target JSON reports into a single Markdown report listing:
#
#   * declared dependencies no source file names   (candidates to delete)
#   * crates resolved at more than one version      (candidates to unify)
#   * the direct dependencies with the largest exclusive transitive footprint
#   * direct dependencies whose resolved version drifts between repositories
#
# Usage:
#   scripts/dep-audit/run.sh [--out <dir>] [--top <n>] [--targets <regex>]
#                            [--snapshot] [--keep-nested] [--no-report] [--verbose]
#
#   --out <dir>       Where JSON reports, summary.json and REPORT.md land.
#                     Default: target/dep-audit (gitignored).
#   --snapshot        Also copy REPORT.md to docs/dep-audit/<YYYY-MM-DD>.md so
#                     the run is committed and the next one can be diffed
#                     against it.
#   --top <n>         How many heavy direct dependencies to list per target (default 15).
#   --targets <re>    Only analyze targets whose name matches this regex
#                     (e.g. --targets '^(root|tinyagents)$').
#   --keep-nested     Also analyze nested submodule checkouts that are pinned at
#                     the same commit as a top-level one (skipped by default —
#                     they are the same code and produce the same report).
#   --no-report       Only write the JSON reports; skip REPORT.md.
#   --verbose         Echo each tinyanalyzer invocation.
#
# Requires: tinyanalyzer on PATH, node >= 20, git, jq (optional, for --verbose sizes).
# See scripts/dep-audit/README.md for how to read the report and what to do next.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

OUT_DIR="$ROOT/target/dep-audit"
TOP=15
TARGET_RE=""
KEEP_NESTED=0
SNAPSHOT=0
WRITE_REPORT=1
VERBOSE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --out) OUT_DIR="$2"; shift 2 ;;
    --top) TOP="$2"; shift 2 ;;
    --targets) TARGET_RE="$2"; shift 2 ;;
    --snapshot) SNAPSHOT=1; shift ;;
    --keep-nested) KEEP_NESTED=1; shift ;;
    --no-report) WRITE_REPORT=0; shift ;;
    --verbose) VERBOSE=1; shift ;;
    -h|--help) sed -n '2,34p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    --) shift ;;
    *) echo "dep-audit: unknown argument: $1" >&2; exit 2 ;;
  esac
done

if ! command -v tinyanalyzer >/dev/null 2>&1; then
  cat >&2 <<'MSG'
dep-audit: `tinyanalyzer` is not on PATH.

Install it with:
  curl --proto '=https' --tlsv1.2 -LsSf \
    https://raw.githubusercontent.com/tinyhumansai/tinyanalyzer/main/install.sh | sh
or, from a checkout of tinyhumansai/tinyanalyzer:
  cargo install --path crates/tinyanalyzer
MSG
  exit 1
fi

CONFIG="$ROOT/scripts/dep-audit/tinyanalyzer.toml"
mkdir -p "$OUT_DIR"
case "$OUT_DIR" in /*) ;; *) OUT_DIR="$ROOT/$OUT_DIR" ;; esac

# ---------------------------------------------------------------------------
# Target discovery.
#
# Each line: <name>\t<relative path>\t<commit>\t<remote url>
# `root` is the OpenHuman workspace itself; `openhuman-app` is the Tauri host
# crate, which `Cargo.toml` excludes from the workspace and so has its own
# lockfile and dependency graph. Every recursive submodule with a Cargo.toml is
# a target named after its directory. Nested copies (vendor/x/vendor/tinybus)
# are dropped when a top-level checkout is pinned at the same commit.
# ---------------------------------------------------------------------------
TARGETS_TSV="$OUT_DIR/targets.tsv"
: > "$TARGETS_TSV"

root_sha="$(git rev-parse HEAD)"
root_remote="$(git remote get-url upstream 2>/dev/null || git remote get-url origin 2>/dev/null || echo '')"
printf 'root\t.\t%s\t%s\n' "$root_sha" "$root_remote" >> "$TARGETS_TSV"
if [[ -f crates/openhuman-app/Cargo.toml ]]; then
  printf 'openhuman-app\tcrates/openhuman-app\t%s\t%s\n' "$root_sha" "$root_remote" >> "$TARGETS_TSV"
fi

declare -A seen_by_repo_sha=()
skipped_nested=()
while read -r sha path _; do
  prefix="${sha:0:1}"
  sha="${sha#[-+U]}"
  if [[ "$prefix" == "-" ]]; then
    echo "dep-audit: submodule not initialized: $path (run: git submodule update --init --recursive vendor/)" >&2
    exit 1
  fi
  [[ -f "$path/Cargo.toml" ]] || continue
  name="$(basename "$path")"
  remote="$(git -C "$path" remote get-url origin 2>/dev/null || echo '')"
  key="$name@$sha"
  depth="$(awk -F/ '{print NF}' <<<"$path")"
  if [[ -n "${seen_by_repo_sha[$key]:-}" && $KEEP_NESTED -eq 0 ]]; then
    skipped_nested+=("$path (same as ${seen_by_repo_sha[$key]})")
    continue
  fi
  # Prefer the shallowest checkout as the canonical name; deeper copies pinned
  # at a *different* commit get a path-derived suffix so both show up.
  if [[ -n "${seen_by_repo_sha[$name]:-}" ]]; then
    name="$name@$(sed 's#/#_#g' <<<"$path")"
  fi
  seen_by_repo_sha[$key]="$path"
  seen_by_repo_sha[$name]="$path"
  printf '%s\t%s\t%s\t%s\n' "$name" "$path" "$sha" "$remote" >> "$TARGETS_TSV"
done < <(git submodule status --recursive | sort -k2,2 | awk '{ print $1, $2 }' | awk '{ n=split($2,p,"/"); print n, $0 }' | sort -n | cut -d' ' -f2-)

# ---------------------------------------------------------------------------
# Analysis.
# ---------------------------------------------------------------------------
analyzed=0
failed=()
rewritten_locks=()
while IFS=$'\t' read -r name path sha remote; do
  if [[ -n "$TARGET_RE" ]] && ! [[ "$name" =~ $TARGET_RE ]]; then
    continue
  fi
  json="$OUT_DIR/$name.json"
  log="$OUT_DIR/$name.log"
  [[ $VERBOSE -eq 1 ]] && echo "dep-audit: tinyanalyzer $path -> $json"
  # tinyanalyzer runs `cargo metadata`, which silently rewrites a Cargo.lock
  # that is stale relative to its manifest (the app crate's lockfile is the
  # usual victim). An audit must not leave edits behind, so the lockfile is
  # snapshotted and put back if the run changed it.
  lock="$path/Cargo.lock"
  lock_backup=""
  if [[ -f "$lock" ]]; then
    lock_backup="$OUT_DIR/.lock-backup/$name.Cargo.lock"
    mkdir -p "$(dirname "$lock_backup")"
    cp "$lock" "$lock_backup"
  fi
  # --no-dead-code and --hide-tests keep the run cheap; the dependency graph is
  # what we are after and it does not depend on either.
  if tinyanalyzer "$path" --config "$CONFIG" --output json --no-dead-code --hide-tests \
      --write "$json" >"$log" 2>&1; then
    analyzed=$((analyzed + 1))
    printf 'dep-audit: %-28s ok\n' "$name"
  else
    failed+=("$name")
    printf 'dep-audit: %-28s FAILED (see %s)\n' "$name" "$log" >&2
    rm -f "$json"
  fi
  if [[ -n "$lock_backup" ]] && ! cmp -s "$lock" "$lock_backup"; then
    cp "$lock_backup" "$lock"
    rewritten_locks+=("$lock")
  fi
done < "$TARGETS_TSV"

echo "dep-audit: analyzed $analyzed target(s) into $OUT_DIR"
if ((${#skipped_nested[@]})); then
  echo "dep-audit: skipped ${#skipped_nested[@]} nested checkout(s) pinned at an already-analyzed commit (--keep-nested to include):"
  printf '  %s\n' "${skipped_nested[@]}"
fi
if ((${#failed[@]})); then
  echo "dep-audit: ${#failed[@]} target(s) failed: ${failed[*]}" >&2
fi
if ((${#rewritten_locks[@]})); then
  echo "dep-audit: cargo metadata re-resolved ${#rewritten_locks[@]} lockfile(s); restored them. Each is stale" \
       "relative to its manifest — refresh it deliberately (cargo update / generate-lockfile) if that is wanted:"
  printf '  %s\n' "${rewritten_locks[@]}"
fi

if [[ $WRITE_REPORT -eq 1 ]]; then
  node "$ROOT/scripts/dep-audit/report.mjs" --reports "$OUT_DIR" --top "$TOP" \
    --tinyanalyzer-version "$(tinyanalyzer --version)" \
    --out "$OUT_DIR/REPORT.md" --json "$OUT_DIR/summary.json"
  echo "dep-audit: report written to $OUT_DIR/REPORT.md (machine-readable: summary.json)"
  if [[ $SNAPSHOT -eq 1 ]]; then
    snap="$ROOT/docs/dep-audit/$(date -u +%Y-%m-%d).md"
    mkdir -p "$(dirname "$snap")"
    cp "$OUT_DIR/REPORT.md" "$snap"
    echo "dep-audit: snapshot saved to ${snap#"$ROOT"/}"
  fi
fi

((${#failed[@]} == 0))
