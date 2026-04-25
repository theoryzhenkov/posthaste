#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
state_root="${POSTHASTE_STATE_ROOT:-$root/var/dev/posthaste/state}"
seed_ready_marker="$state_root/.stalwart-seed-ready"

for _ in $(seq 1 120); do
  if [[ -f "$seed_ready_marker" ]]; then
    exec cargo run --bin posthaste -- serve --api-only
  fi
  sleep 0.5
done

echo "seed readiness marker not found: $seed_ready_marker" >&2
exit 1
