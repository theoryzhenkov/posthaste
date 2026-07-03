#!/usr/bin/env bash
set -euo pipefail

# Codesign + notarize the macOS posthastectl CLI binaries and package every
# platform's CLI binary as a .tar.gz, mirroring how the daemon/authority/
# runtime/wizard binaries are shipped (tools/package/bin.sh, daemon.sh).
#
# Why: the build-cli job cross-compiles all five targets from a single Linux
# runner via Bun, so the darwin binaries left that pipeline as bare, unsigned
# executables. On Apple Silicon an unsigned binary is a hard Gatekeeper
# "is damaged and can't be opened" — arm64 requires *some* valid signature to
# execute at all. Shipping bare binaries (rather than an archive) also meant a
# browser download could lose its executable bit and inherit the quarantine
# flag straight onto the file users try to run.
# See docs/eph/RFC-L2-scripting.md §7.10(a).
#
# Signing identity mirrors the app-bundle path (build-desktop job): Developer
# ID Application + hardened runtime + secure timestamp + notarization when
# the release signing secrets (APPLE_CERTIFICATE et al.) are configured —
# for BOTH channels, not just stable. Ad-hoc (`codesign -s -`) is only the
# fallback when those secrets are absent (forks / PR builds / no signing
# infra), same as tools/release/import-macos-certificate.sh already decides
# for the desktop app. enforce_macos_signing (channel-policy.sh) additionally
# makes Developer ID + successful notarization *mandatory* on the stable
# channel, matching the "Require Apple developer signing for macOS release" /
# "Stable macOS release builds require notarization credentials" gates in the
# build-desktop job.
#
# Notarization ships as zip submission (`ditto` + `notarytool submit --wait`),
# same as Apple's documented flow for standalone command-line tools. Stapling
# is deliberately NOT attempted: `xcrun stapler staple` only supports
# .app/.pkg/.dmg/.framework/.kext bundles, not a bare mach-o executable or an
# ad-hoc tarball around one. A notarized-but-unstapled CLI binary is fully
# supported by Apple's docs — Gatekeeper performs an online ticket check
# against Apple's servers on first launch instead, which means the very first
# run needs network access. That online-check behavior (and whether the
# hardened-runtime entitlements are sufficient for the Bun JIT) can only be
# proven by an actual signed nightly build run on a real Mac; see the
# companion release.yml job for what to verify next.
#
# Usage:
#   sign-and-package-cli.sh <raw-dir> <out-dir>
#
# Env (see tools/release/channel-policy.sh):
#   POSTHASTE_CLI_NAME               e.g. PosthasteCTLNightly
#   POSTHASTE_ENFORCE_MACOS_SIGNING  true|false — stable requires Developer ID
#                                     + successful notarization (hard fail
#                                     otherwise); nightly signs the same way
#                                     when secrets are present but tolerates
#                                     ad-hoc / unnotarized as a soft fallback.
#   POSTHASTE_MACOS_SIGNING          developer-id|adhoc, set by
#                                     tools/release/import-macos-certificate.sh
#   APPLE_SIGNING_IDENTITY           required when POSTHASTE_MACOS_SIGNING=developer-id
#   POSTHASTE_ENTITLEMENTS_PLIST     path to the hardened-runtime entitlements
#                                     plist (defaults to the file next to this
#                                     script: posthastectl-entitlements.plist)
#
# Notarization credentials (either shape, same as build-desktop):
#   POSTHASTE_APPLE_API_KEY / POSTHASTE_APPLE_API_ISSUER / POSTHASTE_APPLE_API_KEY_PATH
#   POSTHASTE_APPLE_ID / POSTHASTE_APPLE_PASSWORD / POSTHASTE_APPLE_TEAM_ID

raw_dir="${1:?usage: sign-and-package-cli.sh <raw-dir> <out-dir>}"
out_dir="${2:?usage: sign-and-package-cli.sh <raw-dir> <out-dir>}"
cli_name="${POSTHASTE_CLI_NAME:?POSTHASTE_CLI_NAME is required (see tools/release/channel-policy.sh)}"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
entitlements_plist="${POSTHASTE_ENTITLEMENTS_PLIST:-$script_dir/posthastectl-entitlements.plist}"

if [ ! -d "$raw_dir" ]; then
  echo "error: raw binary directory '$raw_dir' does not exist" >&2
  exit 1
fi

mkdir -p "$out_dir"

have_codesign() { command -v codesign >/dev/null 2>&1; }

