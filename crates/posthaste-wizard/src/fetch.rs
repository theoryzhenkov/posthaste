//! Fetching and installing a role binary from a GitHub release.
//!
//! The install flow turns "press a button" into: resolve the release tag for the
//! channel, download the role's tarball, verify it against `SHA256SUMS`, extract
//! the binary, and place it on the node. Network + archive handling lives here so
//! [`crate::install`] stays a readable sequence of steps.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::Role;

/// Where releases are fetched from. A trait so tests (and local dev against the
/// tarball the packaging job produced) can swap GitHub for a directory without
/// going near the network.
pub trait ReleaseSource {
    /// Fetch the bytes of `asset` (a release-asset file name) for `tag`.
    fn fetch(&self, tag: &str, asset: &str) -> Result<Vec<u8>, FetchError>;
}

/// GitHub Releases over HTTPS. `latest nightly` is the rolling `nightly` tag;
/// a pinned `vX.Y.Z-nightly.N` resolves directly.
pub struct GithubSource {
    repo: String,
    /// Release host base, e.g. `https://github.com`. Configurable so a
    /// deployment can point at a mirror (and so tests can serve over localhost).
    base_url: String,
}

impl GithubSource {
    pub fn new(repo: impl Into<String>) -> Self {
        Self {
            repo: repo.into(),
            base_url: "https://github.com".into(),
        }
    }

    /// Override the release host base (default `https://github.com`).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }

    /// The default repository the binaries are published to.
    pub fn posthaste() -> Self {
        Self::new("theoryzhenkov/posthaste")
    }
}

impl ReleaseSource for GithubSource {
    fn fetch(&self, tag: &str, asset: &str) -> Result<Vec<u8>, FetchError> {
        let url = format!(
            "{}/{}/releases/download/{tag}/{asset}",
            self.base_url, self.repo
        );
        let resp = ureq::get(&url)
            .call()
            .map_err(|e| FetchError::Http(url.clone(), e.to_string()))?;
        let mut buf = Vec::new();
        resp.into_reader()
            .read_to_end(&mut buf)
            .map_err(|e| FetchError::Http(url, e.to_string()))?;
        Ok(buf)
    }
}

/// The release tag to fetch from: the rolling channel tag, or a pinned version.
pub enum Version {
    /// The latest build on a channel — the rolling `nightly`/`stable` tag.
    Channel(Channel),
    /// A specific tag, e.g. `v0.2.0-nightly.44`.
    Pinned(String),
}

#[derive(Clone, Copy)]
pub enum Channel {
    Nightly,
    Stable,
}

impl Channel {
    /// The rolling release tag that always points at the channel's latest build.
    fn rolling_tag(self) -> &'static str {
        match self {
            Channel::Nightly => "nightly",
            Channel::Stable => "stable",
        }
    }

    /// Infer the channel from a version tag, mirroring `resolve-channel.sh`:
    /// a `-nightly.N` suffix is nightly; a plain or `-rc.N` tag is stable.
    pub fn infer(tag: &str) -> Channel {
        if tag.contains("-nightly.") {
            Channel::Nightly
        } else {
            Channel::Stable
        }
    }
}

impl Version {
    fn tag(&self) -> String {
        match self {
            Version::Channel(c) => c.rolling_tag().to_string(),
            Version::Pinned(tag) => tag.clone(),
        }
    }

    /// The channel this version belongs to, used to build the channel-aware
    /// artifact name (`PosthasteBackendNightly` vs `PosthasteBackend`).
    fn channel(&self) -> Channel {
        match self {
            Version::Channel(c) => *c,
            Version::Pinned(tag) => Channel::infer(tag),
        }
    }
}

/// Fetch the role binary for `version` on `platform`, verify it against the
/// release `SHA256SUMS`, extract it, and write it to `dest` (made executable).
pub fn fetch_and_install(
    source: &dyn ReleaseSource,
    role: Role,
    version: &Version,
    platform: &str,
    dest: &Path,
) -> Result<PathBuf, FetchError> {
    let tag = version.tag();
    let artifact = artifact_base_name(role, version.channel());
    let tarball_name = format!("{artifact}-{platform}.tar.gz");

    let tarball = source.fetch(&tag, &tarball_name)?;
    let sums = source.fetch(&tag, "SHA256SUMS")?;
    verify_checksum(&tarball, &tarball_name, &sums)?;

    let binary = role.binary();
    let bytes = extract_binary(&tarball, binary)?;
    write_executable(dest, &bytes)?;
    Ok(dest.to_path_buf())
}

