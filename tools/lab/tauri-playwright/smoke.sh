#!/usr/bin/env bash
# Run the optional local Linux Tauri Playwright main-window smoke.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$root"

if ! command -v python3 >/dev/null 2>&1; then
  echo "Lab Tauri Playwright smoke requires python3 to write JSON artifacts." >&2
  exit 78
fi

random_suffix() {
  python3 - <<'PY'
import uuid
print(uuid.uuid4().hex[:16])
PY
}

shell_quote() {
  local value="${1-}"
  printf "'%s'" "${value//\'/\'\\\'\'}"
}

reproduction_command() {
  printf 'just lab tauri-smoke'
  for arg in "$@"; do
    printf ' %s' "$(shell_quote "$arg")"
  done
}

run_root="${POSTHASTE_LAB_TAURI_RUN_ROOT:-$root/target/lab/tpw}"
run_id="$(date -u +%Y%m%dT%H%M%SZ)-$(random_suffix)"
run_dir="$run_root/$run_id"
artifact_dir="$run_dir/artifacts"
config_root="$run_dir/config"
state_root="$run_dir/state"
secrets_root="$run_dir/secrets"
playwright_log="$artifact_dir/playwright.log"
playwright_results="$artifact_dir/playwright-results.json"
playwright_output_dir="$artifact_dir/playwright-output"
socket_path="$run_dir/pw.sock"
reproduction="$(reproduction_command "$@")"

mkdir -p "$artifact_dir" "$config_root" "$state_root" "$secrets_root"
chmod 700 "$run_dir"

export POSTHASTE_LAB_RUN_DIR="$run_dir"
export POSTHASTE_REPO_ROOT="$root"
export POSTHASTE_E2E_SOCKET="$socket_path"
export POSTHASTE_CONFIG_ROOT="$config_root"
export POSTHASTE_STATE_ROOT="$state_root"
export PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1

cleanup_lab_processes() {
  python3 - "$run_dir" <<'PY'
import os
import signal
import sys
import time
from pathlib import Path

run_dir = sys.argv[1].encode()
managed_terms = (
    b"posthaste-desktop",
    b"tauri dev",
    b"bun run tauri dev",
    b"bun run web:dev",
    b"vite --host 127.0.0.1 --port 5173 --strictPort",
)
current_pid = os.getpid()
matched = []
proc_root = Path("/proc")
if not proc_root.exists():
    raise SystemExit(0)
for proc_dir in proc_root.iterdir():
    if not proc_dir.name.isdigit():
        continue
    pid = int(proc_dir.name)
    if pid == current_pid:
        continue
    try:
        environ = (proc_dir / "environ").read_bytes()
        cmdline = (proc_dir / "cmdline").read_bytes().replace(b"\0", b" ")
    except (FileNotFoundError, PermissionError, ProcessLookupError):
        continue
    if b"POSTHASTE_LAB_RUN_DIR=" + run_dir + b"\0" not in environ:
        continue
    if not any(term in cmdline for term in managed_terms):
        continue
    try:
        os.kill(pid, signal.SIGTERM)
        matched.append(pid)
    except ProcessLookupError:
        pass

if matched:
    time.sleep(0.5)
for pid in matched:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        continue
    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
PY
}
trap cleanup_lab_processes EXIT

cat >"$run_dir/run.env" <<EOF
POSTHASTE_LAB_RUN_DIR=$POSTHASTE_LAB_RUN_DIR
POSTHASTE_E2E_SOCKET=$POSTHASTE_E2E_SOCKET
POSTHASTE_CONFIG_ROOT=$POSTHASTE_CONFIG_ROOT
POSTHASTE_STATE_ROOT=$POSTHASTE_STATE_ROOT
EOF
: >"$playwright_log"

