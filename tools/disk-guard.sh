#!/usr/bin/env bash
#
# Disk guard for the project user's shared quota.
#
# The project user shares one ~50 GiB quota across the root checkout, every jj
# workspace's `target/`, and the toolchain caches (`.cargo`, `.rustup`). A cargo
# build that crosses the quota dies mid-way with "Disk quota exceeded (os error
# 122)", leaving a half-built `target/`. The recurring hog is `target/`
# duplication: each checkout/workspace keeps its own multi-GiB target full of
# large statically-linked test binaries, and nothing reclaims them.
#
# Modes:
#   guard   (default) — if usage >= threshold, `cargo clean` THIS workspace's
#                       target/ (it is about to be rebuilt anyway) to make room,
#                       then surface the other large target/ dirs it won't touch.
#                       Wired as a pre-step into `just build` / `just test`.
#   report            — print quota usage + every target/ dir under the checkout.
#   clean             — force `cargo clean` this workspace + report what it freed.
#
# Never blocks a build: on any quota-parse failure it warns and exits 0.
# Tunables: POSTHASTE_DISK_THRESHOLD_GIB (default 42).

set -uo pipefail

threshold_gib="${POSTHASTE_DISK_THRESHOLD_GIB:-42}"
mode="${1:-guard}"

# Quota'd filesystem backing $HOME (e.g. /dev/vdb1), then this user's used 1K
# blocks on it. Empty on failure (no quota tooling / unparseable).
fs="$(df -P "$HOME" 2>/dev/null | awk 'NR==2 {print $1}')"
usage_kb() {
  [ -n "${fs:-}" ] || return 1
  quota 2>/dev/null | awk -v fs="$fs" '$1==fs {gsub(/\*/,"",$2); print $2; f=1} END{exit !f}'
}
gib() { echo "$(( ${1:-0} / 1024 / 1024 ))"; }

report_targets() {
  echo "--- target/ dirs under $HOME/src (the recurring hog) ---"
  # shellcheck disable=SC2046
  du -sh $(find "$HOME/src" -maxdepth 4 -type d -name target 2>/dev/null) 2>/dev/null \
    | sort -rh | head -8 || true
}

clean_workspace() {
  local before after
  before="$(usage_kb || echo 0)"
  cargo clean 2>/dev/null || true
  after="$(usage_kb || echo "$before")"
  echo "disk-guard: reclaimed ~$(gib "$(( before - after ))") GiB; quota now $(gib "$after") GiB used"
}

case "$mode" in
  report)
    quota -s 2>/dev/null | tail -2 || echo "disk-guard: no quota info"
    report_targets
    ;;
  clean)
    clean_workspace
    ;;
  guard)
    used_kb="$(usage_kb)" || { echo "disk-guard: no quota info; skipping" >&2; exit 0; }
    used_gib="$(gib "$used_kb")"
    echo "disk-guard: quota ${used_gib} GiB used / ${threshold_gib} GiB threshold"
    if [ "$used_gib" -ge "$threshold_gib" ]; then
      echo "disk-guard: over threshold — reclaiming this workspace's target/ before building" >&2
      clean_workspace >&2
      report_targets >&2
    fi
    ;;
  *)
    echo "usage: disk-guard.sh [guard|report|clean]" >&2
    exit 2
    ;;
esac