/// The channel-aware release artifact base name. Mirrors `channel-policy.sh`,
/// which is the source of truth for these names on the publishing side.
fn artifact_base_name(role: Role, channel: Channel) -> String {
    let base = match role {
        Role::Daemon => "PosthasteDaemon",
        Role::Backend => "PosthasteBackend",
        Role::Runtime => "PosthasteRuntimeDaemon",
    };
    match channel {
        Channel::Nightly => format!("{base}Nightly"),
        Channel::Stable => base.to_string(),
    }
}

/// Verify `tarball` against the line for `tarball_name` in a `SHA256SUMS` file
/// (`<hex>  <name>` per line, as produced by `sha256sum`).
fn verify_checksum(tarball: &[u8], tarball_name: &str, sums: &[u8]) -> Result<(), FetchError> {
    let sums = String::from_utf8_lossy(sums);
    let expected = sums
        .lines()
        .find_map(|line| {
            let (hash, name) = line.split_once("  ")?;
            (name.trim() == tarball_name).then(|| hash.trim().to_ascii_lowercase())
        })
        .ok_or_else(|| FetchError::ChecksumMissing(tarball_name.to_string()))?;

    let actual = format!("{:x}", Sha256::digest(tarball));
    if actual != expected {
        return Err(FetchError::ChecksumMismatch {
            asset: tarball_name.to_string(),
            expected,
            actual,
        });
    }
    Ok(())
}

/// Extract `bin/<binary>` (or its `.exe`) from a gzip-compressed tar.
fn extract_binary(tarball: &[u8], binary: &str) -> Result<Vec<u8>, FetchError> {
    let decoder = flate2::read::GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(decoder);
    let wanted_unix = format!("bin/{binary}");
    let wanted_exe = format!("bin/{binary}.exe");

    for entry in archive.entries().map_err(FetchError::Archive)? {
        let mut entry = entry.map_err(FetchError::Archive)?;
        let path = entry.path().map_err(FetchError::Archive)?;
        // The tarball nests under <name>-<platform>/, so match on the trailing
        // bin/<binary> rather than the full prefixed path.
        let matches = path.to_string_lossy().ends_with(&wanted_unix)
            || path.to_string_lossy().ends_with(&wanted_exe);
        if matches {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(FetchError::Archive)?;
            return Ok(bytes);
        }
    }
    Err(FetchError::BinaryMissing(binary.to_string()))
}

/// Write `bytes` to `dest`, creating parent dirs, with the executable bit set on
/// Unix.
fn write_executable(dest: &Path, bytes: &[u8]) -> Result<(), FetchError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| FetchError::Io(parent.to_path_buf(), e))?;
    }
    fs::write(dest, bytes).map_err(|e| FetchError::Io(dest.to_path_buf(), e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(dest)
            .map_err(|e| FetchError::Io(dest.to_path_buf(), e))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(dest, perms).map_err(|e| FetchError::Io(dest.to_path_buf(), e))?;
    }
    Ok(())
}

#[derive(Debug)]
pub enum FetchError {
    Http(String, String),
    ChecksumMissing(String),
    ChecksumMismatch {
        asset: String,
        expected: String,
        actual: String,
    },
    Archive(std::io::Error),
    BinaryMissing(String),
    Io(PathBuf, std::io::Error),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Http(url, e) => write!(f, "failed to fetch {url}: {e}"),
            FetchError::ChecksumMissing(asset) => {
                write!(f, "{asset} not listed in SHA256SUMS")
            }
            FetchError::ChecksumMismatch {
                asset,
                expected,
                actual,
            } => write!(
                f,
                "checksum mismatch for {asset}: expected {expected}, got {actual}"
            ),
            FetchError::Archive(e) => write!(f, "failed to read release archive: {e}"),
            FetchError::BinaryMissing(bin) => {
                write!(f, "{bin} not found in release archive")
            }
            FetchError::Io(path, e) => write!(f, "{}: {e}", path.display()),
        }
    }
}