write_lab_artifacts() {
  local status="${1:?status required}"
  local reason="${2:?reason required}"
  local exit_code="${3:?exit code required}"
  RUN_ID="$run_id" \
  RUN_DIR="$run_dir" \
  COMMAND_ID="cmd.lab.tauri-playwright.local" \
  REPRODUCTION_COMMAND="$reproduction" \
  SOCKET_PATH="$socket_path" \
  CONFIG_ROOT="$config_root" \
  STATE_ROOT="$state_root" \
  SECRETS_ROOT="$secrets_root" \
  PLAYWRIGHT_LOG="$playwright_log" \
  PLAYWRIGHT_RESULTS="$playwright_results" \
  PLAYWRIGHT_OUTPUT_DIR="$playwright_output_dir" \
  SUMMARY_STATUS="$status" \
  SUMMARY_REASON="$reason" \
  SUMMARY_EXIT_CODE="$exit_code" \
  python3 - <<'PY'
import json
import os
from pathlib import Path

run_dir = Path(os.environ["RUN_DIR"])
artifacts = {
    "playwrightLog": os.environ["PLAYWRIGHT_LOG"],
    "playwrightResults": os.environ["PLAYWRIGHT_RESULTS"],
    "playwrightOutputDir": os.environ["PLAYWRIGHT_OUTPUT_DIR"],
}
base = {
    "schemaVersion": 1,
    "runId": os.environ["RUN_ID"],
    "runDir": str(run_dir),
    "commandId": os.environ["COMMAND_ID"],
    "reproductionCommand": os.environ["REPRODUCTION_COMMAND"],
    "socketPath": os.environ["SOCKET_PATH"],
    "configRoot": os.environ["CONFIG_ROOT"],
    "stateRoot": os.environ["STATE_ROOT"],
    "secretsRoot": os.environ["SECRETS_ROOT"],
    "artifacts": artifacts,
}
manifest = {
    **base,
    "suiteIds": ["suite.desktop.main.linux.test"],
    "runnerIds": ["runner.tauri-playwright.linux.test"],
    "platform": {
        "os": os.uname().sysname,
        "machine": os.uname().machine,
    },
}
summary = {
    **base,
    "status": os.environ["SUMMARY_STATUS"],
    "reason": os.environ["SUMMARY_REASON"],
    "exitCode": int(os.environ["SUMMARY_EXIT_CODE"]),
}
(run_dir / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
(run_dir / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
PY
}

if ((${#socket_path} > 100)); then
  echo "Lab Tauri Playwright smoke socket path is too long for Unix sockets: $socket_path" >&2
  echo "Use a shorter POSTHASTE_LAB_TAURI_RUN_ROOT." >&2
  echo "Run dir: $run_dir" >&2
  write_lab_artifacts "blocked" "socket path too long" 78
  exit 78
fi

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Lab Tauri Playwright smoke is Linux-only for now." >&2
  echo "Run dir: $run_dir" >&2
  write_lab_artifacts "skipped" "unsupported platform" 78
  exit 78
fi

runner=()
if [[ -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
  if command -v xvfb-run >/dev/null 2>&1; then
    runner=(xvfb-run -a)
  elif [[ "${POSTHASTE_E2E_ALLOW_NO_DISPLAY:-}" != "1" ]]; then
    echo "Lab Tauri Playwright smoke needs a display, but DISPLAY/WAYLAND_DISPLAY are unset and xvfb-run is not installed." >&2
    echo "Run dir: $run_dir" >&2
    echo "Install/provide xvfb-run, run from a graphical session, or set POSTHASTE_E2E_ALLOW_NO_DISPLAY=1 to try anyway." >&2
    write_lab_artifacts "blocked" "display unavailable" 78
    exit 78
  fi
fi

echo "Lab Tauri Playwright smoke run dir: $run_dir"
set +e
"${runner[@]}" bun run lab:tauri-playwright "$@" 2>&1 | tee "$playwright_log"
exit_code=${PIPESTATUS[0]}
set -e

if [[ "$exit_code" -eq 0 ]]; then
  write_lab_artifacts "passed" "playwright exited successfully" "$exit_code"
else
  write_lab_artifacts "failed" "playwright exited with nonzero status" "$exit_code"
fi

exit "$exit_code"
