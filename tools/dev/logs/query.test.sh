#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
log_file="$(mktemp)"
trap 'rm -f "$log_file"' EXIT

cat >"$log_file" <<'JSONL'
{"timestamp":"2026-04-28T12:00:00Z","level":"INFO","target":"posthaste_server","fields":{"event":"http.request.completed","message":"http request completed","status":200},"span":{"request_id":"req_1","operation_id":"op_1","operation_kind":"mail.search","name":"http.request"}}
{"timestamp":"2026-04-28T12:00:01Z","level":"DEBUG","target":"posthaste_server::supervisor","fields":{"event":"cache.fetch.completed","message":"cache worker batch completed","account_id":"local-stalwart","operation_id":"op_1"}}
{"timestamp":"2026-04-28T12:00:02Z","level":"INFO","target":"posthaste_server","fields":{"event":"http.request.completed","message":"http request completed","status":200},"span":{"request_id":"req_2","operation_id":"op_2","operation_kind":"mail.list","name":"http.request"}}
JSONL

event_count="$(
  bash "$root/tools/dev/logs/query.sh" \
    --file "$log_file" \
    --event http.request.completed \
    --json \
    | jq -s 'length'
)"
[[ "$event_count" == "2" ]] || {
  echo "expected two http.request.completed events, got $event_count" >&2
  exit 1
}

operation_count="$(
  bash "$root/tools/dev/logs/query.sh" \
    --file "$log_file" \
    --operation-id op_1 \
    --json \
    | jq -s 'length'
)"
[[ "$operation_count" == "2" ]] || {
  echo "expected two op_1 events, got $operation_count" >&2
  exit 1
}

search_count="$(
  bash "$root/tools/dev/logs/query.sh" \
    --file "$log_file" \
    --operation-kind mail.search \
    --json \
    | jq -s 'length'
)"
[[ "$search_count" == "1" ]] || {
  echo "expected one mail.search event, got $search_count" >&2
  exit 1
}

tsv="$(
  bash "$root/tools/dev/logs/query.sh" \
    --file "$log_file" \
    --event cache.fetch.completed \
    --limit 1
)"
[[ "$tsv" == *$'cache.fetch.completed\t'* ]] || {
  echo "expected TSV output to include the event column" >&2
  exit 1
}
