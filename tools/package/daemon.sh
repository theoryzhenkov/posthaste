#!/usr/bin/env bash
# Package the standalone daemon binary (posthasted) into a release archive.
# The web frontend is packaged separately by web.sh (split topology).
#
# Requires: cargo build --release --bin posthasted already run.
# Env:
#   POSTHASTE_DAEMON_NAME      - release artifact base name (from channel-policy.sh)
#   POSTHASTE_PACKAGE_PLATFORM - e.g. linux-x86_64 (defaults to host)
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

platform="${POSTHASTE_PACKAGE_PLATFORM:-$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)}"
name="${POSTHASTE_DAEMON_NAME:-posthaste-daemon}-${platform}"
out_root="$root/target/distribute"
out_dir="$out_root/$name"

binary_name="posthasted"
if [[ -f "$root/target/release/posthasted.exe" ]]; then
  binary_name="posthasted.exe"
fi

if [[ ! -x "$root/target/release/$binary_name" ]]; then
  echo "missing target/release/$binary_name; run 'cargo build --release --bin posthasted' first" >&2
  exit 1
fi

rm -rf "$out_dir"
mkdir -p "$out_dir/bin"
install -m 0755 "$root/target/release/$binary_name" "$out_dir/bin/$binary_name"

cat > "$out_dir/README.md" <<EOF
# ${POSTHASTE_DAEMON_NAME:-Posthaste daemon}

Standalone daemon binary (no desktop UI). Run:

\`\`\`sh
./bin/${binary_name} serve --api-only
\`\`\`

For browser-localhost mode, download the PosthasteWeb bundle separately and
pass its path via POSTHASTE_FRONTEND_DIST.
EOF

tar -C "$out_root" -czf "$out_root/$name.tar.gz" "$name"
echo "$out_root/$name.tar.gz"
