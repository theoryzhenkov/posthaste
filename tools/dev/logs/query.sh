#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
default_log="$("$root/tools/dev/overmind/server-log-path.sh")"

log_file="$default_log"
level=""
target=""
event=""
account=""
sync_id=""
request_id=""
operation_id=""
operation_kind=""
operation_source=""
session_id=""
message=""
since=""
limit="200"
json="false"

usage() {
  cat <<EOF
Usage: tools/dev/logs/query.sh [options]

Options:
  --file PATH       JSONL log file to query (default: current dev server log)
  --level LEVEL     Match level exactly, e.g. INFO, WARN, ERROR
  --target TEXT     Match target substring
  --event NAME      Match stable event name
  --account ID      Match fields.account_id, span.account_id, or spans[].account_id
  --sync-id ID      Match fields.sync_id, span.sync_id, or spans[].sync_id
  --request-id ID   Match request correlation ID
  --operation-id ID Match operation correlation ID
  --operation-kind K Match operation kind, e.g. mail.search
  --operation-source S Match operation source, e.g. message-list
  --session-id ID   Match observability session ID
  --message TEXT    Match fields.message substring
  --since TIME      Keep events with timestamp >= TIME
  --limit N         Number of rows to print from the end (default: 200)
  --json            Print compact JSON instead of TSV summary rows
  -h, --help        Show this help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --file)
      log_file="${2:?--file requires a path}"
      shift 2
      ;;
    --level)
      level="${2:?--level requires a value}"
      shift 2
      ;;
    --target)
      target="${2:?--target requires a value}"
      shift 2
      ;;
    --event)
      event="${2:?--event requires a value}"
      shift 2
      ;;
    --account)
      account="${2:?--account requires a value}"
      shift 2
      ;;
    --sync-id)
      sync_id="${2:?--sync-id requires a value}"
      shift 2
      ;;
    --request-id)
      request_id="${2:?--request-id requires a value}"
      shift 2
      ;;
    --operation-id)
      operation_id="${2:?--operation-id requires a value}"
      shift 2
      ;;
    --operation-kind)
      operation_kind="${2:?--operation-kind requires a value}"
      shift 2
      ;;
    --operation-source)
      operation_source="${2:?--operation-source requires a value}"
      shift 2
      ;;
    --session-id)
      session_id="${2:?--session-id requires a value}"
      shift 2
      ;;
    --message)
      shift
      if [[ $# -eq 0 || "$1" == --* ]]; then
        echo "--message requires a value" >&2
        exit 2
      fi
      message="$1"
      shift
      while [[ $# -gt 0 && "$1" != --* ]]; do
        message="$message $1"
        shift
      done
      ;;
    --since)
      since="${2:?--since requires a value}"
      shift 2
      ;;
    --limit)
      limit="${2:?--limit requires a value}"
      shift 2
      ;;
    --json)
      json="true"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ ! -f "$log_file" ]]; then
  echo "log file does not exist: $log_file" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required to query JSON logs" >&2
  exit 127
fi

jq_filter='
  def field_message: .fields.message // .message // "";
  def field_value($name):
    .fields[$name] // .span[$name] // (first(.spans[]? | select(has($name)) | .[$name]) // "");
  def value_matches($name; $value):
    $value == ""
    or .fields[$name] == $value
    or .span[$name] == $value
    or any(.spans[]?; .[$name] == $value);

  select($level == "" or .level == ($level | ascii_upcase))
  | select($target == "" or ((.target // "") | contains($target)))
  | select(value_matches("event"; $event))
  | select($message == "" or (field_message | contains($message)))
  | select($since == "" or ((.timestamp // "") >= $since))
  | select(value_matches("account_id"; $account))
  | select(value_matches("sync_id"; $sync_id))
  | select(value_matches("request_id"; $request_id))
  | select(value_matches("operation_id"; $operation_id))
  | select(value_matches("operation_kind"; $operation_kind))
  | select(value_matches("operation_source"; $operation_source))
  | select(value_matches("session_id"; $session_id))
'

if [[ "$json" == "true" ]]; then
  jq -c \
    --arg level "$level" \
    --arg target "$target" \
    --arg event "$event" \
    --arg account "$account" \
    --arg sync_id "$sync_id" \
    --arg request_id "$request_id" \
    --arg operation_id "$operation_id" \
    --arg operation_kind "$operation_kind" \
    --arg operation_source "$operation_source" \
    --arg session_id "$session_id" \
    --arg message "$message" \
    --arg since "$since" \
    "$jq_filter" \
    "$log_file" | tail -n "$limit"
else
  jq -r \
    --arg level "$level" \
    --arg target "$target" \
    --arg event "$event" \
    --arg account "$account" \
    --arg sync_id "$sync_id" \
    --arg request_id "$request_id" \
    --arg operation_id "$operation_id" \
    --arg operation_kind "$operation_kind" \
    --arg operation_source "$operation_source" \
    --arg session_id "$session_id" \
    --arg message "$message" \
    --arg since "$since" \
    "$jq_filter
    | [
        (.timestamp // \"\"),
        (.level // \"\"),
        (.target // \"\"),
        field_value(\"event\"),
        field_value(\"account_id\"),
        field_value(\"sync_id\"),
        field_value(\"request_id\"),
        field_value(\"operation_id\"),
        field_value(\"operation_kind\"),
        field_message
      ]
    | @tsv" \
    "$log_file" | tail -n "$limit"
fi
