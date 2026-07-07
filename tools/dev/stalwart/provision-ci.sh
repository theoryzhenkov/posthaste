#!/usr/bin/env bash
# Provision a pinned, checksum-verified Stalwart (mail server + CLI) for the
# send-path CI gate — WITHOUT the private nix flake that the runner cannot reach.
#
# Determinism (RFC send-path-gate): the version and both SHA-256 digests are
# pinned here; no `latest`, no unverified download. The two artifacts come from
# the SAME upstream release tag so server and CLI never drift. After extraction
# the only network the gate needs is loopback (Stalwart→Stalwart delivery); this
# script is the sole network dependency and it fails closed on a checksum miss.
#
# Layout mirrors the local dev/nix stack so CI and desks don't diverge:
#   * `stalwart`      — the server binary the StalwartFixture spawns on loopback
#                       (crates/posthaste-testkit/src/stalwart.rs, via
#                       POSTHASTE_STALWART_BIN).
#   * `stalwart-cli`  — required by tools/dev/stalwart/seed.sh to import the
#                       fixture maildir (`stalwart-cli ... import messages`).
#                       The previous soft CI job fetched only the server and so
#                       silently failed at seed time — this fetches both.
#
# In GitHub Actions (GITHUB_ENV/GITHUB_PATH present) it exports
# POSTHASTE_STALWART_BIN and prepends the bin dir to PATH for later steps.
# Locally it prints `export …` lines on stdout for `eval "$(…)"`; all logging
# goes to stderr so the eval capture stays clean.
#
# Dev-desk escape hatch: the release artifacts are glibc builds. On a host whose
# loader is not /lib64/ld-linux-x86-64.so.2 (e.g. NixOS dev shells) the pinned
# binary cannot exec. With POSTHASTE_STALWART_ALLOW_PATH_FALLBACK=1 the script
# then reuses an on-PATH `stalwart`/`stalwart-cli` of the EXACT pinned version
# (what the nix devshell provides), so the same script drives the gate at a desk.
# CI never sets that flag: there a checksum-verified download that cannot exec is
# a hard failure.
set -euo pipefail

STALWART_VERSION="${STALWART_VERSION:-0.15.5}"
# SHA-256 of the x86_64-unknown-linux-gnu release tarballs for the pinned tag.
# Verified against github.com/stalwartlabs/stalwart releases/download/v0.15.5.
STALWART_SERVER_SHA256="38ea325845c6de77e062d3295b107d01cb804205de1f8186a78f6ffdec1a2832"
STALWART_CLI_SHA256="2f886bfa80bc037431d012d80452e9e6a4c3272313b8dbf64393547f115061cd"

ARCH_TRIPLE="x86_64-unknown-linux-gnu"
BASE_URL="https://github.com/stalwartlabs/stalwart/releases/download/v${STALWART_VERSION}"
DEST="${STALWART_PROVISION_DIR:-${RUNNER_TEMP:-/tmp}/stalwart-${STALWART_VERSION}}"

log() { printf '[provision-stalwart] %s\n' "$*" >&2; }

emit_env() {
  local server_bin="$1" bin_dir="$2"
  if [[ -n "${GITHUB_ENV:-}" ]]; then
    echo "POSTHASTE_STALWART_BIN=${server_bin}" >> "$GITHUB_ENV"
  fi
  if [[ -n "${GITHUB_PATH:-}" ]]; then
    echo "${bin_dir}" >> "$GITHUB_PATH"
  fi
  # Always echo sourceable exports for local `eval "$(…)"` use.
  echo "export POSTHASTE_STALWART_BIN=${server_bin}"
  echo "export PATH=${bin_dir}:\$PATH"
}

fetch_and_verify() {
  local asset="$1" sha="$2" out="$3"
  log "downloading ${asset}"
  curl -fsSL --retry 3 --max-time 300 "${BASE_URL}/${asset}" -o "$out"
  echo "${sha}  ${out}" | sha256sum -c - >/dev/null
  log "verified ${asset} (sha256 ok)"
}

runnable() { "$1" --version >/dev/null 2>&1; }

path_fallback() {
  # Reuse an on-PATH stalwart of the EXACT pinned version (nix devshell).
  local sw cli ver
  sw="$(command -v stalwart || true)"
  cli="$(command -v stalwart-cli || true)"
  [[ -n "$sw" && -n "$cli" ]] || {
    log "no on-PATH stalwart/stalwart-cli for fallback"; return 1
  }
  ver="$("$sw" --version 2>/dev/null | tr -d '[:space:]')"
  [[ "$ver" == "$STALWART_VERSION" ]] || {
    log "on-PATH stalwart version '${ver}' != pinned '${STALWART_VERSION}'"; return 1
  }
  log "using on-PATH stalwart ${ver} (dev-desk fallback)"
  emit_env "$sw" "$(dirname "$sw")"
}

main() {
  mkdir -p "$DEST"
  local server_tar="${DEST}/stalwart-${ARCH_TRIPLE}.tar.gz"
  local cli_tar="${DEST}/stalwart-cli-${ARCH_TRIPLE}.tar.gz"

  fetch_and_verify "stalwart-${ARCH_TRIPLE}.tar.gz" "$STALWART_SERVER_SHA256" "$server_tar"
  fetch_and_verify "stalwart-cli-${ARCH_TRIPLE}.tar.gz" "$STALWART_CLI_SHA256" "$cli_tar"

  tar -xzf "$server_tar" -C "$DEST"
  tar -xzf "$cli_tar" -C "$DEST"
  chmod +x "${DEST}/stalwart" "${DEST}/stalwart-cli"

  if runnable "${DEST}/stalwart" && runnable "${DEST}/stalwart-cli"; then
    log "provisioned stalwart ${STALWART_VERSION} at ${DEST}"
    emit_env "${DEST}/stalwart" "${DEST}"
    return 0
  fi

  log "pinned binary cannot exec on this host (loader mismatch?)"
  if [[ "${POSTHASTE_STALWART_ALLOW_PATH_FALLBACK:-}" == "1" ]]; then
    path_fallback && return 0
  fi
  log "ERROR: provisioned stalwart is not runnable and no fallback permitted"
  exit 1
}

main "$@"
