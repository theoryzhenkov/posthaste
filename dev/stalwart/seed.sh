#!/usr/bin/env bash
# Provision the dev Stalwart instance with a domain + mailbox user.
# Idempotent: safe to re-run against an already-seeded server.
set -euo pipefail

: "${POSTHASTE_STALWART_ADMIN_PASSWORD:?must be set}"
: "${POSTHASTE_STALWART_USER_PASSWORD:?must be set}"

BASE="${POSTHASTE_STALWART_URL:-http://127.0.0.1:8080}"
ADMIN="admin:${POSTHASTE_STALWART_ADMIN_PASSWORD}"
DOMAIN="localhost"
USER="dev"
EMAIL="${USER}@${DOMAIN}"
STALWART_DATA_ROOT="${POSTHASTE_STALWART_DATA:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/data}"
FIXTURE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/fixtures/maildir"
FIXTURE_MARKER="$STALWART_DATA_ROOT/.posthaste-fixtures-v1-imported"

wait_for_stalwart() {
  for _ in $(seq 1 60); do
    if curl -sf -o /dev/null -u "$ADMIN" "$BASE/api/principal?limit=1"; then
      return 0
    fi
    sleep 0.5
  done
  echo "stalwart not reachable at $BASE" >&2
  exit 1
}

principal_exists() {
  # Stalwart returns 200 with `{"error":"notFound"}` for missing principals,
  # so we must inspect the body rather than rely on HTTP status.
  local name="$1"
  local body
  body=$(curl -sf -u "$ADMIN" "$BASE/api/principal/$name") || return 1
  ! printf '%s' "$body" | grep -q '"error":"notFound"'
}

ensure_domain() {
  if principal_exists "$DOMAIN"; then
    echo "domain $DOMAIN already exists"
    return
  fi
  curl -sf -u "$ADMIN" -X POST "$BASE/api/principal" \
    -H 'Content-Type: application/json' \
    -d "{\"type\":\"domain\",\"name\":\"$DOMAIN\"}" >/dev/null
  echo "created domain $DOMAIN"
}

ensure_user() {
  if principal_exists "$USER"; then
    echo "user $USER already exists"
  else
    curl -sf -u "$ADMIN" -X POST "$BASE/api/principal" \
      -H 'Content-Type: application/json' \
      -d "{\"type\":\"individual\",\"name\":\"$USER\",\"description\":\"Posthaste dev mailbox\",\"secrets\":[\"$POSTHASTE_STALWART_USER_PASSWORD\"],\"emails\":[\"$EMAIL\"]}" >/dev/null
    echo "created user $USER ($EMAIL)"
  fi
  # Always reconcile role + password so re-running with a new password updates the secret.
  curl -sf -u "$ADMIN" -X PATCH "$BASE/api/principal/$USER" \
    -H 'Content-Type: application/json' \
    -d "[
      {\"action\":\"set\",\"field\":\"roles\",\"value\":[\"user\"]},
      {\"action\":\"set\",\"field\":\"secrets\",\"value\":[\"$POSTHASTE_STALWART_USER_PASSWORD\"]}
    ]" >/dev/null
}

stage_fixture_maildir() {
  local staged_root source rel dir mailbox_dir target_dir
  staged_root="$(mktemp -d)"
  mkdir -p "$staged_root/cur" "$staged_root/new" "$staged_root/tmp"

  while IFS= read -r source; do
    rel="${source#$FIXTURE_ROOT/}"
    dir="$(dirname "$rel")"
    mailbox_dir=""
    if [[ "$dir" != "." && "$dir" != "cur" && "$dir" != "new" ]]; then
      mailbox_dir="${dir%/cur}"
      mailbox_dir="${mailbox_dir%/new}"
    fi

    target_dir="$staged_root"
    if [[ -n "$mailbox_dir" ]]; then
      target_dir="$staged_root/$mailbox_dir"
    fi
    mkdir -p "$target_dir/cur" "$target_dir/new" "$target_dir/tmp"
    cp "$source" "$target_dir/cur/$(basename "$source")"
  done < <(find "$FIXTURE_ROOT" -type f | sort)

  printf '%s\n' "$staged_root"
}

import_fixture_messages() {
  local staged_root

  if [[ -e "$FIXTURE_MARKER" ]]; then
    echo "fixture messages already imported for $USER"
    return
  fi

  if [[ ! -d "$FIXTURE_ROOT" ]]; then
    echo "fixture maildir missing: $FIXTURE_ROOT" >&2
    exit 1
  fi

  staged_root="$(stage_fixture_maildir)"
  stalwart-cli -u "$BASE" -c "$ADMIN" import messages -f maildir "$USER" "$staged_root" >/dev/null
  rm -rf "$staged_root"

  mkdir -p "$STALWART_DATA_ROOT"
  touch "$FIXTURE_MARKER"
  echo "imported fixture messages for $USER from $FIXTURE_ROOT"
}

wait_for_stalwart
ensure_domain
ensure_user
import_fixture_messages

echo "seeded: login as '$USER' with POSTHASTE_STALWART_USER_PASSWORD at $BASE"
