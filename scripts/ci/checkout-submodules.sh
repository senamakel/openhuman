#!/usr/bin/env bash
# Check out the submodules a CI build needs, fetching in parallel.
#
# All 16 top-level vendor/ submodules, plus only the nested submodules a Cargo
# build reads. The rest of the recursive tree (three GitHub wikis, twelve
# nested copies of tinybus that the root [patch] replaces with vendor/tinybus,
# and tinymemory's own tinycortex/tinyinference) is never compiled, so fetching
# it only costs checkout time. If a build starts to need another nested
# submodule, cargo fails with "failed to read .../Cargo.toml"; add it below.
#
#   bash scripts/ci/checkout-submodules.sh
set -euo pipefail
cd "$(dirname "$0")/../.."

# "<parent submodule> <nested path>" pairs a Cargo build reads.
NESTED=(
  "vendor/tinyagents vendor/tinytools"
  "vendor/tinyagents vendor/tinyinference"
)
JOBS="${SUBMODULE_JOBS:-8}"

git -c protocol.version=2 submodule update --init --jobs "${JOBS}"
for pair in "${NESTED[@]}"; do
  read -r parent nested <<<"${pair}"
  git -C "${parent}" -c protocol.version=2 submodule update --init --jobs "${JOBS}" -- "${nested}"
done
echo "[ci][submodules] checked out $(git submodule status | wc -l) top-level and ${#NESTED[@]} nested submodules"
