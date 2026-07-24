#!/usr/bin/env bash
# Validate the final, repacked AppImage bytes before release signing.
#
# This file is sourceable for fixture tests. When executed directly it extracts
# exactly one AppImage from a foreign working directory, validates the released
# sharun layout, and optionally performs a bounded Xvfb startup smoke.

set -euo pipefail

RUNTIME_VALIDATOR_SCRIPT_DIR="$(
  cd "$(dirname "${BASH_SOURCE[0]}")" && pwd
)"
# shellcheck source=strip-appimage-graphics-libs.sh
source "$RUNTIME_VALIDATOR_SCRIPT_DIR/strip-appimage-graphics-libs.sh"

runtime_validation_error() {
  echo "[appimage-runtime] ERROR: $*" >&2
  return 1
}

validate_extracted_appdir() {
  local appdir="$1"
  local lib_path="$appdir/shared/lib/lib.path"

  echo "[appimage-runtime] Validating extracted AppDir: $appdir"
  if [ -f "$lib_path" ]; then
    echo "[appimage-runtime] shared/lib/lib.path:"
    sed 's/^/[appimage-runtime]   /' "$lib_path"
  else
    echo "[appimage-runtime] shared/lib/lib.path: <missing>"
  fi

  if ! uses_sharun_launcher "$appdir"; then
    runtime_validation_error "released AppDir does not use a detectable sharun launcher"
    return 1
  fi

  local executable
  for executable in \
    "$appdir/AppRun" \
    "$appdir/sharun" \
    "$appdir/bin/OpenHuman" \
    "$appdir/shared/bin/OpenHuman"; do
    if ! is_executable_elf "$executable"; then
      runtime_validation_error "${executable#"$appdir"/} is not an executable ELF"
      return 1
    fi
  done

  local alias
  for alias in "$appdir/AppRun" "$appdir/bin/OpenHuman"; do
    if [ "$alias" -ef "$appdir/sharun" ] || cmp -s "$alias" "$appdir/sharun"; then
      continue
    fi
    runtime_validation_error "${alias#"$appdir"/} does not match sharun"
    return 1
  done

  validate_sharun_lib_path "$appdir" || return 1
  validate_appimage_required_libs "$appdir" || return 1

  local real_app="$appdir/shared/bin/OpenHuman"
  local needed
  if ! needed="$(patchelf --print-needed "$real_app")"; then
    runtime_validation_error \
      "could not read NEEDED entries from shared/bin/OpenHuman"
    return 1
  fi
  echo "[appimage-runtime] shared/bin/OpenHuman NEEDED entries:"
  if [ -n "$needed" ]; then
    printf '%s\n' "$needed" | sed 's/^/[appimage-runtime]   /'
  else
    echo "[appimage-runtime]   <none>"
  fi

  local expected_needed="${APPIMAGE_EXPECTED_NEEDED-libxdo.so.3 libcef.so}"
  local needed_name
  for needed_name in $expected_needed; do
    if ! printf '%s\n' "$needed" | grep -Fx "$needed_name" >/dev/null; then
      runtime_validation_error "shared/bin/OpenHuman is missing NEEDED entry '$needed_name'"
      return 1
    fi
  done

  local root elf rpath
  for root in \
    "$appdir/shared/bin" \
    "$appdir/shared/lib" \
    "$appdir/bin" \
    "$appdir/lib" \
    "$appdir/usr/bin" \
    "$appdir/usr/lib"; do
    [ -d "$root" ] || continue
    while IFS= read -r -d '' elf; do
      is_elf "$elf" || continue
      rpath="$(patchelf --print-rpath "$elf" 2>/dev/null || true)"
      case "$rpath" in
        *"/home/runner/"*|*"/__w/"*)
          runtime_validation_error \
            "${elf#"$appdir"/} retains forbidden build-runner RPATH '$rpath'"
          return 1
          ;;
      esac
    done < <(find "$root" -type f -print0)
  done
}

