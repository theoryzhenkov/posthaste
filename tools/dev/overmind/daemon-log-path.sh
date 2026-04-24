#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
state_root="${POSTHASTE_STATE_ROOT:-$root/var/dev/posthaste/state}"
log_dir="$state_root/logs"
today_log="$log_dir/posthaste.$(date +%F)"

if [[ -e "$today_log" ]]; then
  echo "$today_log"
  exit 0
fi

if [[ -d "$log_dir" ]]; then
  latest_log="$(find "$log_dir" -maxdepth 1 -type f -name 'posthaste.*' -print | sort | tail -n 1)"
  if [[ -n "$latest_log" ]]; then
    echo "$latest_log"
    exit 0
  fi
fi

echo "$today_log"
