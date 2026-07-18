#!/usr/bin/env bash
# Validate local dev-stack wiring without starting long-running services.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

require_path() {
  local path="${1:?path required}"
  [[ -e "$path" ]] || { echo "missing required path: $path" >&2; exit 1; }
}

reject_path() {
  local path="${1:?path required}"
  [[ ! -e "$path" ]] || { echo "legacy path should not exist: $path" >&2; exit 1; }
}

require_path crates
require_path apps/client/frontend
require_path apps/client/backend
require_path tools/dev
require_path tools/dev/stalwart/config.toml
require_path tools/dev/stalwart/seed.sh
require_path tools/dev/overmind/launch.sh

reject_path web
reject_path src-tauri
reject_path dev
reject_path crates/data
reject_path legacy
reject_path apps/mcp

smoke_root="$root/var/dev/smoke-$$"
trap 'rm -rf "$smoke_root"' EXIT

run_layout_smoke() {
  local layout="${1:?layout required}"
  local offset="${2:?offset required}"
  local layout_root="$smoke_root/$layout"

  POSTHASTE_DEV_STACK_SMOKE=1 \
  POSTHASTE_STALWART_BIND="127.0.0.1:$((18080 + offset))" \
  POSTHASTE_STALWART_URL= \
  POSTHASTE_BOOTSTRAP_PATH= \
  POSTHASTE_STALWART_DATA="$layout_root/stalwart/data" \
  POSTHASTE_STALWART_LOGS="$layout_root/stalwart/logs" \
  POSTHASTE_CONFIG_ROOT="$layout_root/posthaste/config" \
  POSTHASTE_STATE_ROOT="$layout_root/posthaste/state" \
    bash tools/dev/overmind/launch.sh "$layout"

  require_path "$layout_root/stalwart/data"
  require_path "$layout_root/stalwart/logs"
  require_path "$layout_root/posthaste/config"
  require_path "$layout_root/posthaste/state/generated/bootstrap.stalwart.toml"
}

run_layout_smoke services 0

require_recipe() {
  just --dry-run "$@" >/dev/null 2>&1
}

reject_recipe() {
  if just --dry-run "$@" >/dev/null 2>&1; then
    echo "legacy recipe should not exist: just $*" >&2
    exit 1
  fi
}

require_recipe dev services
require_recipe dev smoke
require_recipe dev log path
require_recipe dev log tail
require_recipe dev log query --event http.request.completed
require_recipe lab suite list
require_recipe lab verify suite.lab.core.rust.test

reject_recipe dev web
reject_recipe dev desktop
reject_recipe web dev
reject_recipe desktop dev
reject_recipe mcp check
reject_recipe dev-web
reject_recipe dev-desktop
reject_recipe dev-services
reject_recipe dev-smoke
reject_recipe server-log-path
reject_recipe server-log-tail
reject_recipe server-log-query
reject_recipe frontend dev
reject_recipe lab-suite-list
reject_recipe lab-verify

echo "Dev layout smoke passed."
