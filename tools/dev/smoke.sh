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

require_path apps/web
require_path apps/desktop
require_path crates
require_path tools/dev
require_path tools/dev/stalwart/config.toml
require_path tools/dev/stalwart/seed.sh
require_path tools/dev/overmind/launch.sh

reject_path web
reject_path src-tauri
reject_path dev
reject_path crates/data

smoke_root="$root/var/dev/smoke-$$"
trap 'rm -rf "$smoke_root"' EXIT

run_layout_smoke() {
  local layout="${1:?layout required}"
  local offset="${2:?offset required}"
  local layout_root="$smoke_root/$layout"

  POSTHASTE_DEV_STACK_SMOKE=1 \
  POSTHASTE_STALWART_BIND="127.0.0.1:$((18080 + offset))" \
  POSTHASTE_STALWART_URL= \
  POSTHASTE_BIND="127.0.0.1:$((13001 + offset))" \
  POSTHASTE_VITE_HOST="127.0.0.1" \
  POSTHASTE_VITE_PORT="$((15173 + offset))" \
  VITE_API_BASE_URL= \
  POSTHASTE_CORS_ORIGIN= \
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

run_layout_smoke web 0
run_layout_smoke desktop 1
run_layout_smoke services 2

just --dry-run dev-web >/dev/null
just --dry-run dev-desktop >/dev/null
just --dry-run dev-services >/dev/null
just --dry-run frontend dev >/dev/null
just --dry-run desktop dev >/dev/null

echo "Dev layout smoke passed."
