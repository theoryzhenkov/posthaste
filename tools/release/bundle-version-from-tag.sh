#!/usr/bin/env bash
set -euo pipefail

# Derive the bundle/updater version from a release tag.
#
# Accepted v0.2.0+ line uses real semver:
#   vA.B.C-nightly.N  -> A.B.C-nightly.N
#   vA.B.C-rc.N       -> A.B.C-rc.N
#   vA.B.C            -> A.B.C
#
# The legacy v0.1.0-dogfood.N line keeps the old flattening (0.1.N) so
# already-shipped dogfood installs keep auto-updating until they move to the
# 0.2.0 nightly stream.
#
# Usage:
#   bundle-version-from-tag.sh <tag>

tag="${1:?usage: bundle-version-from-tag.sh <tag>}"
tag="${tag#v}"

if [[ "$tag" =~ ^0\.1\.0-dogfood\.([0-9]+)$ ]]; then
  # Legacy dogfood flatten for already-shipped v0.1.x installs.
  echo "0.1.${BASH_REMATCH[1]}"
elif [[ "$tag" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-(rc|nightly)\.[0-9]+)?$ ]]; then
  echo "$tag"
else
  echo "error: cannot derive bundle version from tag '$tag'" >&2
  exit 1
fi
