#!/bin/sh
# Posthaste install-wizard bootstrap.
#
# Fetches the one-shot `posthaste-wizard` binary for this platform from the
# GitHub release, verifies it against SHA256SUMS, and drops it in ~/.local/bin.
# This is the only rung that must be obtained by hand; the wizard itself then
# fetches the (large, role-specific) node binaries in-process.
#
#   curl -fsSL https://posthaste.theor.net/install.sh | sh
#
# Then, on each machine:
#   posthaste-wizard install --role <daemon|authority server|runtime> ...
#
# Environment overrides:
#   POSTHASTE_CHANNEL        nightly (default) | stable
#   POSTHASTE_VERSION        pin an exact tag, e.g. v0.2.0-nightly.45
#                            (skips the "latest on channel" lookup)
#   POSTHASTE_BIN_DIR        install dir (default: ~/.local/bin)
#   POSTHASTE_PLATFORM       override platform detection (linux-x86_64|macos|windows-x86_64)
#   POSTHASTE_REPO           owner/name (default: theoryzhenkov/posthaste)
#   POSTHASTE_DOWNLOAD_BASE  release-download base (default: GitHub releases)
#   POSTHASTE_API_URL        GitHub API base (default: https://api.github.com)
set -eu

REPO="${POSTHASTE_REPO:-theoryzhenkov/posthaste}"
CHANNEL="${POSTHASTE_CHANNEL:-nightly}"
VERSION="${POSTHASTE_VERSION:-}"
BIN_DIR="${POSTHASTE_BIN_DIR:-${HOME}/.local/bin}"
DOWNLOAD_BASE="${POSTHASTE_DOWNLOAD_BASE:-https://github.com/${REPO}/releases/download}"
API_URL="${POSTHASTE_API_URL:-https://api.github.com}"

die() {
	echo "install.sh: $*" >&2
	exit 1
}

# --- HTTP: prefer curl, fall back to wget. Both support file:// for testing. ---
if command -v curl >/dev/null 2>&1; then
	fetch() { curl -fsSL "$1"; }
	fetch_to() { curl -fsSL -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
	fetch() { wget -qO- "$1"; }
	fetch_to() { wget -qO "$2" "$1"; }
else
	die "need curl or wget to download the wizard"
fi

# --- sha256: sha256sum (Linux) or shasum -a 256 (macOS). ---
if command -v sha256sum >/dev/null 2>&1; then
	sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
	sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
	die "need sha256sum or shasum to verify the download"
fi

# --- 1. detect platform (mirrors the wizard's detect_platform + release matrix). ---
platform="${POSTHASTE_PLATFORM:-}"
if [ -z "$platform" ]; then
	os="$(uname -s)"
	arch="$(uname -m)"
	case "$os" in
	Linux)
		case "$arch" in
		x86_64 | amd64) platform="linux-x86_64" ;;
		*) die "no published wizard for Linux/$arch; build from source or set POSTHASTE_PLATFORM" ;;
		esac
		;;
	Darwin) platform="macos" ;;
	*) die "unsupported OS '$os'; on Windows download the tarball from the releases page" ;;
	esac
fi

# --- 2. channel-aware artifact name (mirrors channel-policy.sh). ---
case "$CHANNEL" in
nightly) artifact="PosthasteWizardNightly" ;;
stable) artifact="PosthasteWizard" ;;
*) die "unknown channel '$CHANNEL' (expected: nightly|stable)" ;;
esac

# --- 3. resolve the version tag (latest on channel) unless pinned. ---
if [ -z "$VERSION" ]; then
	if [ "$CHANNEL" = "stable" ]; then
		# The newest non-prerelease is GitHub's "latest".
		VERSION="$(fetch "${API_URL}/repos/${REPO}/releases/latest" |
			grep '"tag_name"' | head -1 |
			sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')"
	else
		# Releases come back newest-first; take the first versioned nightly tag
		# (the rolling "nightly" tag carries no '-nightly.N' suffix, so it is
		# skipped).
		VERSION="$(fetch "${API_URL}/repos/${REPO}/releases?per_page=30" |
			grep '"tag_name"' |
			sed -E 's/.*"tag_name": *"([^"]+)".*/\1/' |
			grep -- '-nightly\.' | head -1)"
	fi
	[ -n "$VERSION" ] || die "could not resolve the latest $CHANNEL release; pin one with POSTHASTE_VERSION"
fi

tarball="${artifact}-${platform}.tar.gz"
base="${DOWNLOAD_BASE}/${VERSION}"
echo "install.sh: fetching ${tarball} from ${VERSION}"

# --- 4. download tarball + checksums into a temp dir. ---
tmp="$(mktemp -d "${TMPDIR:-/tmp}/posthaste-wizard.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT INT TERM

fetch_to "${base}/${tarball}" "${tmp}/${tarball}" ||
	die "download failed: ${base}/${tarball}"

# --- 5. verify against SHA256SUMS (refuse to install an unverified binary). ---
if fetch "${base}/SHA256SUMS" >"${tmp}/SHA256SUMS" 2>/dev/null && [ -s "${tmp}/SHA256SUMS" ]; then
	expected="$(grep " ${tarball}\$" "${tmp}/SHA256SUMS" | head -1 | cut -d' ' -f1)"
	[ -n "$expected" ] || die "${tarball} not listed in SHA256SUMS"
	actual="$(sha256 "${tmp}/${tarball}")"
	[ "$expected" = "$actual" ] ||
		die "checksum mismatch for ${tarball}: expected ${expected}, got ${actual}"
	echo "install.sh: checksum verified"
else
	die "could not fetch SHA256SUMS for ${VERSION}; refusing to install unverified"
fi

# --- 6. extract bin/posthaste-wizard and install it. ---
tar -xzf "${tmp}/${tarball}" -C "$tmp"
binary="$(find "$tmp" -type f -name 'posthaste-wizard' -path '*/bin/*' | head -1)"
[ -n "$binary" ] || binary="$(find "$tmp" -type f -name 'posthaste-wizard.exe' -path '*/bin/*' | head -1)"
[ -n "$binary" ] || die "posthaste-wizard not found inside ${tarball}"

mkdir -p "$BIN_DIR"
dest="${BIN_DIR}/$(basename "$binary")"
install -m 0755 "$binary" "$dest" 2>/dev/null || {
	cp "$binary" "$dest"
	chmod 0755 "$dest"
}

echo "install.sh: installed $dest"

# --- 7. next steps + PATH hint. ---
case ":${PATH}:" in
*":${BIN_DIR}:"*) ;;
*) echo "install.sh: note — ${BIN_DIR} is not on your PATH; add it or run ${dest} directly" ;;
esac

cat <<EOF

The wizard is one-shot. Provision a node:

  posthaste-wizard install --role authority server --tls --host <hostname> \\
    --bind 0.0.0.0:3002 --link-token <secret> \\
    --config-root ~/.config/posthaste --state-root ~/.local/share/posthaste

It prints a one-line join string; run the wizard on the second machine with
--join <string> to wire it up. See \`posthaste-wizard --help\`.
EOF
