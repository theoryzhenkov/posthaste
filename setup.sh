#!/usr/bin/env bash
# Phase 2: project setup. Requires the flake devShell to be active
# (age, git, sops, jj in PATH). Run `./bootstrap.sh` first.
set -euo pipefail

TEMPLATE_REMOTE_NAME="${TEMPLATE_REMOTE_NAME:-template}"
TEMPLATE_REMOTE_URL="${TEMPLATE_REMOTE_URL:-git@github.com:theoryzhenkov/repo_template.base.git}"

# -- tool check --
missing=()
for tool in age-keygen git sops jj; do
    command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
done
if [ ${#missing[@]} -gt 0 ]; then
    echo "ERROR: missing tools: ${missing[*]}"
    echo "Run ./bootstrap.sh first, or enter the devShell with 'nix develop'."
    exit 1
fi

# -- age key --
SOPS_UPDATED=0
if [ ! -f .age-key ]; then
    age-keygen -o .age-key 2>&1
    PUBLIC_KEY=$(age-keygen -y .age-key)
    sed -i "s|REPLACE_WITH_AGE_PUBLIC_KEY|$PUBLIC_KEY|" .sops.yaml
    SOPS_UPDATED=1
    echo "Generated .age-key and updated .sops.yaml"
else
    echo ".age-key already exists, skipping"
fi

# -- jj --
if [ ! -d .jj ]; then
    jj git init --colocate
    echo "Initialized colocated jj repository"
else
    echo "jj repository already exists, skipping"
fi

# -- template remote --
if [ -n "$TEMPLATE_REMOTE_URL" ]; then
    if git remote get-url "$TEMPLATE_REMOTE_NAME" >/dev/null 2>&1; then
        echo "$TEMPLATE_REMOTE_NAME remote already exists, skipping"
    else
        git remote add "$TEMPLATE_REMOTE_NAME" "$TEMPLATE_REMOTE_URL"
        echo "Added $TEMPLATE_REMOTE_NAME remote: $TEMPLATE_REMOTE_URL"
    fi
fi

echo "Done."

if [ "$SOPS_UPDATED" -eq 1 ]; then
    echo
    echo "WARNING: .sops.yaml has been updated with your age public key."
    echo "         Commit this change: jj desc -m 'chore: initial project setup'"
fi
