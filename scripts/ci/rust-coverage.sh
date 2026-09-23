#!/usr/bin/env bash
# PR CI Rust coverage lane.
#
# Every Rust-core change runs the complete product-feature test suite. Path
# filtering decides whether this high-level area runs; it never narrows the
# suite to changed files or source modules.
set -euo pipefail

OUT="${OUT:-lcov-core.info}"
PRODUCT_FEATURES="$(bash scripts/ci/product-features.sh)"

log() { echo "[ci][rust-cov] $*"; }

# cargo-llvm-cov owns RUSTFLAGS while it instruments crates. Keeping the
# job-level linker flag here can suppress instrumentation and produce no data.
unset RUSTFLAGS

llvm_cov() {
  case "${1:-}" in
    clean | report)
      bash scripts/ci-cancel-aware.sh cargo llvm-cov "$@"
      ;;
    *)
      bash scripts/ci-cancel-aware.sh cargo llvm-cov \
        --features "${PRODUCT_FEATURES}" "$@"
      ;;
  esac
}

llvm_cov_package() {
  bash scripts/ci-cancel-aware.sh cargo llvm-cov "$@"
}

llvm_cov_embed() {
  bash scripts/ci-cancel-aware.sh cargo llvm-cov \
    --features "${PRODUCT_FEATURES}" "$@"
}

integration_test_targets() {
  find tests -maxdepth 1 -type f -name '*.rs' -print |
    sed -e 's#^tests/##' -e 's#\.rs$##' |
    sort
}

raw_coverage_modules() {
  find tests/raw_coverage -maxdepth 1 -type f -name '*.rs' -print |
    sed -e 's#^tests/raw_coverage/##' -e 's#\.rs$##' |
    sort
}

# Print each explicitly declared integration-test target and its required
# features as "<name><TAB><comma-separated gates>".
test_target_required_features() {
  awk '
    /^\[\[test\]\]/ { if (name != "" && req != "") print name "\t" req; name=""; req=""; inblk=1; next }
    /^\[/              { if (name != "" && req != "") print name "\t" req; name=""; req=""; inblk=0 }
    inblk && /^name[ \t]*=/ {
      line=$0; sub(/^name[ \t]*=[ \t]*"/, "", line); sub(/".*$/, "", line); name=line; next
    }
    inblk && /^required-features[ \t]*=/ {
      line=$0
      sub(/^required-features[ \t]*=[ \t]*\[/, "", line); sub(/\].*$/, "", line)
      gsub(/[" ]/, "", line); req=line; next
    }
    END { if (name != "" && req != "") print name "\t" req }
  ' crates/openhuman-cli/Cargo.toml
}

TEST_TARGET_REQS="$(test_target_required_features)"

target_features_satisfied() {
  local target="$1" req feature
  req="$(printf '%s\n' "${TEST_TARGET_REQS}" | awk -F'\t' -v t="${target}" '$1 == t { print $2 }')"
  [ -n "${req}" ] || return 0
  for feature in $(printf '%s' "${req}" | tr ',' ' '); do
    case ",${PRODUCT_FEATURES}," in
      *",${feature},"*) ;;
      *) return 1 ;;
    esac
  done
}

# Every suite runs even when an earlier one fails, so one red crate never
# hides the results of the crates and targets after it. Failures are
# collected and reported together at the end, after the lcov report is still
# merged from whatever did run. A cancellation (130/143 from
# scripts/ci-cancel-aware.sh) stops the run at once.
FAILED_SUITES=()
suite() {
  local name="$1" rc=0
  shift
  "$@" || rc=$?
  case "${rc}" in
    0) ;;
    130 | 143)
      log "cancelled during ${name} (exit ${rc})"
      exit "${rc}"
      ;;
    *)
      FAILED_SUITES+=("${name} (exit ${rc})")
      log "FAILED: ${name} (exit ${rc}); continuing with the remaining suites"
      ;;
  esac
}

run_integration_target() {
  local target="$1"
  if ! target_features_satisfied "${target}"; then
    log "skipping ${target}: required features are not in the product set"
    return 0
  fi

  if [ "${target}" = "raw_coverage_all" ]; then
    # These modules mutate process-global state. Keep one process per module
    # while paying the link cost for the aggregate target only once.
    while IFS= read -r module; do
      [ -n "${module}" ] || continue
      log "running raw coverage module: ${module}"
      suite "${target}::${module}" llvm_cov --no-report --no-fail-fast -p openhuman-cli \
        --test "${target}" -- "${module}::" --test-threads=1
    done < <(raw_coverage_modules)
  elif [ "${target}" = "json_rpc_e2e" ]; then
    # JSON-RPC tests share runtime/config globals and must remain serial.
    suite "${target}" llvm_cov --no-report --no-fail-fast -p openhuman-cli \
      --test "${target}" -- --test-threads=1
  else
    suite "${target}" llvm_cov --no-report --no-fail-fast -p openhuman-cli --test "${target}"
  fi
}

