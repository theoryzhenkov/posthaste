#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: tools/release/set-macos-signing-secrets.sh path/to/certificate.p12

Stores the macOS signing certificate in GitHub Actions repository secrets.
The p12 file must be the password-protected export from Keychain Access.

Required tools: gh, openssl
Optional env:
  APPLE_CERTIFICATE_PASSWORD  p12 export password; prompted if omitted
  KEYCHAIN_PASSWORD           CI keychain password; generated if omitted
  APPLE_SIGNING_IDENTITY      Exact codesigning identity if the p12 has more than one
  GH_REPO                     GitHub owner/repo; defaults to origin
EOF
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  usage
  exit 0
fi

p12_path="${1:-}"
if [ -z "$p12_path" ]; then
  usage >&2
  exit 2
fi

if [ ! -f "$p12_path" ]; then
  echo "ERROR: p12 file not found: $p12_path" >&2
  exit 1
fi

missing=()
for tool in gh openssl; do
  command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
done

if [ ${#missing[@]} -gt 0 ]; then
  echo "ERROR: missing tools: ${missing[*]}" >&2
  exit 1
fi

if ! gh auth status >/dev/null 2>&1; then
  echo "ERROR: gh is not authenticated for this repository." >&2
  exit 1
fi

repo="${GH_REPO:-}"
origin_url=""
if [ -z "$repo" ]; then
  origin_url="$(git config --get remote.origin.url || true)"
  repo="$(printf '%s' "$origin_url" | sed -E 's#^git@github.com:##; s#^https://github.com/##; s#\.git$##')"
fi
if [ -z "$repo" ] || [ "$repo" = "$origin_url" ]; then
  echo "ERROR: cannot determine GitHub repository; set GH_REPO=owner/repo." >&2
  exit 1
fi

if [ -z "${APPLE_CERTIFICATE_PASSWORD:-}" ]; then
  printf 'p12 export password: ' >&2
  read -rs APPLE_CERTIFICATE_PASSWORD
  printf '\n' >&2
fi

if [ -z "${APPLE_CERTIFICATE_PASSWORD:-}" ]; then
  echo "ERROR: p12 export password cannot be empty." >&2
  exit 1
fi

if [ -z "${KEYCHAIN_PASSWORD:-}" ]; then
  KEYCHAIN_PASSWORD="$(openssl rand -base64 32)"
fi

openssl base64 -A -in "$p12_path" | gh secret set -R "$repo" APPLE_CERTIFICATE
printf '%s' "$APPLE_CERTIFICATE_PASSWORD" | gh secret set -R "$repo" APPLE_CERTIFICATE_PASSWORD
printf '%s' "$KEYCHAIN_PASSWORD" | gh secret set -R "$repo" KEYCHAIN_PASSWORD

if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  printf '%s' "$APPLE_SIGNING_IDENTITY" | gh secret set -R "$repo" APPLE_SIGNING_IDENTITY
fi

echo "Configured APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD, and KEYCHAIN_PASSWORD secrets for $repo."
if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  echo "Configured APPLE_SIGNING_IDENTITY secret."
else
  echo "APPLE_SIGNING_IDENTITY was not set; CI will prefer Developer ID Application, then Apple Distribution."
fi
