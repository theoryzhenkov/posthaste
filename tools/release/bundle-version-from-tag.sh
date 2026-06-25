#!/usr/bin/env bash
set -euo pipefail

# Derive the bundle version from a release tag.
#
# macOS CFBundleShortVersionString requires three non-negative integers, so
# dogfood/beta/rc tags are rewritten; plain stable tags pass through unchanged.
#
#   vA.B.C-dogfood.N  -> A.B.N
#   vA.B.C-beta.N     -> A.B.N
#   vA.B.C-rc.N       -> A.B.N
#   vA.B.C            -> A.B.C
#
# Usage:
#   bundle-version-from-tag.sh <tag>

tag="${1:?usage: bundle-version-from-tag.sh <tag>}"
tag="${tag#v}"

if [[ "$tag" =~ ^([0-9]+)\.([0-9]+)\.[0-9]+-(dogfood|beta|rc)\.([0-9]+)$ ]]; then
  echo "${BASH_REMATCH[1]}.${BASH_REMATCH[2]}.${BASH_REMATCH[4]}"
elif [[ "$tag" =~ ^([0-9]+\.[0-9]+\.[0-9]+)$ ]]; then
  echo "${BASH_REMATCH[1]}"
else
  echo "error: cannot derive bundle version from tag '$tag'" >&2
  exit 1
fi
