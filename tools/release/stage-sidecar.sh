#!/usr/bin/env bash
# Stage the posthastectl Tauri sidecar for a desktop bundle.
#
# `bundle.externalBin` in apps/client/desktop/tauri.conf.json requires
# apps/client/desktop/binaries/posthastectl-<host-triple>[.exe] to exist before
# `tauri build` — Tauri validates it at bundle time (`tauri dev` does not), so a
# missing sidecar fails the bundle, not the compile.
#
# ONE copy, called from three places so they cannot drift:
#   - the release build          (.github/workflows/release.yml, build-desktop)
#   - the CI packaging gate      (.github/workflows/ci.yml, desktop-package)
#   - `just desktop stage-sidecar` for local bundle testing
# It previously lived inline in release.yml with a near-copy in the justfile,
# which meant the release path was exercised only when a tag was cut.
#
# Run from the repo root. Requires bun with apps/tools dependencies installed
# (`bun install`) and rustc, which supplies the host triple.
#
# Idempotent: skips if the sidecar is already staged — delete the file to
# refresh it after changing the CLI. The skip is the justfile recipe's local
# convenience and cannot make a release ship a stale CLI: workflow runners
# check out clean, and no cache covers apps/client/desktop/binaries/ (rust-cache
# takes target/ and ~/.cargo, the bun cache takes ~/.bun/install/cache).
set -euo pipefail

triple="$(rustc -vV | sed -n 's/^host: //p')"
ext=""
case "$triple" in *windows*) ext=".exe" ;; esac
dest="apps/client/desktop/binaries/posthastectl-${triple}${ext}"

mkdir -p apps/client/desktop/binaries
if [ -f "$dest" ]; then
  echo "sidecar already staged: $dest"
  exit 0
fi

# build-cli.ts prints the final artifact path (including .exe on Windows) as its
# last stdout line so callers can capture it.
artifact="$(bun run apps/tools/scripts/build-cli.ts | tail -1)"
test -n "$artifact" && test -f "$artifact" || {
  echo "posthastectl build failed" >&2
  exit 1
}
cp "$artifact" "$dest"
echo "staged sidecar: $dest"
