#!/usr/bin/env bash
# Optional mock Gmail IMAP dev provider. Runs only when POSTHASTE_DEV_GMAIL=1
# (otherwise it idles so overmind keeps a stable process list). Serves the
# Gmail-shaped IMAP fixture + a tiny HTTP control surface; pair it with the
# `local-gmail` account that launch.sh seeds under the same flag.
set -euo pipefail

if [[ "${POSTHASTE_DEV_GMAIL:-}" != "1" ]]; then
  echo "mock-gmail: disabled (set POSTHASTE_DEV_GMAIL=1 to enable)"
  # Idle without exiting so overmind doesn't treat it as a crashed process.
  exec sleep infinity
fi

exec cargo run -q -p posthaste-testkit --bin mock-gmail
