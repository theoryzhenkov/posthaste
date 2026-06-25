#!/usr/bin/env bash
set -euo pipefail

# Update a lightweight rolling tag to point at a release tag.
#
# The tag is force-pushed, so a channel's static GitHub Release asset URL always
# resolves to the latest release for that channel.
#
# Usage:
#   update-rolling-tag.sh <channel> <release-tag>

channel="${1:?usage: update-rolling-tag.sh <channel> <release-tag>}"
release_tag="${2:?missing release tag}"

case "$channel" in
  nightly|stable) ;;
  *) echo "error: unknown channel '$channel'" >&2; exit 1 ;;
esac

if [ -z "${GITHUB_TOKEN:-}" ]; then
  echo "error: GITHUB_TOKEN is not set; cannot update rolling tag" >&2
  exit 1
fi

repo="${GITHUB_REPOSITORY:-theoryzhenkov/posthaste}"

# Resolve the commit for the release tag so the rolling tag is exact.
commit="$(git rev-parse -q --verify "${release_tag}^{commit}")" \
  || fail "cannot resolve release tag '$release_tag'"

git tag -f "$channel" "$commit"
git push -f origin "refs/tags/$channel"

echo "updated rolling tag '$channel' -> $release_tag ($commit)" >&2
