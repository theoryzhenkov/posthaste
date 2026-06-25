#!/usr/bin/env bash
set -euo pipefail

# Resolve the release channel and related flags from a release tag.
#
# Outputs name=value pairs, one per line, suitable for GITHUB_OUTPUT or for
# sourcing with `set -a`/`source`. Unknown tag shapes are rejected.
#
# Usage:
#   resolve-channel.sh <tag>
#
# Example:
#   resolve-channel.sh v0.1.0-dogfood.42

tag="${1:?usage: resolve-channel.sh <tag>}"

# Tag-pattern matching uses BASH_REMATCH for readability and does not write
# intermediate files.
channel=""
if [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+-dogfood\.[0-9]+$ ]]; then
  channel="nightly"
elif [[ "$tag" =~ -nightly\. ]]; then
  channel="nightly"
elif [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-(beta|rc)\.[0-9]+)?$ ]]; then
  channel="stable"
fi

if [ -z "$channel" ]; then
  echo "error: release tag '$tag' does not match a known channel pattern" >&2
  exit 1
fi

case "$channel" in
  nightly)
    include_devtools="true"
    enforce_macos_signing="false"
    run_artifact_smoke="true"
    updater_manifest_filename="latest.json"
    is_stable="false"
    ;;
  stable)
    include_devtools="false"
    enforce_macos_signing="true"
    run_artifact_smoke="true"
    updater_manifest_filename="latest-stable.json"
    is_stable="true"
    ;;
  *)
    echo "error: unreachable channel '$channel'" >&2
    exit 1
    ;;
esac

cat <<EOF
POSTHASTE_RELEASE_CHANNEL=$channel
POSTHASTE_INCLUDE_DEVTOOLS=$include_devtools
POSTHASTE_ENFORCE_MACOS_SIGNING=$enforce_macos_signing
POSTHASTE_RUN_ARTIFACT_SMOKE=$run_artifact_smoke
POSTHASTE_UPDATER_MANIFEST_FILENAME=$updater_manifest_filename
POSTHASTE_IS_STABLE=$is_stable
EOF
