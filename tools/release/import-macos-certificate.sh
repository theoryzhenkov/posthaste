#!/usr/bin/env bash
set -euo pipefail

if [ -z "${APPLE_CERTIFICATE:-}" ]; then
  echo "APPLE_CERTIFICATE is not configured; macOS release will use ad-hoc signing."
  if [ -n "${GITHUB_ENV:-}" ]; then
    echo "POSTHASTE_MACOS_SIGNING=adhoc" >> "$GITHUB_ENV"
  fi
  exit 0
fi

if [ "$(uname -s)" != "Darwin" ]; then
  echo "ERROR: Apple certificate import requires a macOS runner." >&2
  exit 1
fi

if [ -z "${APPLE_CERTIFICATE_PASSWORD:-}" ]; then
  echo "ERROR: APPLE_CERTIFICATE_PASSWORD is required when APPLE_CERTIFICATE is set." >&2
  exit 1
fi

if [ -z "${KEYCHAIN_PASSWORD:-}" ]; then
  echo "ERROR: KEYCHAIN_PASSWORD is required when APPLE_CERTIFICATE is set." >&2
  exit 1
fi

if [ -z "${RUNNER_TEMP:-}" ]; then
  echo "ERROR: RUNNER_TEMP is required." >&2
  exit 1
fi

cert_path="$RUNNER_TEMP/posthaste-apple-certificate.p12"
keychain_path="$RUNNER_TEMP/posthaste-signing.keychain-db"

cleanup_cert() {
  rm -f "$cert_path"
}
trap cleanup_cert EXIT

if ! printf '%s' "$APPLE_CERTIFICATE" | base64 --decode > "$cert_path" 2>/dev/null; then
  printf '%s' "$APPLE_CERTIFICATE" | base64 -D > "$cert_path"
fi

security create-keychain -p "$KEYCHAIN_PASSWORD" "$keychain_path"
security set-keychain-settings -lut 21600 "$keychain_path"
security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$keychain_path"
security import "$cert_path" \
  -k "$keychain_path" \
  -P "$APPLE_CERTIFICATE_PASSWORD" \
  -T /usr/bin/codesign \
  -T /usr/bin/productsign \
  -T /usr/bin/security
security list-keychains -d user -s "$keychain_path" $(security list-keychains -d user | tr -d '"')
security default-keychain -d user -s "$keychain_path"
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KEYCHAIN_PASSWORD" "$keychain_path"

identities="$(security find-identity -v -p codesigning "$keychain_path")"
printf '%s\n' "$identities"

identity_line=""
if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  identity_line="$(printf '%s\n' "$identities" | grep -m1 -F "$APPLE_SIGNING_IDENTITY" || true)"
  if [ -z "$identity_line" ]; then
    echo "ERROR: APPLE_SIGNING_IDENTITY was provided but no matching imported identity was found." >&2
    exit 1
  fi
else
  for identity_kind in "Developer ID Application" "Apple Distribution"; do
    identity_line="$(printf '%s\n' "$identities" | grep -m1 -F "$identity_kind" || true)"
    if [ -n "$identity_line" ]; then
      break
    fi
  done
  if [ -z "$identity_line" ]; then
    identity_line="$(printf '%s\n' "$identities" | awk -F'\"' '/\"/ { print; exit }')"
  fi
fi

if [ -z "$identity_line" ]; then
  echo "ERROR: no codesigning identity found in imported Apple certificate." >&2
  exit 1
fi

identity="$(printf '%s\n' "$identity_line" | awk -F'\"' '{ print $2 }')"
if [ -z "$identity" ]; then
  echo "ERROR: failed to parse imported codesigning identity." >&2
  exit 1
fi

if [ -n "${GITHUB_ENV:-}" ]; then
  {
    printf 'APPLE_SIGNING_IDENTITY=%s\n' "$identity"
    printf 'POSTHASTE_MACOS_SIGNING=developer-id\n'
    printf 'POSTHASTE_MACOS_KEYCHAIN=%s\n' "$keychain_path"
  } >> "$GITHUB_ENV"
fi

echo "Imported Apple codesigning identity: $identity"
