#!/usr/bin/env bash
set -euo pipefail

# Smoke-test a collected desktop bundle directory before it is uploaded.
#
# Usage:
#   smoke-desktop-bundle.sh <channel> <platform> <bundle-dir>
#
# Supported platforms: linux-x86_64, macos, windows-x86_64.
# "channel" is nightly or stable and controls strictness.

channel="${1:?usage: smoke-desktop-bundle.sh <channel> <platform> <bundle-dir>}"
platform="${2:?missing platform}"
bundle_dir="${3:?missing bundle dir}"

if [ ! -d "$bundle_dir" ]; then
  echo "error: bundle directory does not exist: $bundle_dir" >&2
  exit 1
fi

fail() {
  echo "error: $*" >&2
  exit 1
}

linux_smoke() {
  local appimage
  appimage="$(find "$bundle_dir" -maxdepth 1 -type f -name '*_amd64.AppImage' -print -quit | head -n1)"
  [ -n "$appimage" ] || fail "no AppImage bundle found in $bundle_dir"
  [ -x "$appimage" ] || chmod +x "$appimage"

  local extract_dir
  extract_dir="$(mktemp -d)"
  cleanup_extract() { rm -rf "$extract_dir"; }
  trap cleanup_extract EXIT

  (cd "$extract_dir" && "$appimage" --appimage-extract >/dev/null) || \
    fail "AppImage extraction failed"

  local bin
  bin="$(find "$extract_dir/squashfs-root/usr/bin" -maxdepth 1 -type f -executable -print -quit | head -n1)"
  if [ -z "$bin" ]; then
    bin="$(find "$extract_dir/squashfs-root" -maxdepth 1 -type f -executable -print -quit | head -n1)"
  fi
  [ -n "$bin" ] || fail "no executable found in extracted AppImage"

  "$bin" --version >/dev/null || fail "desktop binary --version failed"
  "$bin" --help >/dev/null || fail "desktop binary --help failed"

  if [ "$channel" = "stable" ]; then
    # Stable bundles must not carry dev-server/devtools endpoint strings.
    if grep -Rq "127\.0\.0\.1:5173" "$extract_dir/squashfs-root" 2>/dev/null; then
      fail "stable bundle contains a dev-server endpoint string"
    fi
  fi

  echo "linux smoke passed: $(basename "$appimage")"
}

macos_smoke() {
  local app
  app="$(find "$bundle_dir" -maxdepth 1 -type d -name '*.app' -print -quit | head -n1)"
  local dmg
  dmg="$(find "$bundle_dir" -maxdepth 1 -type f -name '*.dmg' -print -quit | head -n1)"

  if [ -n "$app" ]; then
    [ -x "$app/Contents/MacOS/"* ] || fail "macOS .app binary is not executable"
    echo "macos smoke passed: .app structure ok"
  elif [ -n "$dmg" ]; then
    echo "macos smoke passed: .dmg present"
  else
    fail "no macOS bundle found in $bundle_dir"
  fi
}

windows_smoke() {
  local installer
  installer="$(find "$bundle_dir" -maxdepth 1 -type f -name '*_x64-setup.exe' -print -quit | head -n1)"
  [ -n "$installer" ] || fail "no Windows installer found in $bundle_dir"
  # SHA-256 the installer as a minimal integrity check; `file` may not be on
  # Windows Git Bash, so we avoid it.
  sha256sum "$installer" >/dev/null || fail "Windows installer is not readable"
  echo "windows smoke passed: $(basename "$installer")"
}

case "$platform" in
  linux-x86_64)
    linux_smoke
 ;;
  macos)
    macos_smoke
    ;;
  windows-x86_64)
    windows_smoke
    ;;
  *)
    fail "unknown platform '$platform'"
    ;;
esac
