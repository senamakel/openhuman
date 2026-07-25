#!/usr/bin/env bash
set -euo pipefail

target="${1:-x86_64-unknown-linux-gnu}"
forbidden='^(native-tls|openssl|openssl-sys) v'

tree="$(cargo tree --locked --target "$target" --prefix none)"
if matches="$(printf '%s\n' "$tree" | grep -E "$forbidden")"; then
  printf 'error: native TLS/OpenSSL dependencies found for %s:\n%s\n' \
    "$target" "$matches" >&2
  exit 1
fi

printf 'Linux TLS dependency policy passed for %s\n' "$target"