impl std::error::Error for FetchError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// In-memory release source: maps (tag, asset) -> bytes. Lets the fetch
    /// path be exercised end-to-end with a real tarball + SHA256SUMS, no network.
    struct MapSource(HashMap<(String, String), Vec<u8>>);

    impl ReleaseSource for MapSource {
        fn fetch(&self, tag: &str, asset: &str) -> Result<Vec<u8>, FetchError> {
            self.0
                .get(&(tag.to_string(), asset.to_string()))
                .cloned()
                .ok_or_else(|| FetchError::Http(format!("{tag}/{asset}"), "not found".into()))
        }
    }

    /// Build a gzip tarball with `bin/<binary>` holding `contents`, matching the
    /// layout `tools/package/bin.sh` produces.
    fn make_tarball(name: &str, binary: &str, contents: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut tar = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, format!("{name}/bin/{binary}"), contents)
            .unwrap();
        let tar_bytes = tar.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&tar_bytes).unwrap();
        gz.finish().unwrap()
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn source_with(role: Role, channel: Channel, tag: &str, contents: &[u8]) -> MapSource {
        let artifact = artifact_base_name(role, channel);
        let tarball_name = format!("{artifact}-linux-x86_64.tar.gz");
        let tarball = make_tarball(&format!("{artifact}-linux-x86_64"), role.binary(), contents);
        let sums = format!("{}  {}\n", sha256_hex(&tarball), tarball_name);
        let mut map = HashMap::new();
        map.insert((tag.to_string(), tarball_name), tarball);
        map.insert(
            (tag.to_string(), "SHA256SUMS".to_string()),
            sums.into_bytes(),
        );
        MapSource(map)
    }

    #[test]
    fn installs_a_verified_binary() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("posthaste_backend");
        let source = source_with(Role::Backend, Channel::Nightly, "nightly", b"BACKEND-BYTES");

        let out = fetch_and_install(
            &source,
            Role::Backend,
            &Version::Channel(Channel::Nightly),
            "linux-x86_64",
            &dest,
        )
        .unwrap();

        assert_eq!(out, dest);
        assert_eq!(fs::read(&dest).unwrap(), b"BACKEND-BYTES");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&dest).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "binary must be executable");
        }
    }

    #[test]
    fn rejects_a_tampered_tarball() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("posthaste_backend");
        // Build a source whose SHA256SUMS is correct, then corrupt the tarball
        // bytes so the recorded hash no longer matches.
        let artifact = artifact_base_name(Role::Backend, Channel::Nightly);
        let tarball_name = format!("{artifact}-linux-x86_64.tar.gz");
        let good = make_tarball(
            &format!("{artifact}-linux-x86_64"),
            "posthaste_backend",
            b"GOOD",
        );
        let sums = format!("{}  {}\n", sha256_hex(&good), tarball_name);
        let mut map = HashMap::new();
        let mut tampered = good;
        *tampered.last_mut().unwrap() ^= 0xff;
        map.insert(("nightly".into(), tarball_name), tampered);
        map.insert(("nightly".into(), "SHA256SUMS".into()), sums.into_bytes());

        let err = fetch_and_install(
            &MapSource(map),
            Role::Backend,
            &Version::Channel(Channel::Nightly),
            "linux-x86_64",
            &dest,
        )
        .unwrap_err();

        assert!(matches!(err, FetchError::ChecksumMismatch { .. }));
        assert!(
            !dest.exists(),
            "must not write a binary that fails verification"
        );
    }

    #[test]
    fn pinned_tag_infers_channel_for_artifact_name() {
        // A pinned nightly tag must resolve the *Nightly* artifact name, not the
        // stable one, or the asset won't be found.
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("posthaste_runtime_daemon");
        let source = source_with(Role::Runtime, Channel::Nightly, "v0.2.0-nightly.44", b"RT");

        let out = fetch_and_install(
            &source,
            Role::Runtime,
            &Version::Pinned("v0.2.0-nightly.44".into()),
            "linux-x86_64",
            &dest,
        );
        assert!(
            out.is_ok(),
            "pinned nightly tag should map to *Nightly artifact: {out:?}"
        );
    }
}
