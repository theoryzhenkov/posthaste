#!/usr/bin/env bash
set -euo pipefail

# Resolve the release channel and prerelease status for a tag.
#
# Accepted patterns:
#   vX.Y.Z-nightly.N  -> channel=nightly,  prerelease=true
#   vX.Y.Z-rc.N       -> channel=stable,   prerelease=true
#   vX.Y.Z            -> channel=stable,   prerelease=false
#
# Manual dispatch can pass an explicit channel to override inference. Unknown
# tag shapes with no override are rejected.
#
# Emits name=value pairs (channel, semver version, prerelease).

tag="${1:?usage: resolve-channel.sh <tag> [channel]}"
explicit_channel="${2:-}"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ -n "$explicit_channel" ]; then
  channel="$explicit_channel"
  # If the user explicitly passed the channel, we still inspect the tag to
  # derive prerelease status: a plain tag is a release, anything with a suffix
  # is a prerelease.
  if [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    prerelease="false"
  else
    prerelease="true"
  fi
else
  channel=""
  prerelease=""
  if [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+-nightly\.[0-9]+$ ]]; then
    channel="nightly"
    prerelease="true"
  elif [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+-rc\.[0-9]+$ ]]; then
    channel="stable"
    prerelease="true"
  elif [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    channel="stable"
    prerelease="false"
  fi
  if [ -z "$channel" ]; then
    echo "error: release tag '$tag' does not match a known channel pattern; pass an explicit channel" >&2
    exit 1
  fi
fi

case "$channel" in
  nightly|stable) ;;
  *) echo "error: unknown channel '$channel' (expected: nightly|stable)" >&2; exit 1 ;;
esac

version="$("$script_dir/bundle-version-from-tag.sh" "$tag")"

cat <<EOF
POSTHASTE_RELEASE_CHANNEL=$channel
POSTHASTE_BUNDLE_VERSION=$version
POSTHASTE_PRERELEASE=$prerelease
EOF
