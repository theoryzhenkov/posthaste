#!/usr/bin/env bash
# Build a local browser-mode distribution archive from already-built artifacts.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

version="$(awk -F '"' '/^version = / { print $2; exit }' crates/posthaste-server/Cargo.toml)"
platform="${POSTHASTE_PACKAGE_PLATFORM:-$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)}"
name="posthaste-serve-$version-$platform"
out_root="$root/target/distribute"
out_dir="$out_root/$name"
binary_name="posthaste"
if [[ -f "$root/target/release/posthaste.exe" ]]; then
  binary_name="posthaste.exe"
fi

if [[ ! -x "$root/target/release/$binary_name" ]]; then
  echo "missing target/release/$binary_name; run 'just build-serve' first" >&2
  exit 1
fi

if [[ ! -f "$root/apps/web/dist/index.html" ]]; then
  echo "missing apps/web/dist/index.html; run 'just build-serve' first" >&2
  exit 1
fi

rm -rf "$out_dir"
mkdir -p "$out_dir/bin" "$out_dir/share/posthaste"
install -m 0755 "$root/target/release/$binary_name" "$out_dir/bin/$binary_name"
cp -R "$root/apps/web/dist" "$out_dir/share/posthaste/web"

cat > "$out_dir/README.md" <<EOF
# PostHaste browser-localhost distribution

Run:

\`\`\`sh
POSTHASTE_FRONTEND_DIST="\$(pwd)/share/posthaste/web" ./bin/$binary_name serve --open
\`\`\`
EOF

tar -C "$out_root" -czf "$out_root/$name.tar.gz" "$name"
echo "$out_root/$name.tar.gz"
