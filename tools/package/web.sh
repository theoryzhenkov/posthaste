#!/usr/bin/env bash
# Package the browser-localhost web frontend into a platform-agnostic release
# archive. Split from the daemon (the frontend is co-deployed but distributed
# separately so the daemon artifact stays small + platform-scoped).
#
# Requires: bun --cwd=legacy/web run build already run (legacy/web/dist populated).
# Env:
#   POSTHASTE_WEB_NAME - release artifact base name (from channel-policy.sh)
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

name="${POSTHASTE_WEB_NAME:-posthaste-web}"
out_root="$root/target/distribute"
out_dir="$out_root/$name"

if [[ ! -f "$root/legacy/web/dist/index.html" ]]; then
  echo "missing legacy/web/dist/index.html; run 'bun --cwd=legacy/web run build' first" >&2
  exit 1
fi

rm -rf "$out_dir"
mkdir -p "$out_dir"
cp -R "$root/legacy/web/dist" "$out_dir/web"

cat > "$out_dir/README.md" <<EOF
# ${POSTHASTE_WEB_NAME:-Posthaste web frontend}

Browser-localhost frontend for split/self-hosted topologies. Serve it with the
Posthaste daemon:

\`\`\`sh
POSTHASTE_FRONTEND_DIST="/path/to/web" posthaste-authority-runtime-server serve
\`\`\`
EOF

tar -C "$out_root" -czf "$out_root/$name.tar.gz" "$name"
echo "$out_root/$name.tar.gz"