log "running complete instrumented Rust suite (runner: ${OH_COV_RUNNER:-cargo})"
llvm_cov clean --workspace

if [ "${OH_COV_RUNNER:-cargo}" = "nextest" ]; then
  # cargo-nextest runs every test in its own process, in parallel. Process
  # globals (the event bus, registries, env vars, one-shot OnceLocks) are then
  # per test, so the serial runner and the isolated reaper run below are not
  # needed: every test is isolated. Settings: .config/nextest.toml [profile.ci].
  suite "openhuman lib+bins (nextest)" llvm_cov nextest --profile ci --no-report \
    --no-fail-fast -p openhuman --lib --bins
else
  # Keep the aggregate unit-test process aligned with the canonical Rust runner.
  # The isolated reaper test installs one-shot process globals, so it cannot run
  # in the same process as the rest of the library tests.
  suite "openhuman lib+bins" llvm_cov --no-report --no-fail-fast -p openhuman --lib --bins -- \
    --test-threads=1 \
    --skip a_build_only_runtime_is_swept_before_it_can_be_invoked

  log "running isolated build-only reaper test"
  suite "openhuman reaper (isolated)" llvm_cov --no-report --no-fail-fast -p openhuman --lib -- \
    openhuman::agent::tinyagents::reaper::tests::a_build_only_runtime_is_swept_before_it_can_be_invoked \
    --exact --test-threads=1
fi

# Run every root-workspace Rust support crate rather than only crates named by
# changed paths. Product features are forwarded to the embedding facade; the
# remaining crates do not expose that feature vocabulary.
suite "openhuman-embed" llvm_cov_embed --no-report --no-fail-fast -p openhuman-embed --all-targets
suite "openhuman-rpc" llvm_cov_package --no-report --no-fail-fast -p openhuman-rpc --all-targets
suite "openhuman-tinyhumans" llvm_cov_embed --no-report --no-fail-fast -p openhuman-tinyhumans --all-targets
# The terminal frontend builds the core a third time (default features, not the
# product set), so CI Fast leaves it to pushes to main (OH_COV_TUI=0), where
# CI Lite runs this script with it on.
if [ "${OH_COV_TUI:-1}" = "1" ]; then
  suite "openhuman-tui" llvm_cov_package --no-report --no-fail-fast -p openhuman-tui --all-targets
else
  log "skipping openhuman-tui (OH_COV_TUI=${OH_COV_TUI}); pushes to main run it"
fi

if [ "${OH_COV_RUNNER:-cargo}" = "nextest" ]; then
  # Every integration target in one parallel run, one process per test: the
  # raw_coverage modules and the JSON-RPC tests are then isolated from each
  # other without the per-module and serial invocations below. Cargo skips a
  # target whose required-features are not in the product set; `kind(test)`
  # keeps the run to the integration targets, as the loop below runs them.
  suite "openhuman-cli integration tests (nextest)" llvm_cov nextest --profile ci \
    --no-report --no-fail-fast -p openhuman-cli --tests -E 'kind(test)'
else
  while IFS= read -r target; do
    [ -n "${target}" ] || continue
    log "running integration target: ${target}"
    run_integration_target "${target}"
  done < <(integration_test_targets)
fi

# Doctests are not collected by cargo-llvm-cov, but they are still part of the
# complete Rust test suite and must run whenever the Rust-core area changes.
# They compile their own uninstrumented core, so a caller that runs them in a
# parallel lane instead (CI Fast on the EX63) sets OH_COV_DOCTESTS=0.
if [ "${OH_COV_DOCTESTS:-1}" = "1" ]; then
  suite "openhuman doctests" bash scripts/ci-cancel-aware.sh cargo test -p openhuman \
    --doc --features "${PRODUCT_FEATURES}"
else
  log "skipping doctests (OH_COV_DOCTESTS=${OH_COV_DOCTESTS}); the caller runs them"
fi

log "merging coverage into ${OUT}"
suite "lcov report" llvm_cov report --lcov --output-path "${OUT}"

# A full product build must produce records for every eligible source file,
# except in the crates whose suites this run skipped (OH_COV_TUI=0).
presence_skip=""
[ "${OH_COV_TUI:-1}" = "1" ] || presence_skip="crates/openhuman-tui/src/"
[ -z "${presence_skip}" ] || log "coverage presence: not checking ${presence_skip} (suite skipped)"
suite "coverage presence" env COVERAGE_PRESENCE_SKIP_PREFIXES="${presence_skip}" \
  bash scripts/ci/assert-coverage-presence.sh "${OUT}" --all

if [ "${#FAILED_SUITES[@]}" -gt 0 ]; then
  log "${#FAILED_SUITES[@]} suite(s) failed:"
  printf '[ci][rust-cov]   - %s\n' "${FAILED_SUITES[@]}"
  exit 1
fi
log "all suites passed"
