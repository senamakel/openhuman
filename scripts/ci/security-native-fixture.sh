#!/usr/bin/env bash
# Build the real native module for the explicitly invoked adapter fixture test.
# Its test-only library digest never becomes a product release pin.
set -euo pipefail

bash scripts/ci-cancel-aware.sh cargo build --release \
  --manifest-path vendor/tinysecurity/Cargo.toml -p tinysecurity-module

case "$(uname -s)" in
  Darwin) security_library="libtinysecurity_module.dylib" ;;
  Linux) security_library="libtinysecurity_module.so" ;;
  MINGW* | MSYS* | CYGWIN*) security_library="tinysecurity_module.dll" ;;
  *) echo "Unsupported native security fixture host" >&2; exit 1 ;;
esac

export OPENHUMAN_TEST_SECURITY_MODULE="$PWD/vendor/tinysecurity/target/release/$security_library"
test -f "$OPENHUMAN_TEST_SECURITY_MODULE"

# CI container checkouts may be owned by the host runner uid. Copy the test
# library under the current user's owned ancestors for strict native admission.
# Compiled build output stays in the vendored checkout's target directory.
export OPENHUMAN_TEST_SECURITY_FIXTURE_DIR="$HOME/.cache/openhuman-security-fixtures"
mkdir -p "$OPENHUMAN_TEST_SECURITY_FIXTURE_DIR"

bash scripts/ci-cancel-aware.sh cargo test -p openhuman --lib \
  --no-default-features --features inference,web3,modules,security-module \
  modules::security -- --include-ignored --nocapture
