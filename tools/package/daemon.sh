#!/usr/bin/env bash
# Package the standalone daemon binary (posthaste-authority-runtime-server) into a release archive.
# The web frontend is packaged separately by web.sh (split topology).
#
# Requires: cargo build --release --bin posthaste-authority-runtime-server already run.
# Env:
#   POSTHASTE_AUTHORITY_RUNTIME_SERVER_NAME      - release artifact base name (from channel-policy.sh)
#   POSTHASTE_PACKAGE_PLATFORM - e.g. linux-x86_64 (defaults to host)
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

platform="${POSTHASTE_PACKAGE_PLATFORM:-$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)}"
name="${POSTHASTE_AUTHORITY_RUNTIME_SERVER_NAME:-posthaste-daemon}-${platform}"
out_root="$root/target/distribute"
out_dir="$out_root/$name"

binary_name="posthaste-authority-runtime-server"
if [[ -f "$root/target/release/posthaste-authority-runtime-server.exe" ]]; then
  binary_name="posthaste-authority-runtime-server.exe"
fi

if [[ ! -x "$root/target/release/$binary_name" ]]; then
  echo "missing target/release/$binary_name; run 'cargo build --release --bin posthaste-authority-runtime-server' first" >&2
  exit 1
fi

rm -rf "$out_dir"
mkdir -p "$out_dir/bin"
install -m 0755 "$root/target/release/$binary_name" "$out_dir/bin/$binary_name"

cat > "$out_dir/README.md" <<EOF
# ${POSTHASTE_AUTHORITY_RUNTIME_SERVER_NAME:-Posthaste daemon}

Standalone daemon binary (no desktop UI). Run:

\`\`\`sh
./bin/${binary_name} serve --api-only
\`\`\`

For browser-localhost mode, download the PosthasteWeb bundle separately and
pass its path via POSTHASTE_FRONTEND_DIST.
EOF

tar -C "$out_root" -czf "$out_root/$name.tar.gz" "$name"
echo "$out_root/$name.tar.gz"
