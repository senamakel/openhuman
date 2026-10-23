#!/usr/bin/env bash
# Build TinySecurity and exercise the authenticated browser-dial host callbacks
# against that native module.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
security_dir="$repo_root/vendor/tinysecurity"
computer_dir="$repo_root/vendor/tinycomputer"

if [[ -n "${OPENHUMAN_TEST_SECURITY_MODULE:-}" ]]; then
  module_path="$OPENHUMAN_TEST_SECURITY_MODULE"
else
  case "$(uname -s)" in
    Darwin) module_path="$security_dir/target/debug/libtinysecurity_module.dylib" ;;
    Linux) module_path="$security_dir/target/debug/libtinysecurity_module.so" ;;
    MINGW* | MSYS* | CYGWIN*) module_path="$security_dir/target/debug/tinysecurity_module.dll" ;;
    *) echo "Unsupported TinySecurity browser egress fixture host" >&2; exit 1 ;;
  esac
fi

# TinySecurity's .cargo/config.toml selects polyval_force_soft, which is
# required by its GHASH zeroization assertion. Cargo must run from that repo.
(
  cd -- "$security_dir"
  bash "$repo_root/scripts/ci-cancel-aware.sh" cargo build -p tinysecurity-module
)

test -f "$module_path" || {
  echo "TinySecurity module artifact was not produced: $module_path" >&2
  exit 1
}

cd -- "$repo_root"
export OPENHUMAN_TEST_SECURITY_MODULE="$module_path"
bash scripts/ci-cancel-aware.sh cargo test -p openhuman --lib \
  --no-default-features --features modules,security-module,native-security-tests,legacy-import,storage-sqlite,secret-file \
  modules::browser_egress_host::tests::browser_egress_callbacks_use_tinysecurity_one_use_grants_and_leases \
  -- --exact

if [[ "$(uname -s)" == Linux ]]; then
  if [[ -n "${OPENHUMAN_TEST_TINYCOMPUTER_MODULE:-}" ]]; then
    computer_module="$OPENHUMAN_TEST_TINYCOMPUTER_MODULE"
  else
    computer_module="$computer_dir/target/debug/libtinycomputer.so"
  fi
  if [[ -z "${OPENHUMAN_TEST_TINYCOMPUTER_MODULE:-}" ]]; then
    (
      cd -- "$computer_dir"
      bash "$repo_root/scripts/ci-cancel-aware.sh" cargo build -p tinycomputer
    )
    strip --strip-debug "$computer_module"
  fi
  test -f "$computer_module" || {
    echo "TinyComputer module artifact was not produced: $computer_module" >&2
    exit 1
  }
  OPENHUMAN_TEST_TINYCOMPUTER_MODULE="$computer_module" \
    bash scripts/ci-cancel-aware.sh cargo test -p openhuman --lib \
      --no-default-features --features modules,security-module,native-security-tests,legacy-import,storage-sqlite,secret-file \
      modules::browser_egress_host::tests::loaded_tinycomputer_uses_tinysecurity_to_block_loopback_dials \
      -- --ignored --exact

  OPENHUMAN_TEST_TINYCOMPUTER_MODULE="$computer_module" \
    bash scripts/ci-cancel-aware.sh cargo test -p openhuman --lib \
      --no-default-features --features modules,security-module,native-security-tests,legacy-import,storage-sqlite,secret-file \
      modules::browser_egress_host::tests::loaded_tinycomputer_uses_tinysecurity_to_allow_one_public_socket \
      -- --ignored --exact
fi
