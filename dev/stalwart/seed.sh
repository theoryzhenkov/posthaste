#!/usr/bin/env bash
# Provision the dev Stalwart instance with a domain + mailbox user.
# Idempotent: safe to re-run against an already-seeded server.
set -euo pipefail

: "${POSTHASTE_STALWART_ADMIN_PASSWORD:?must be set}"
: "${POSTHASTE_STALWART_USER_PASSWORD:?must be set}"

BASE="${POSTHASTE_STALWART_URL:-http://127.0.0.1:8080}"
ADMIN="admin:${POSTHASTE_STALWART_ADMIN_PASSWORD}"
DOMAIN="${POSTHASTE_STALWART_DOMAIN:-example.org}"
USER="dev"
EMAIL="${USER}@${DOMAIN}"
STALWART_DATA_ROOT="${POSTHASTE_STALWART_DATA:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/data}"
FIXTURE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/fixtures/maildir"
FIXTURE_MARKER="$STALWART_DATA_ROOT/.posthaste-fixtures-v1-imported"
STATE_ROOT="${POSTHASTE_STATE_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/../posthaste/state}"
SEED_READY_MARKER="$STATE_ROOT/.stalwart-seed-ready"

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
  # Always reconcile role, address, and password so re-running updates the dev account.
  curl -sf -u "$ADMIN" -X PATCH "$BASE/api/principal/$USER" \
    -H 'Content-Type: application/json' \
    -d "[
      {\"action\":\"set\",\"field\":\"roles\",\"value\":[\"user\"]},
      {\"action\":\"set\",\"field\":\"emails\",\"value\":[\"$EMAIL\"]},
      {\"action\":\"set\",\"field\":\"secrets\",\"value\":[\"$POSTHASTE_STALWART_USER_PASSWORD\"]}
    ]" >/dev/null
}

ensure_jmap_identity() {
  local session account_id request response created_id

  if ! command -v jq >/dev/null; then
    echo "jq is required to seed the dev JMAP identity" >&2
    exit 1
  fi

  session=$(curl -sfL -u "$USER:$POSTHASTE_STALWART_USER_PASSWORD" "$BASE/.well-known/jmap")
  account_id=$(printf '%s' "$session" | jq -r '.primaryAccounts["urn:ietf:params:jmap:mail"] // empty')
  if [[ -z "$account_id" ]]; then
    echo "could not discover JMAP mail account for $USER" >&2
    exit 1
  fi

  request=$(jq -cn --arg account_id "$account_id" '{
    using: ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
    methodCalls: [[
      "Identity/get",
      {
        accountId: $account_id,
        properties: ["id", "name", "email"]
      },
      "0"
    ]]
  }')
  response=$(curl -sf -u "$USER:$POSTHASTE_STALWART_USER_PASSWORD" "$BASE/jmap/" \
    -H 'Content-Type: application/json' \
    -d "$request")

  if printf '%s' "$response" | jq -e --arg email "$EMAIL" \
    '.methodResponses[0][1].list[]? | select(.email == $email)' >/dev/null; then
    echo "JMAP identity $EMAIL already exists"
    return
  fi

  request=$(jq -cn --arg account_id "$account_id" --arg email "$EMAIL" '{
    using: ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
    methodCalls: [[
      "Identity/set",
      {
        accountId: $account_id,
        create: {
          "posthaste-dev": {
            name: "Posthaste Dev",
            email: $email
          }
        }
      },
      "0"
    ]]
  }')
  response=$(curl -sf -u "$USER:$POSTHASTE_STALWART_USER_PASSWORD" "$BASE/jmap/" \
    -H 'Content-Type: application/json' \
    -d "$request")
  created_id=$(printf '%s' "$response" | jq -r \
    '.methodResponses[0][1].created["posthaste-dev"].id // empty')
  if [[ -z "$created_id" ]]; then
    echo "failed to create JMAP identity for $EMAIL: $response" >&2
    exit 1
  fi

  echo "created JMAP identity $EMAIL"
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
ensure_jmap_identity
import_fixture_messages
mkdir -p "$STATE_ROOT"
touch "$SEED_READY_MARKER"

echo "seeded: login as '$USER' with POSTHASTE_STALWART_USER_PASSWORD at $BASE"
