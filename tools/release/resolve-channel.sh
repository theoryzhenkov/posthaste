#!/usr/bin/env bash
set -euo pipefail

# Resolve the release channel for a tag.
#
# Channel is the single first-class output; per-channel policy is read from
# channel-policy.sh, not threaded as booleans. For tag-push the channel is
# inferred from the tag; for manual dispatch the caller passes an explicit
# channel so a typoed tag cannot silently flip the channel.
#
# Usage:
#   resolve-channel.sh <tag> [channel]
#
# Emits name=value pairs (channel + semver version). Unknown tag shapes with no
# explicit channel are rejected.

tag="${1:?usage: resolve-channel.sh <tag> [channel]}"
explicit_channel="${2:-}"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ -n "$explicit_channel" ]; then
  channel="$explicit_channel"
else
  channel=""
  if [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+-dogfood\.[0-9]+$ ]]; then
    channel="nightly"
  elif [[ "$tag" =~ -nightly\. ]]; then
    channel="nightly"
  elif [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-(beta|rc)\.[0-9]+)?$ ]]; then
    channel="stable"
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
EOF
