#!/usr/bin/env bash
set -euo pipefail

if [ -e secrets/release-signing.yaml ]; then
    echo "ERROR: secrets/release-signing.yaml already exists"
    echo "Delete or rotate it deliberately before generating a new release signing key."
    exit 1
fi

missing=()
for tool in age-keygen gpg sops; do
    command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
done

if [ ${#missing[@]} -gt 0 ]; then
    echo "ERROR: missing tools: ${missing[*]}"
    exit 1
fi

umask 077

if [ ! -f .age-key ]; then
    age-keygen -o .age-key >/dev/null
    echo "Generated .age-key"
fi

if [ ! -f .age-key.github ]; then
    age-keygen -o .age-key.github >/dev/null
    echo "Generated .age-key.github"
fi

GNUPGHOME="$(mktemp -d)"
cleanup() {
    gpgconf --homedir "$GNUPGHOME" --kill all >/dev/null 2>&1 || true
    rm -rf "$GNUPGHOME"
}
trap cleanup EXIT
export GNUPGHOME

uid="PostHaste Release Signing <releases@posthaste.local>"
gpg --batch --pinentry-mode loopback --passphrase "" --quick-generate-key "$uid" ed25519 sign 2y >/dev/null

fingerprint="$(gpg --batch --with-colons --list-keys "$uid" | awk -F: '/^fpr:/ { print $10; exit }')"
private_key="$(gpg --batch --armor --export-secret-keys "$fingerprint")"
public_key="$(gpg --batch --armor --export "$fingerprint")"

mkdir -p keys secrets
printf "%s\n" "$public_key" > keys/release-gpg-public.asc

{
    printf "release_gpg_key_id: \"%s\"\n" "$fingerprint"
    printf "release_gpg_private_key: |-\n"
    printf "%s\n" "$private_key" | sed "s/^/  /"
} | SOPS_AGE_KEY_FILE=.age-key sops --encrypt \
    --filename-override secrets/release-signing.yaml \
    --input-type yaml \
    --output-type yaml \
    /dev/stdin > secrets/release-signing.yaml

echo "Generated release GPG key $fingerprint"
echo "Encrypted private key: secrets/release-signing.yaml"
echo "Public key: keys/release-gpg-public.asc"
