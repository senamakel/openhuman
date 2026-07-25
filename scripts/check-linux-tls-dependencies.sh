#!/usr/bin/env bash
set -euo pipefail

target="${1:-x86_64-unknown-linux-gnu}"
forbidden='^(native-tls|openssl|openssl-sys|aws-lc-rs|aws-lc-sys) v'

tree="$(cargo tree --locked --target "$target" --prefix none)"
if matches="$(printf '%s\n' "$tree" | grep -E "$forbidden")"; then
  printf 'error: native TLS/OpenSSL dependencies found for %s:\n%s\n' \
    "$target" "$matches" >&2
  exit 1
fi

reqwest_versions="$(
  printf '%s\n' "$tree" |
    sed -nE 's/^reqwest v([^ ]+).*/\1/p' |
    sort -u
)"
if [[ "$(printf '%s\n' "$reqwest_versions" | sed '/^$/d' | wc -l)" -ne 1 ]]; then
  printf 'error: expected exactly one reqwest version for %s, found:\n%s\n' \
    "$target" "$reqwest_versions" >&2
  exit 1
fi

printf 'Linux TLS dependency policy passed for %s\n' "$target"
