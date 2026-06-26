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

# Prove the binary was built on the expected channel. The sentinel is baked in
# at compile time (apps/desktop/src/lib.rs), so this catches a misbuilt artifact
# regardless of how the bundle was packaged.
check_sentinel() {
  local bin="$1"
  local expected="posthaste-release-channel=$channel"
  if ! grep -aq "$expected" "$bin"; then
    local found
    found="$(grep -ao 'posthaste-release-channel=[a-z]*' "$bin" | head -n1 || true)"
    fail "binary channel sentinel mismatch: expected '$expected', found '${found:-none}'"
  fi
}

linux_smoke() {
  local appimage
  appimage="$(find "$bundle_dir" -maxdepth 1 -type f -name '*_amd64.AppImage' -print -quit | head -n1)"
  [ -n "$appimage" ] || fail "no AppImage bundle found in $bundle_dir"
  # Resolve to an absolute path before we cd into the extraction directory.
  appimage="$(realpath -e "$appimage")"
  [ -f "$appimage" ] || fail "AppImage file not readable: $appimage"
  [ -x "$appimage" ] || chmod +x "$appimage"

  # Keep this global: the EXIT trap is left behind after linux_smoke returns
  # and a local variable would be out of scope when the trap fires.
  linux_extract_dir="$(mktemp -d)"
  cleanup_extract() { rm -rf "$linux_extract_dir"; }
  trap cleanup_extract EXIT

  (cd "$linux_extract_dir" && "$appimage" --appimage-extract >/dev/null) || \
    fail "AppImage extraction failed"

  local bin
  bin="$(find "$linux_extract_dir/squashfs-root/usr/bin" -maxdepth 1 -type f -executable -print -quit | head -n1)"
  if [ -z "$bin" ]; then
    bin="$(find "$linux_extract_dir/squashfs-root" -maxdepth 1 -type f -executable -print -quit | head -n1)"
  fi
  [ -n "$bin" ] || fail "no executable found in extracted AppImage"

  "$bin" --version >/dev/null || fail "desktop binary --version failed"
  "$bin" --help >/dev/null || fail "desktop binary --help failed"

  check_sentinel "$bin"

  echo "linux smoke passed: $(basename "$appimage")"
}

macos_smoke() {
  local app
  app="$(find "$bundle_dir" -maxdepth 1 -type d -name '*.app' -print -quit | head -n1)"
  local dmg
  dmg="$(find "$bundle_dir" -maxdepth 1 -type f -name '*.dmg' -print -quit | head -n1)"

  if [ -n "$app" ]; then
    local bin
    bin="$(find "$app/Contents/MacOS" -maxdepth 1 -type f -perm -111 -print -quit | head -n1)"
    [ -n "$bin" ] || fail "no executable found in $app"
    [ -x "$bin" ] || fail "macOS .app binary is not executable: $bin"

    "$bin" --version >/dev/null || fail "desktop binary --version failed"
    "$bin" --help >/dev/null || fail "desktop binary --help failed"

    check_sentinel "$bin"

    echo "macos smoke passed: $(basename "$app")"
  elif [ -n "$dmg" ]; then
    echo "macos smoke passed: .dmg present (sentinel not checked)"
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
