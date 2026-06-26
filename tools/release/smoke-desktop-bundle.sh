#!/usr/bin/env bash
set -euo pipefail

# Smoke-test a collected desktop bundle directory before it is uploaded.
#
# Usage:
#   smoke-desktop-bundle.sh <channel> <platform> <bundle-dir>
#
# Supported platforms: linux-x86_64, macos, windows-x86_64.
#
# The channel check is a direct self-report: the desktop binary, run with
# `--print-release-channel`, prints its compiled-in channel and exits before any
# GUI init. We compare that to the expected channel. This is toolchain- and
# packaging-independent (no byte-grepping a baked sentinel), so a mismatch shows
# the actual channel the binary was built for rather than an opaque failure.

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

# Find the largest executable file under a directory. The real Rust binary
# dwarfs any launcher/wrapper script, so size is a robust way to pick it without
# hard-coding the (channel-dependent) product name.
#
# Deliberately portable: GNU `find -printf`/`-perm -111` are not available on
# macOS's BSD find, so we list plain files and test each with POSIX `[ -x ]` and
# `wc -c`. Runs on both the Linux and macOS runners unchanged.
largest_executable() {
  local root="$1" best="" best_size=-1 f size
  while IFS= read -r f; do
    [ -x "$f" ] || continue
    size="$(wc -c < "$f" 2>/dev/null | tr -d '[:space:]')"
    [ -n "$size" ] || continue
    if [ "$size" -gt "$best_size" ]; then
      best_size="$size"
      best="$f"
    fi
  done < <(find "$root" -type f)
  printf '%s\n' "$best"
}

# Run the binary's self-report and assert it matches the expected channel.
check_channel() {
  local bin="$1"
  local reported
  reported="$("$bin" --print-release-channel 2>/dev/null || true)"
  [ -n "$reported" ] || fail "binary did not report a release channel: $bin"
  if [ "$reported" != "$channel" ]; then
    fail "binary channel mismatch: expected '$channel', binary reports '$reported'"
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
  bin="$(largest_executable "$linux_extract_dir/squashfs-root")"
  [ -n "$bin" ] || fail "no executable found in extracted AppImage"

  check_channel "$bin"

  echo "linux smoke passed: $(basename "$appimage") (channel $channel)"
}

macos_smoke() {
  # Prefer the updater tarball: it contains the .app and lets us run the binary
  # without mounting the DMG (hdiutil is flaky on CI runners).
  local app_tar
  app_tar="$(find "$bundle_dir" -maxdepth 1 -type f -name '*.app.tar.gz' -print -quit | head -n1)"
  local app
  app="$(find "$bundle_dir" -maxdepth 1 -type d -name '*.app' -print -quit | head -n1)"
  local dmg
  dmg="$(find "$bundle_dir" -maxdepth 1 -type f -name '*.dmg' -print -quit | head -n1)"

  local bin
  if [ -n "$app_tar" ]; then
    # Keep this global: the EXIT trap outlives this function, so a local would be
    # out of scope (and unbound under `set -u`) when the trap fires.
    macos_extract_dir="$(mktemp -d)"
    cleanup_macos() { rm -rf "$macos_extract_dir"; }
    trap cleanup_macos EXIT
    tar -xzf "$app_tar" -C "$macos_extract_dir" || fail "failed to extract $app_tar"
    bin="$(largest_executable "$macos_extract_dir")"
    [ -n "$bin" ] || fail "no executable found in $app_tar"
    check_channel "$bin"
    echo "macos smoke passed: $(basename "$app_tar") (channel $channel)"
  elif [ -n "$app" ]; then
    bin="$(largest_executable "$app/Contents/MacOS")"
    [ -n "$bin" ] || fail "no executable found in $app"
    check_channel "$bin"
    echo "macos smoke passed: $(basename "$app") (channel $channel)"
  elif [ -n "$dmg" ]; then
    # No runnable bundle present (e.g. updater artifacts disabled); fall back to
    # an existence check so the gate still verifies an artifact was produced.
    echo "macos smoke passed: .dmg present (channel not verified \u2014 no .app.tar.gz)"
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
