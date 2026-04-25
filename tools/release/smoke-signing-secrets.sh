#!/usr/bin/env bash
set -euo pipefail

missing=()
for tool in gpg sops; do
    command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
done

if [ ${#missing[@]} -gt 0 ]; then
    echo "ERROR: missing tools: ${missing[*]}"
    exit 1
fi

age_key_file="${1:-.age-key}"

GNUPGHOME="$(mktemp -d)"
workdir="$(mktemp -d)"
cleanup() {
    gpgconf --homedir "$GNUPGHOME" --kill all >/dev/null 2>&1 || true
    rm -rf "$GNUPGHOME" "$workdir"
}
trap cleanup EXIT
export GNUPGHOME

key_id="$(SOPS_AGE_KEY_FILE="$age_key_file" sops --decrypt --extract '["release_gpg_key_id"]' secrets/release-signing.yaml)"
SOPS_AGE_KEY_FILE="$age_key_file" sops --decrypt --extract '["release_gpg_private_key"]' secrets/release-signing.yaml \
    | gpg --batch --import >/dev/null

printf "posthaste release signing smoke\n" > "$workdir/artifact.txt"
gpg --batch --yes --pinentry-mode loopback --armor --detach-sign --local-user "$key_id" "$workdir/artifact.txt"
gpg --batch --verify "$workdir/artifact.txt.asc" "$workdir/artifact.txt" >/dev/null

echo "gpg signing smoke ok"
