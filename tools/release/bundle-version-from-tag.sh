#!/usr/bin/env bash
set -euo pipefail

# Derive the bundle/updater version from a release tag.
#
# From v0.2.0 onward the version is the **real semver** from the tag, so
# prerelease ordering survives (`0.2.0-beta.5 < 0.2.0-rc.1 < 0.2.0`) and the
# updater never sees a release as older than its own betas.
#
# The legacy `v0.1.0-dogfood.N` line keeps the old flattening (`0.1.N`) so
# already-shipped dogfood installs (which embed `0.1.N`) keep auto-updating.
# The flip to real semver happens at the v0.2.0 cut, which is semver-newer than
# any `0.1.N`.
#
#   v0.1.0-dogfood.N  -> 0.1.N            (legacy, preserves shipped installs)
#   vA.B.C-dogfood.N  -> A.B.C-dogfood.N  (v0.2.0+ real semver)
#   vA.B.C-beta.N     -> A.B.C-beta.N
#   vA.B.C-rc.N       -> A.B.C-rc.N
#   vA.B.C            -> A.B.C
#
# Usage:
#   bundle-version-from-tag.sh <tag>

tag="${1:?usage: bundle-version-from-tag.sh <tag>}"
tag="${tag#v}"

if [[ "$tag" =~ ^0\.1\.0-dogfood\.([0-9]+)$ ]]; then
  echo "0.1.${BASH_REMATCH[1]}"
elif [[ "$tag" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-(beta|rc|dogfood)\.[0-9]+)?$ ]]; then
  echo "$tag"
else
  echo "error: cannot derive bundle version from tag '$tag'" >&2
  exit 1
fi
