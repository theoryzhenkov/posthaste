#!/usr/bin/env bash
set -euo pipefail

# Generate the Tauri updater manifest (`latest.json`) from the release assets.
#
# The Tauri updater plugin fetches this manifest from the GitHub Release and,
# for the running platform, downloads the referenced bundle and verifies it
# against the embedded public key using the `.sig` produced at build time.
#
# Usage:
#   generate-updater-manifest.sh <assets-dir> <release-tag> [repo-slug]
#
# Reads the updater bundle + `.sig` for each platform from <assets-dir> and
# writes <assets-dir>/latest.json. Platforms without a present bundle are
# skipped, so a partial matrix still produces a valid (smaller) manifest.

assets_dir="${1:?usage: generate-updater-manifest.sh <assets-dir> <release-tag> [repo-slug]}"
release_tag="${2:?missing release tag}"
repo_slug="${3:-theoryzhenkov/posthaste}"

# Derive the bundle version the same way the desktop build does: a
# `vX.Y.Z-dogfood.N` tag maps to `X.Y.N`; a plain `vX.Y.Z` tag is used as-is.
tag="${release_tag#v}"
if [[ "$tag" =~ ^([0-9]+)\.([0-9]+)\.[0-9]+-dogfood\.([0-9]+)$ ]]; then
  version="${BASH_REMATCH[1]}.${BASH_REMATCH[2]}.${BASH_REMATCH[3]}"
else
  version="$tag"
fi

download_base="https://github.com/${repo_slug}/releases/download/${release_tag}"

# Map Tauri updater platform keys to the bundle filename glob for that target.
# macOS runners are arm64, so the only macOS updater target is darwin-aarch64.
declare -A platform_glob=(
  [linux-x86_64]='*_amd64.AppImage'
  [windows-x86_64]='*_x64-setup.exe'
  [darwin-aarch64]='*.app.tar.gz'
)

platforms_json="{}"
for platform in "${!platform_glob[@]}"; do
  glob="${platform_glob[$platform]}"
  # Prefer the primary bundle; never the DevTools variant if one exists.
  bundle="$(find "$assets_dir" -maxdepth 1 -type f -name "$glob" ! -name '*DevTools*' -print 2>/dev/null | sort | head -n1)"
  if [ -z "$bundle" ]; then
    echo "no bundle for $platform (glob $glob); skipping" >&2
    continue
  fi
  sig_file="${bundle}.sig"
  if [ ! -f "$sig_file" ]; then
    echo "missing signature for $bundle ($sig_file); skipping $platform" >&2
    continue
  fi
  signature="$(cat "$sig_file")"
  url="${download_base}/$(basename "$bundle")"
  platforms_json="$(jq \
    --arg platform "$platform" \
    --arg signature "$signature" \
    --arg url "$url" \
    '. + {($platform): {signature: $signature, url: $url}}' \
    <<<"$platforms_json")"
done

if [ "$(jq 'length' <<<"$platforms_json")" -eq 0 ]; then
  echo "no updater bundles found in $assets_dir; refusing to write empty manifest" >&2
  exit 1
fi

pub_date="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
jq -n \
  --arg version "$version" \
  --arg pub_date "$pub_date" \
  --argjson platforms "$platforms_json" \
  '{version: $version, notes: ("Posthaste " + $version), pub_date: $pub_date, platforms: $platforms}' \
  >"$assets_dir/latest.json"

echo "wrote $assets_dir/latest.json for version $version" >&2
cat "$assets_dir/latest.json" >&2
