#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
default_log="$("$root/tools/dev/overmind/server-log-path.sh")"

log_file="$default_log"
level=""
target=""
account=""
sync_id=""
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
  --account ID      Match fields.account_id, span.account_id, or spans[].account_id
  --sync-id ID      Match fields.sync_id, span.sync_id, or spans[].sync_id
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
    --account)
      account="${2:?--account requires a value}"
      shift 2
      ;;
    --sync-id)
      sync_id="${2:?--sync-id requires a value}"
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
  def field_account: .fields.account_id // .span.account_id // "";
  def field_sync_id: .fields.sync_id // .span.sync_id // "";
  def account_matches($account):
    $account == ""
    or .fields.account_id == $account
    or .span.account_id == $account
    or any(.spans[]?; .account_id == $account);
  def sync_id_matches($sync_id):
    $sync_id == ""
    or .fields.sync_id == $sync_id
    or .span.sync_id == $sync_id
    or any(.spans[]?; .sync_id == $sync_id);

  select($level == "" or .level == ($level | ascii_upcase))
  | select($target == "" or ((.target // "") | contains($target)))
  | select($message == "" or (field_message | contains($message)))
  | select($since == "" or ((.timestamp // "") >= $since))
  | select(account_matches($account))
  | select(sync_id_matches($sync_id))
'

if [[ "$json" == "true" ]]; then
  jq -c \
    --arg level "$level" \
    --arg target "$target" \
    --arg account "$account" \
    --arg sync_id "$sync_id" \
    --arg message "$message" \
    --arg since "$since" \
    "$jq_filter" \
    "$log_file" | tail -n "$limit"
else
  jq -r \
    --arg level "$level" \
    --arg target "$target" \
    --arg account "$account" \
    --arg sync_id "$sync_id" \
    --arg message "$message" \
    --arg since "$since" \
    "$jq_filter
    | [
        (.timestamp // \"\"),
        (.level // \"\"),
        (.target // \"\"),
        (field_account // \"\"),
        (field_sync_id // \"\"),
        field_message
      ]
    | @tsv" \
    "$log_file" | tail -n "$limit"
fi
