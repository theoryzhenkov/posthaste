#!/usr/bin/env bash
set -euo pipefail

# Move a lightweight rolling tag to point at a release tag.
#
# This only moves the git tag; it does NOT attach release assets. The Tauri
# updater's channel URL (`releases/download/<channel>/<manifest>`) resolves via
# a GitHub *release* object for the rolling tag, which is created separately by
# the `Publish updater manifest to rolling channel release` workflow step.
# Moving the tag alone is not enough — assets are bound to a release, not a
# tag.
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
