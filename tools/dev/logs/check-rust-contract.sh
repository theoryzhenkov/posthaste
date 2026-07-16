#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
status=0

while IFS= read -r match; do
  file="${match%%:*}"
  case "$file" in
    */crates/posthaste-observability/src/lib.rs)
      continue
      ;;
  esac
  echo "raw tracing log macro is not allowed: $match" >&2
  status=1
done < <(
  rg -n \
    '(^|[^[:alnum:]_])(tracing::)?(trace|debug|info|warn|error)!\(' \
    "$root/crates" "$root/legacy/desktop/src" \
    --glob '*.rs' \
    --glob '!crates/posthaste-observability/src/lib.rs'
)

if rg -n 'ph_forwarded_(trace|debug|info|warn|error)!' "$root/crates" "$root/legacy/desktop/src" --glob '*.rs' \
  | rg -v '/legacy/desktop/src/(lib|frontend_logging)\.rs:' >/tmp/posthaste-forwarded-log-contract.$$; then
  cat /tmp/posthaste-forwarded-log-contract.$$ | while IFS= read -r match; do
    echo "forwarded dynamic log macro is only allowed in the desktop frontend bridge: $match" >&2
  done
  status=1
fi
rm -f /tmp/posthaste-forwarded-log-contract.$$

exit "$status"