smoke_extracted_apprun() {
  local appdir="$1"
  local foreign_cwd="$2"
  local log_file="$3"

  command -v timeout >/dev/null 2>&1 \
    || { runtime_validation_error "timeout is required for the AppImage smoke"; return 1; }
  command -v xvfb-run >/dev/null 2>&1 \
    || { runtime_validation_error "xvfb-run is required for the AppImage smoke"; return 1; }

  echo "[appimage-runtime] Smoking AppRun from AppDir: $appdir"
  echo "[appimage-runtime] Smoke caller CWD: $foreign_cwd"

  local temp_root
  temp_root="$(dirname "$foreign_cwd")"
  local smoke_home="$temp_root/home"
  local smoke_config="$temp_root/config"
  local smoke_data="$temp_root/data"
  local smoke_cache="$temp_root/cache"
  mkdir -p \
    "$foreign_cwd" \
    "$smoke_home" \
    "$smoke_config" \
    "$smoke_data" \
    "$smoke_cache"

  local -a unset_args=()
  local secret_name
  for secret_name in \
    GITHUB_TOKEN \
    GH_TOKEN \
    TAURI_SIGNING_PRIVATE_KEY \
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD \
    SENTRY_AUTH_TOKEN; do
    unset_args+=(-u "$secret_name")
  done
  while IFS= read -r secret_name; do
    [ -n "$secret_name" ] && unset_args+=(-u "$secret_name")
  done < <(compgen -v OPENAI_ || true)

  local status
  if (
    cd "$foreign_cwd"
    timeout --signal=TERM --kill-after=5s 15s \
      xvfb-run -a --server-args="-screen 0 1280x960x24" \
      env "${unset_args[@]}" \
        HOME="$smoke_home" \
        XDG_CONFIG_HOME="$smoke_config" \
        XDG_DATA_HOME="$smoke_data" \
        XDG_CACHE_HOME="$smoke_cache" \
        OPENHUMAN_CEF_PREWARM=0 \
        OPENHUMAN_DISABLE_GPU=1 \
        "$appdir/AppRun"
  ) >"$log_file" 2>&1; then
    status=0
  else
    status=$?
  fi

  local forbidden=0
  if grep -Eiq \
    'anylinux\.so.*cannot be preloaded|cannot be preloaded.*anylinux\.so' \
    "$log_file"; then
    forbidden=1
  fi
  if grep -Eiq \
    'libxdo\.so\.3.*cannot open shared object file|cannot open shared object file.*libxdo\.so\.3' \
    "$log_file"; then
    forbidden=1
  fi
  if grep -Eiq \
    'libcef\.so.*cannot open shared object file|cannot open shared object file.*libcef\.so' \
    "$log_file"; then
    forbidden=1
  fi
  if grep -Eiq 'error while loading shared libraries' "$log_file"; then
    forbidden=1
  fi

  if [ "$forbidden" -ne 0 ] || [ "$status" -ne 124 ]; then
    echo "[appimage-runtime] AppImage startup smoke failed (status $status):" >&2
    sed 's/^/[appimage-runtime]   /' "$log_file" >&2
    return 1
  fi

  grep -Ei 'loader|loading|cef|startup|started|ready' "$log_file" \
    | sed 's/^/[appimage-runtime]   /' \
    || true
  echo "[appimage-runtime] Application remained alive for the 15-second startup window"
}

validate_final_appimage() (
  local image="$1"
  if ! image="$(realpath "$image")"; then
    runtime_validation_error "could not resolve AppImage path: $1"
    return 1
  fi
  if [ ! -f "$image" ] || [ ! -x "$image" ]; then
    runtime_validation_error "AppImage must be an executable regular file: $image"
    return 1
  fi
  command -v patchelf >/dev/null 2>&1 \
    || { runtime_validation_error "patchelf is required for final AppImage validation"; return 1; }

  local temp_root
  temp_root="$(mktemp -d)"
  trap 'rm -rf -- "$temp_root"' EXIT
  local foreign_cwd="$temp_root/foreign-cwd"
  local extraction_log="$temp_root/extraction.log"
  local smoke_log="$temp_root/smoke.log"
  mkdir -p "$foreign_cwd"

  if ! (
    cd "$foreign_cwd"
    "$image" --appimage-extract
  ) >"$extraction_log" 2>&1; then
    echo "[appimage-runtime] AppImage extraction failed:" >&2
    sed 's/^/[appimage-runtime]   /' "$extraction_log" >&2
    return 1
  fi

  local appdir="$foreign_cwd/squashfs-root"
  if [ ! -d "$appdir" ]; then
    runtime_validation_error "extraction did not create $appdir"
    return 1
  fi
  validate_extracted_appdir "$appdir" || return 1

  if [ "${APPIMAGE_RUNTIME_SMOKE:-0}" = "1" ]; then
    smoke_extracted_apprun "$appdir" "$foreign_cwd" "$smoke_log" || return 1
  else
    echo "[appimage-runtime] Static validation complete; executable smoke disabled for this architecture"
  fi
)

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  [ "$#" -eq 1 ] || {
    echo "Usage: $0 <final.AppImage>" >&2
    exit 2
  }
  APPIMAGE_EXPECTED_NEEDED="libxdo.so.3 libcef.so" \
    validate_final_appimage "$1"
fi
