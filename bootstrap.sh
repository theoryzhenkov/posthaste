#!/usr/bin/env bash
# Phase 1: prepare .envrc and .env so direnv can activate the flake devShell.
# Runs with only bash + direnv available (no nix tools required).
# After this completes, direnv activates the flake and `just setup` can run.
set -euo pipefail

# -- .env --
if [ ! -f .env ]; then
    cp .env.example .env
    echo "Created .env from .env.example"
else
    echo ".env already exists, skipping"
fi

# -- .envrc --
if [ ! -f .envrc ]; then
    cp .envrc.example .envrc
    echo "Created .envrc from .envrc.example"
else
    echo ".envrc already exists, skipping"
fi

# -- direnv --
if command -v direnv >/dev/null 2>&1; then
    direnv allow
    echo "direnv allowed. The flake devShell will activate on next cd into this directory."
else
    echo "WARNING: direnv not found. Install direnv + nix-direnv, or run 'nix develop' manually."
fi

echo
echo "Next: re-enter the directory (or run 'nix develop'), then run 'just setup'."
