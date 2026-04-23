#!/usr/bin/env bash
# Launch the PostHaste dev stack with Overmind.
#
# Usage: launch.sh <web|desktop>
#
# Defaults point the daemon at an isolated dev-only config/state root
# (dev/posthaste/) so the real ~/.config/posthaste is never touched.
# Override any POSTHASTE_* env var before invoking to change behavior.
set -euo pipefail

layout="${1:?usage: launch.sh <web|desktop>}"
case "$layout" in
  web|desktop) ;;
  *) echo "unknown layout: $layout (expected web|desktop)" >&2; exit 2 ;;
esac

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

export POSTHASTE_STALWART_ADMIN_PASSWORD="${POSTHASTE_STALWART_ADMIN_PASSWORD:-devadmin}"
export POSTHASTE_STALWART_USER_PASSWORD="${POSTHASTE_STALWART_USER_PASSWORD:-devpass}"
export POSTHASTE_STALWART_DATA="${POSTHASTE_STALWART_DATA:-$root/dev/stalwart/data}"
export POSTHASTE_STALWART_LOGS="${POSTHASTE_STALWART_LOGS:-$root/dev/stalwart/logs}"

export POSTHASTE_CONFIG_ROOT="${POSTHASTE_CONFIG_ROOT:-$root/dev/posthaste/config}"
export POSTHASTE_STATE_ROOT="${POSTHASTE_STATE_ROOT:-$root/dev/posthaste/state}"
export POSTHASTE_BOOTSTRAP_PATH="${POSTHASTE_BOOTSTRAP_PATH:-$root/dev/bootstrap.stalwart.toml}"
export POSTHASTE_BIND="${POSTHASTE_BIND:-127.0.0.1:3001}"
export POSTHASTE_CORS_ORIGIN="${POSTHASTE_CORS_ORIGIN:-http://localhost:5173}"

mkdir -p \
  "$POSTHASTE_STALWART_DATA" "$POSTHASTE_STALWART_LOGS" \
  "$POSTHASTE_CONFIG_ROOT" "$POSTHASTE_STATE_ROOT"

if ! command -v overmind >/dev/null 2>&1; then
  echo "overmind is not on PATH; reload the Nix dev shell with 'direnv reload' or 'nix develop'." >&2
  exit 127
fi

exec overmind start -d "$root" -N -c seed -f "dev/Procfile.$layout"
