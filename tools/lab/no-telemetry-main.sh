#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../.."

forbidden_paths=(
  "crates/posthaste-telemetry"
  "crates/posthaste-telemetry-ingest"
  "deploy/telemetry"
  ".github/workflows/telemetry-image.yml"
)

for path in "${forbidden_paths[@]}"; do
  if [[ -f "$path" ]] || { [[ -d "$path" ]] && [[ -n "$(find "$path" -mindepth 1 -print -quit)" ]]; }; then
    echo "forbidden telemetry artifact present on dogfood/main: $path" >&2
    exit 1
  fi
done

forbidden_refs=$(rg -n --hidden --glob '!docs/**' --glob '!tmp/**' --glob '!target/**' \
  'posthaste-telemetry|telemetry-ingest|telemetry-image|deploy/telemetry' \
  Cargo.toml package.json bun.lock justfile .github/workflows apps crates deploy || true)
if [[ -n "$forbidden_refs" ]]; then
  printf '%s\n' "$forbidden_refs" >&2
  echo "forbidden telemetry reference present on dogfood/main" >&2
  exit 1
fi

echo "No active telemetry artifacts found on dogfood/main."
