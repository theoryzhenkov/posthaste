#!/usr/bin/env bash
# Package a single standalone binary into a release archive.
#
# Generic companion to daemon.sh (which carries daemon-specific README copy and
# the POSTHASTE_DAEMON_NAME env). Used for the distributed self-host binaries:
# posthaste_backend, posthaste_runtime_daemon, and the posthaste-wizard
# installer. Produces the same target/distribute/<name>-<platform>/ layout the
# publish job expects, so its output is picked up by the SHA256SUMS + signing
# steps without any per-binary wiring there.
#
# Requires: the binary already built via `cargo build --release --bin <bin>`.
# Env:
#   POSTHASTE_BIN              - cargo binary name, e.g. posthaste_backend
#   POSTHASTE_ARTIFACT_NAME    - release artifact base name, e.g. PosthasteBackendNightly
#   POSTHASTE_PACKAGE_PLATFORM - e.g. linux-x86_64 (defaults to host)
#   POSTHASTE_BIN_LABEL       - human label for the README (defaults to artifact name)
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

bin_name="${POSTHASTE_BIN:?POSTHASTE_BIN is required (cargo bin name, e.g. posthaste_backend)}"
artifact_name="${POSTHASTE_ARTIFACT_NAME:-$bin_name}"
label="${POSTHASTE_BIN_LABEL:-$artifact_name}"
platform="${POSTHASTE_PACKAGE_PLATFORM:-$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)}"
name="${artifact_name}-${platform}"
out_root="$root/target/distribute"
out_dir="$out_root/$name"

# Windows cross-builds produce a .exe; detect and preserve the extension so
# the install + tar steps reference the real file.
binary="$bin_name"
if [[ -f "$root/target/release/$bin_name.exe" ]]; then
  binary="$bin_name.exe"
fi

if [[ ! -x "$root/target/release/$binary" ]]; then
  echo "missing target/release/$binary; run 'cargo build --release --bin $bin_name' first" >&2
  exit 1
fi

rm -rf "$out_dir"
mkdir -p "$out_dir/bin"
install -m 0755 "$root/target/release/$binary" "$out_dir/bin/$binary"

cat > "$out_dir/README.md" <<EOF
# $label

Standalone binary for the Posthaste distributed self-host topology.

Run:

\`\`\`sh
./bin/$binary
\`\`\`
EOF

tar -C "$out_root" -czf "$out_root/$name.tar.gz" "$name"
echo "$out_root/$name.tar.gz"