# Resolve whether notarization credentials are usable, mirroring
# build-desktop's configure_notarization: prefer the App Store Connect API
# key shape, fall back to Apple ID + app-specific password, deriving the team
# ID from the signing identity string if it was not supplied explicitly.
notarization_args=()
notarization_configured=false
resolve_notarization_credentials() {
  if [ -n "${POSTHASTE_APPLE_API_KEY:-}" ] || [ -n "${POSTHASTE_APPLE_API_ISSUER:-}" ] || [ -n "${POSTHASTE_APPLE_API_KEY_PATH:-}" ]; then
    if [ -n "${POSTHASTE_APPLE_API_KEY:-}" ] && [ -n "${POSTHASTE_APPLE_API_ISSUER:-}" ] && [ -n "${POSTHASTE_APPLE_API_KEY_PATH:-}" ]; then
      notarization_args=(--key "$POSTHASTE_APPLE_API_KEY_PATH" --key-id "$POSTHASTE_APPLE_API_KEY" --issuer "$POSTHASTE_APPLE_API_ISSUER")
      notarization_configured=true
      echo "notarization: using App Store Connect API key."
    else
      echo "notarization: incomplete App Store Connect API key secrets; skipping." >&2
    fi
    return
  fi

  local team_id="${POSTHASTE_APPLE_TEAM_ID:-}"
  if [ -z "$team_id" ] && [[ "${APPLE_SIGNING_IDENTITY:-}" =~ \(([A-Z0-9]{3,})\)$ ]]; then
    team_id="${BASH_REMATCH[1]}"
  fi

  if [ -n "${POSTHASTE_APPLE_ID:-}" ] || [ -n "${POSTHASTE_APPLE_PASSWORD:-}" ] || [ -n "$team_id" ]; then
    if [ -n "${POSTHASTE_APPLE_ID:-}" ] && [ -n "${POSTHASTE_APPLE_PASSWORD:-}" ] && [ -n "$team_id" ]; then
      notarization_args=(--apple-id "$POSTHASTE_APPLE_ID" --password "$POSTHASTE_APPLE_PASSWORD" --team-id "$team_id")
      notarization_configured=true
      echo "notarization: using Apple ID credentials."
    else
      echo "notarization: incomplete Apple ID notarization secrets; skipping." >&2
    fi
  else
    echo "notarization: no credentials configured; CLI will be signed but not notarized." >&2
  fi
}

sign_darwin_binary() {
  local bin="$1"

  if ! have_codesign; then
    # Not on a macOS runner. Real release jobs must run this on macos-latest
    # (codesign/xcrun are Darwin-only); this fallback exists purely so the
    # packaging half of this script can be dry-run on Linux.
    echo "warning: codesign not found; leaving $bin unsigned (dry run?)" >&2
    return 0
  fi

  if [ "${POSTHASTE_MACOS_SIGNING:-adhoc}" = "developer-id" ]; then
    : "${APPLE_SIGNING_IDENTITY:?APPLE_SIGNING_IDENTITY is required when POSTHASTE_MACOS_SIGNING=developer-id}"
    if [ ! -f "$entitlements_plist" ]; then
      echo "error: entitlements plist not found at $entitlements_plist" >&2
      exit 1
    fi
    echo "codesign (Developer ID, hardened runtime: $APPLE_SIGNING_IDENTITY): $bin"
    codesign --force --options runtime --timestamp \
      --entitlements "$entitlements_plist" \
      -s "$APPLE_SIGNING_IDENTITY" "$bin"
  else
    echo "codesign (ad-hoc fallback — no signing secrets configured): $bin"
    codesign -s - --force "$bin"
  fi
  codesign --verify --verbose=2 "$bin"
}

notarize_darwin_binary() {
  local bin="$1"
  local name; name="$(basename "$bin")"

  if [ "${POSTHASTE_MACOS_SIGNING:-adhoc}" != "developer-id" ]; then
    if [ "${POSTHASTE_ENFORCE_MACOS_SIGNING:-false}" = "true" ]; then
      echo "error: stable macOS CLI binaries require Developer ID signing; got ad-hoc for $name." >&2
      exit 1
    fi
    echo "notarization: skipped for $name (ad-hoc signed)." >&2
    return 0
  fi

  if [ "$notarization_configured" != "true" ]; then
    if [ "${POSTHASTE_ENFORCE_MACOS_SIGNING:-false}" = "true" ]; then
      echo "error: stable macOS CLI binaries require notarization credentials; none configured for $name." >&2
      exit 1
    fi
    echo "notarization: skipped for $name (no credentials)." >&2
    return 0
  fi

  local work_dir zip_path
  work_dir="$(mktemp -d)"
  zip_path="$work_dir/${name}.zip"
  # Apple's documented flow for standalone command-line tools: zip (ditto
  # preserves the mach-o's code signature/metadata better than `zip`) then
  # submit for notarization. No stapling — see the file header.
  ditto -c -k --keepParent "$bin" "$zip_path"

  echo "notarizing $name..."
  if ! xcrun notarytool submit "$zip_path" "${notarization_args[@]}" --wait; then
    rm -rf "$work_dir"
    if [ "${POSTHASTE_ENFORCE_MACOS_SIGNING:-false}" = "true" ]; then
      echo "error: notarization failed for $name (stable release requires it)." >&2
      exit 1
    fi
    echo "warning: notarization failed for $name; shipping Developer ID signed but unnotarized." >&2
    return 0
  fi
  rm -rf "$work_dir"
  echo "notarized $name (ticket recorded with Apple; not stapled — see file header)."
}

resolve_notarization_credentials

found=0
for path in "$raw_dir/$cli_name"-*; do
  [ -e "$path" ] || continue
  found=1
  name="$(basename "$path")"

  case "$name" in
  *-darwin-*)
    chmod +x "$path"
    sign_darwin_binary "$path"
    notarize_darwin_binary "$path"
    ;;
  *)
    # Linux/Windows CLI binaries have no signing story yet; only the exec
    # bit + archive packaging apply to them.
    chmod +x "$path" 2>/dev/null || true
    ;;
  esac

  # Outer archive name drops a trailing .exe (Windows targets) so it matches
  # the <artifact>-<platform>.tar.gz convention install.sh/tools/package/*.sh
  # already use; the binary's real filename (incl. .exe) is preserved as the
  # bare entry inside the tarball.
  archive_base="${name%.exe}"
  tar -C "$raw_dir" -czf "$out_dir/${archive_base}.tar.gz" "$name"
  echo "packaged $out_dir/${archive_base}.tar.gz"
done

if [ "$found" -eq 0 ]; then
  echo "error: no $cli_name-* binaries found in $raw_dir" >&2
  exit 1
fi
