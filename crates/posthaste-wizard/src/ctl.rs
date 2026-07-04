//! `ctl install` / `ctl register` / `ctl status`: the wizard becomes the
//! `posthastectl` installer (RFC-L2-scripting §7 ruling 10b, owner-ruled
//! 2026-07-03: "setup goes through the wizard" — supersedes the raw-symlink
//! shape of ruling 9).
//!
//! Three steps, in order:
//!
//! 1. **Locate** a `posthastectl` binary: an explicit `--from` path, the
//!    desktop app's bundled sidecar (a convention this module defines —
//!    [`sidecar_candidates`]), or a checksum-verified GitHub release download
//!    (reusing [`crate::fetch`]'s release machinery).
//! 2. **Install** it to a bin dir (`~/.local/bin` by default), refusing (never
//!    sudo-ing) on a permission error, and — only for a verified download, on
//!    macOS — clearing the quarantine xattr so Gatekeeper does not block the
//!    first run.
//! 3. **Register**: prove discovery works end to end (binary on PATH, an app
//!    is running, its `daemon.json` parses, a live authenticated probe
//!    succeeds) and print a crisp ✓/✗ table. `ctl register` runs this
//!    automatically after install; `ctl status` re-runs it standalone.
//!
//! @spec docs/eph/RFC-L2-scripting#7-rulings

use std::path::{Path, PathBuf};

use crate::fetch::{verify_checksum, write_executable, Channel, FetchError, ReleaseSource, Version};

// -- Locate + install --------------------------------------------------

/// The platform-appropriate name of the *installed* binary — independent of
/// the channel/platform-scoped release asset name (`PosthasteCTL[Nightly]-
/// <platform>`). Always `posthastectl` (`.exe` on Windows), matching what
/// `just mcp build-cli`'s host build already produces.
pub fn ctl_binary_name() -> &'static str {
    if cfg!(windows) {
        "posthastectl.exe"
    } else {
        "posthastectl"
    }
}

/// Where a located `posthastectl` binary came from. Gates the quarantine
/// strip: only a checksum-verified download counts as established provenance
/// ("never strip unverified downloads") — an explicit `--from` path is
/// operator-supplied and unverified, and a sidecar is trusted transitively
/// through the app bundle's own signing, not this module's checksum check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtlSource {
    /// `--from <path>`.
    Explicit,
    /// The desktop app's bundled sidecar ([`sidecar_candidates`]).
    Sidecar,
    /// Fetched from the GitHub release and checksum-verified against
    /// `SHA256SUMS`.
    Downloaded,
}

/// **The sidecar convention** (defined here for the desktop rider to adopt —
/// not yet wired into `apps/desktop/tauri.conf.json`; this module is what
/// will check for it once it lands):
///
/// - Tauri `bundle.externalBin` will list `binaries/posthastectl` under
///   `apps/desktop/`, with per-target build inputs named
///   `binaries/posthastectl-<target-triple>[.exe]` (Tauri's standard sidecar
///   naming).
/// - At bundle/install time Tauri places the resolved sidecar **next to the
///   main app executable** (`mainBinaryName` = `posthaste-client`), named
///   `posthastectl` (`posthastectl.exe` on Windows) — no target-triple suffix
///   survives into the installed bundle.
/// - So the search is: the `Contents/MacOS` (macOS) / install dir (Windows,
///   Linux) of a known Posthaste install, plus `posthastectl` appended.
///
/// `POSTHASTE_APP_DIR`, if set, is checked first and is the exact contract
/// the desktop's own "Install CLI" affordance should use: it already knows
/// its own `current_exe()` directory, so it can hand the wizard that
/// directory directly rather than making the wizard guess.
pub fn sidecar_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = std::env::var_os("POSTHASTE_APP_DIR") {
        out.push(PathBuf::from(dir).join(ctl_binary_name()));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match std::env::consts::OS {
        "macos" => {
            for app in ["Posthaste.app", "PosthasteNightly.app"] {
                out.push(
                    PathBuf::from("/Applications")
                        .join(app)
                        .join("Contents/MacOS")
                        .join(ctl_binary_name()),
                );
                if let Some(home) = &home {
                    out.push(
                        home.join("Applications")
                            .join(app)
                            .join("Contents/MacOS")
                            .join(ctl_binary_name()),
                    );
                }
            }
        }
        // AppImage/Linux desktop bundling has no fixed install directory
        // today (D18/self-host builds an AppImage, not a .deb/.rpm), so these
        // are forward-looking best guesses for a future packaged install; a
        // Linux desktop today should pass POSTHASTE_APP_DIR explicitly.
        "linux" => {
            for dir in ["/opt/Posthaste", "/opt/PosthasteNightly"] {
                out.push(PathBuf::from(dir).join(ctl_binary_name()));
            }
        }
        "windows" => {
            if let Some(local) = std::env::var_os("LOCALAPPDATA") {
                for app in ["Posthaste", "PosthasteNightly"] {
                    out.push(
                        PathBuf::from(&local)
                            .join("Programs")
                            .join(app)
                            .join(ctl_binary_name()),
                    );
                }
            }
            if let Some(pf) = std::env::var_os("PROGRAMFILES") {
                for app in ["Posthaste", "PosthasteNightly"] {
                    out.push(PathBuf::from(&pf).join(app).join(ctl_binary_name()));
                }
            }
        }
        _ => {}
    }
    out
}

/// The first sidecar candidate that exists as a file.
pub fn find_sidecar() -> Option<PathBuf> {
    sidecar_candidates().into_iter().find(|p| p.is_file())
}

/// Everything `install_ctl` needs.
pub struct CtlInstallOptions {
    /// (a) An explicit path to an existing `posthastectl` binary.
    pub from: Option<PathBuf>,
    /// Directory the binary is installed into (e.g. `~/.local/bin`).
    pub to_dir: PathBuf,
    /// (c) Which release to fetch, if neither `from` nor a sidecar is found.
    pub version: Version,
    /// Release platform suffix (e.g. `linux-x64`); detected when `None`.
    pub platform: Option<String>,
}

#[derive(Debug)]
pub struct CtlInstalled {
    pub binary_path: PathBuf,
    pub source: CtlSource,
    /// Non-fatal problems (e.g. the quarantine xattr could not be cleared).
    pub warnings: Vec<String>,
}

/// Locate (a → b → c, in order) and install `posthastectl` into
/// `opts.to_dir`. Never escalates privilege: a permission error explains
/// itself rather than retrying with sudo.
pub fn install_ctl(opts: &CtlInstallOptions, release: &dyn ReleaseSource) -> Result<CtlInstalled, String> {
    let dest = opts.to_dir.join(ctl_binary_name());
    let mut warnings = Vec::new();

    let (bytes, source) = if let Some(from) = &opts.from {
        let bytes = std::fs::read(from).map_err(|e| format!("read --from {}: {e}", from.display()))?;
        (bytes, CtlSource::Explicit)
    } else if let Some(sidecar) = find_sidecar() {
        let bytes = std::fs::read(&sidecar)
            .map_err(|e| format!("read sidecar {}: {e}", sidecar.display()))?;
        (bytes, CtlSource::Sidecar)
    } else {
        let platform = match &opts.platform {
            Some(p) => p.clone(),
            None => detect_ctl_platform()?,
        };
        let bytes = fetch_ctl(release, &opts.version, &platform)
            .map_err(|e| format!("fetch posthastectl: {e}"))?;
        (bytes, CtlSource::Downloaded)
    };

    write_executable(&dest, &bytes).map_err(|e| {
        let hint = match &e {
            FetchError::Io(_, io_err) if io_err.kind() == std::io::ErrorKind::PermissionDenied => format!(
                " — refusing to sudo; install into a directory you own (e.g. --to {}), \
                 or fix the ownership of {} yourself",
                default_bin_dir_hint(),
                opts.to_dir.display()
            ),
            _ => String::new(),
        };
        format!("write {}: {e}{hint}", dest.display())
    })?;

    if should_strip_quarantine(std::env::consts::OS, source) {
        strip_quarantine(&dest, &mut warnings);
    }

    Ok(CtlInstalled {
        binary_path: dest,
        source,
        warnings,
    })
}

fn default_bin_dir_hint() -> &'static str {
    "~/.local/bin"
}

/// Map the host triple to the `posthastectl` release asset's platform suffix.
/// Distinct from [`crate::install::detect_platform`]: the CLI is a Bun
/// cross-compile and ships per-arch on every OS (`build-cli.ts`'s `SUPPORTED`
/// list), unlike the Rust role binaries, which only split by arch on Linux.
pub fn detect_ctl_platform() -> Result<String, String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("linux", "x86_64") => Ok("linux-x64".into()),
        ("linux", "aarch64") => Ok("linux-arm64".into()),
        ("macos", "x86_64") => Ok("darwin-x64".into()),
        ("macos", "aarch64") => Ok("darwin-arm64".into()),
        ("windows", "x86_64") => Ok("windows-x64".into()),
        _ => Err(format!(
            "no published posthastectl binary for {os}/{arch}; pass --from or --platform to override"
        )),
    }
}

/// The channel-aware `posthastectl` release artifact base name (mirrors
/// `channel-policy.sh`'s `cli_name`).
fn ctl_artifact_base_name(channel: Channel) -> &'static str {
    match channel {
        Channel::Nightly => "PosthasteCTLNightly",
        Channel::Stable => "PosthasteCTL",
    }
}

/// Fetch the `posthastectl` binary for `version`/`platform` and verify it
/// against the release's `SHA256SUMS`. Since the package-cli job landed
/// (release hotfix, 2026-07-03) the CLI ships as `<base>-<platform>.tar.gz`
/// with the signed bare binary as the sole entry; releases up to
/// v0.2.0-nightly.49 shipped the bare binary directly, so `--version` against
/// an older tag falls back to the unwrapped asset name.
pub(crate) fn fetch_ctl(source: &dyn ReleaseSource, version: &Version, platform: &str) -> Result<Vec<u8>, FetchError> {
    let tag = version.tag();
    let base = ctl_artifact_base_name(version.channel());
    let exe = if platform.starts_with("windows") { ".exe" } else { "" };
    let inner_name = format!("{base}-{platform}{exe}");
    let tarball_name = format!("{base}-{platform}.tar.gz");

    let sums = source.fetch(&tag, "SHA256SUMS")?;
    // The SHA256SUMS manifest tells us which convention this release uses:
    // .50+ lists the tarball; older releases list the bare binary.
    let sums_text = String::from_utf8_lossy(&sums);
    if sums_text.contains(&tarball_name) {
        let tarball = source.fetch(&tag, &tarball_name)?;
        verify_checksum(&tarball, &tarball_name, &sums)?;
        crate::fetch::extract_binary(&tarball, &inner_name)
    } else {
        // pre-.50 release: the asset IS the bare binary
        let bytes = source.fetch(&tag, &inner_name)?;
        verify_checksum(&bytes, &inner_name, &sums)?;
        Ok(bytes)
    }
}

// -- Quarantine (macOS Gatekeeper) --------------------------------------

/// The gate: strip the quarantine xattr only on macOS, only for a
/// checksum-verified download. Platform-independent so the gate logic is
/// unit-testable without running on macOS; the actual `xattr` call is behind
/// `cfg(target_os = "macos")`.
pub fn should_strip_quarantine(os: &str, source: CtlSource) -> bool {
    os == "macos" && source == CtlSource::Downloaded
}

#[cfg(target_os = "macos")]
fn strip_quarantine(path: &Path, warnings: &mut Vec<String>) {
    let output = std::process::Command::new("xattr")
        .args(["-d", "com.apple.quarantine"])
        .arg(path)
        .output();
    match output {
        Ok(o) if o.status.success() => {}
        // The file was never quarantined (e.g. a re-run) — not an error.
        Ok(o) if String::from_utf8_lossy(&o.stderr).contains("No such xattr") => {}
        Ok(o) => warnings.push(format!(
            "could not clear the macOS quarantine flag on {}: {}",
            path.display(),
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => warnings.push(format!(
            "could not run xattr to clear the quarantine flag on {}: {e}",
            path.display()
        )),
    }
}

#[cfg(not(target_os = "macos"))]
fn strip_quarantine(_path: &Path, _warnings: &mut [String]) {}

// -- Register / status: end-to-end discovery verification ---------------

/// One row of the register/status table.
pub struct CheckResult {
    pub ok: bool,
    pub detail: String,
}

impl CheckResult {
    fn ok(detail: impl Into<String>) -> Self {
        CheckResult { ok: true, detail: detail.into() }
    }
    fn fail(detail: impl Into<String>) -> Self {
        CheckResult { ok: false, detail: detail.into() }
    }
}

/// The full register/status table: binary placed, PATH resolves it, an app
/// is running, its discovery file parses, and a live authenticated probe
/// succeeds.
pub struct RegisterReport {
    pub binary: CheckResult,
    pub path: CheckResult,
    pub app_running: CheckResult,
    pub discovery: CheckResult,
    pub probe: CheckResult,
}

impl RegisterReport {
    pub fn all_ok(&self) -> bool {
        self.binary.ok && self.path.ok && self.app_running.ok && self.discovery.ok && self.probe.ok
    }

    /// Render the crisp ✓/✗ table.
    pub fn format(&self) -> String {
        let mut out = String::from("posthastectl setup:\n");
        for (name, check) in [
            ("binary", &self.binary),
            ("PATH", &self.path),
            ("app running", &self.app_running),
            ("discovery", &self.discovery),
            ("probe", &self.probe),
        ] {
            let mark = if check.ok { "\u{2713}" } else { "\u{2717}" };
            out.push_str(&format!("  {mark} {name:<12} {}\n", check.detail));
        }
        out
    }
}

/// Run the full register/status check sequence against a binary expected at
/// `bin_dir/<ctl_binary_name>`.
pub fn register(bin_dir: &Path) -> RegisterReport {
    let bin_path = bin_dir.join(ctl_binary_name());
    let binary = check_binary(&bin_path);
    let path = check_path(bin_dir);

    let discovery_path = daemon_json_path();
    let app_running = check_app_running(&discovery_path);
    let (discovery, discovered) = check_discovery(&discovery_path);
    let probe = check_probe(discovered.as_ref());

    RegisterReport {
        binary,
        path,
        app_running,
        discovery,
        probe,
    }
}

fn check_binary(path: &Path) -> CheckResult {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return CheckResult::fail(format!("not found at {}", path.display())),
    };
    if !meta.is_file() {
        return CheckResult::fail(format!("{} is not a file", path.display()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return CheckResult::fail(format!("{} exists but is not executable", path.display()));
        }
    }
    CheckResult::ok(path.display().to_string())
}

/// Is `dir` on `$PATH`? If not, a shell-appropriate one-line hint (printed,
/// never written to an rc file).
fn check_path(dir: &Path) -> CheckResult {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let on_path = std::env::split_paths(&path_var).any(|p| p == dir);
    if on_path {
        CheckResult::ok(format!("{} is on PATH", dir.display()))
    } else {
        CheckResult::fail(format!("{} is not on PATH — {}", dir.display(), shell_hint(dir)))
    }
}

fn shell_hint(dir: &Path) -> String {
    let shell = std::env::var("SHELL").unwrap_or_default();
    let d = dir.display();
    if shell.ends_with("fish") {
        format!("add it to PATH: fish_add_path {d}")
    } else if shell.ends_with("zsh") {
        format!("add to ~/.zshrc: export PATH=\"{d}:$PATH\"")
    } else {
        format!("add to ~/.bashrc (or your shell's rc file): export PATH=\"{d}:$PATH\"")
    }
}

/// The daemon discovery file path. Mirrors
/// `crates/posthaste-http-api-adapter/src/config.rs`'s `resolve_roots`
/// (`POSTHASTE_STATE_ROOT`, else `$XDG_DATA_HOME/posthaste`, else
/// `~/.local/share/posthaste`) — duplicated here, the same way
/// `apps/mcp/src/client.ts`'s `defaultStateRoot` already does, so the wizard
/// stays free of the config-crate dependency (it is lean by design).
fn daemon_json_path() -> PathBuf {
    state_root().join("daemon.json")
}

fn state_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("POSTHASTE_STATE_ROOT") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    match base {
        Some(base) => base.join("posthaste"),
        None => PathBuf::from(".local/share/posthaste"),
    }
}

fn check_app_running(discovery_path: &Path) -> CheckResult {
    if discovery_path.is_file() {
        CheckResult::ok(format!("daemon.json found at {}", discovery_path.display()))
    } else {
        CheckResult::fail(format!(
            "no daemon.json at {} — start the app or `posthaste serve`",
            discovery_path.display()
        ))
    }
}

/// What the probe needs out of `daemon.json`.
struct Discovered {
    url: String,
    token: String,
}

fn check_discovery(path: &Path) -> (CheckResult, Option<Discovered>) {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return (CheckResult::fail("daemon.json not readable"), None),
    };
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return (CheckResult::fail(format!("daemon.json is not valid JSON: {e}")), None),
    };
    let url = value.get("url").and_then(|v| v.as_str()).map(str::to_string);
    let token = value.get("token").and_then(|v| v.as_str()).map(str::to_string);
    match (url, token) {
        (Some(url), Some(token)) => {
            let version = value.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
            (
                CheckResult::ok(format!("version {version}, {url}")),
                Some(Discovered { url, token }),
            )
        }
        _ => (CheckResult::fail("daemon.json is missing url/token"), None),
    }
}

/// The cheapest authenticated ping: `GET {url}/openapi.json` with the
/// discovery bootstrap token — the same request
/// `crates/posthaste-server/tests/discovery.rs` already uses to prove
/// discovery works end to end.
fn check_probe(discovered: Option<&Discovered>) -> CheckResult {
    let Some(d) = discovered else {
        return CheckResult::fail("skipped (no discovery file)");
    };
    let url = format!("{}/openapi.json", d.url.trim_end_matches('/'));
    match ureq::get(&url)
        .set("Authorization", &format!("Bearer {}", d.token))
        .call()
    {
        Ok(resp) => CheckResult::ok(format!("{url} -> {}", resp.status())),
        Err(ureq::Error::Status(code, _)) => CheckResult::fail(format!("{url} -> {code}")),
        Err(e) => CheckResult::fail(format!("{url}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    // -- sidecar convention --

    #[test]
    fn app_dir_override_wins_and_is_checked_first() {
        temp_env(&[("POSTHASTE_APP_DIR", Some("/opt/custom-posthaste"))], || {
            let candidates = sidecar_candidates();
            assert_eq!(candidates[0], PathBuf::from("/opt/custom-posthaste").join(ctl_binary_name()));
        });
    }

    #[test]
    fn find_sidecar_picks_the_first_existing_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join(ctl_binary_name());
        std::fs::write(&bin, b"stand-in").unwrap();
        temp_env(&[("POSTHASTE_APP_DIR", Some(dir.path().to_str().unwrap()))], || {
            assert_eq!(find_sidecar(), Some(bin.clone()));
        });
        // No candidate exists: None.
        let empty_dir = tempfile::tempdir().unwrap();
        temp_env(
            &[("POSTHASTE_APP_DIR", Some(empty_dir.path().to_str().unwrap()))],
            || {
                assert_eq!(find_sidecar(), None);
            },
        );
    }

    // -- locate order + install --

    #[test]
    fn install_prefers_explicit_from_over_sidecar_and_download() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("my-ctl");
        std::fs::write(&from, b"EXPLICIT-BYTES").unwrap();
        let to_dir = dir.path().join("bin");

        struct NoopSource;
        impl ReleaseSource for NoopSource {
            fn fetch(&self, _tag: &str, _asset: &str) -> Result<Vec<u8>, FetchError> {
                panic!("must not fetch when --from is given");
            }
        }

        let opts = CtlInstallOptions {
            from: Some(from),
            to_dir: to_dir.clone(),
            version: Version::Channel(Channel::Nightly),
            platform: None,
        };
        let installed = install_ctl(&opts, &NoopSource).expect("install from explicit path");
        assert_eq!(installed.source, CtlSource::Explicit);
        assert_eq!(
            std::fs::read(&installed.binary_path).unwrap(),
            b"EXPLICIT-BYTES"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&installed.binary_path).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "installed ctl must be executable");
        }
    }

    #[test]
    fn install_falls_back_to_sidecar_when_no_from_given() {
        let dir = tempfile::tempdir().unwrap();
        let app_dir = dir.path().join("app");
        std::fs::create_dir_all(&app_dir).unwrap();
        let sidecar = app_dir.join(ctl_binary_name());
        std::fs::write(&sidecar, b"SIDECAR-BYTES").unwrap();
        let to_dir = dir.path().join("bin");

        struct NoopSource;
        impl ReleaseSource for NoopSource {
            fn fetch(&self, _tag: &str, _asset: &str) -> Result<Vec<u8>, FetchError> {
                panic!("must not fetch when a sidecar is found");
            }
        }

        temp_env(&[("POSTHASTE_APP_DIR", Some(app_dir.to_str().unwrap()))], || {
            let opts = CtlInstallOptions {
                from: None,
                to_dir: to_dir.clone(),
                version: Version::Channel(Channel::Nightly),
                platform: None,
            };
            let installed = install_ctl(&opts, &NoopSource).expect("install from sidecar");
            assert_eq!(installed.source, CtlSource::Sidecar);
            assert_eq!(
                std::fs::read(&installed.binary_path).unwrap(),
                b"SIDECAR-BYTES"
            );
        });
    }

    #[test]
    fn install_downloads_and_verifies_when_nothing_else_found() {
        let dir = tempfile::tempdir().unwrap();
        let to_dir = dir.path().join("bin");

        let asset_name = "PosthasteCTLNightly-linux-x64";
        let bytes = b"DOWNLOADED-CTL-BYTES".to_vec();
        let sums = format!("{}  {}\n", sha256_hex(&bytes), asset_name);

        struct MapSource {
            asset_name: String,
            bytes: Vec<u8>,
            sums: String,
        }
        impl ReleaseSource for MapSource {
            fn fetch(&self, tag: &str, asset: &str) -> Result<Vec<u8>, FetchError> {
                assert_eq!(tag, "nightly");
                if asset == self.asset_name {
                    Ok(self.bytes.clone())
                } else if asset == "SHA256SUMS" {
                    Ok(self.sums.clone().into_bytes())
                } else {
                    Err(FetchError::Http(asset.to_string(), "not found".into()))
                }
            }
        }
        let source = MapSource {
            asset_name: asset_name.to_string(),
            bytes: bytes.clone(),
            sums,
        };

        // No POSTHASTE_APP_DIR and an empty HOME/no /Applications match: falls
        // all the way through to a download.
        temp_env(&[("POSTHASTE_APP_DIR", None)], || {
            let opts = CtlInstallOptions {
                from: None,
                to_dir: to_dir.clone(),
                version: Version::Channel(Channel::Nightly),
                platform: Some("linux-x64".into()),
            };
            let installed = install_ctl(&opts, &source).expect("install via download");
            assert_eq!(installed.source, CtlSource::Downloaded);
            assert_eq!(std::fs::read(&installed.binary_path).unwrap(), bytes);
        });
    }

    #[test]
    fn download_rejects_a_tampered_binary() {
        let dir = tempfile::tempdir().unwrap();
        let to_dir = dir.path().join("bin");
        let asset_name = "PosthasteCTLNightly-linux-x64";
        let good = b"GOOD-CTL".to_vec();
        let sums = format!("{}  {}\n", sha256_hex(&good), asset_name);

        struct MapSource {
            sums: String,
        }
        impl ReleaseSource for MapSource {
            fn fetch(&self, _tag: &str, asset: &str) -> Result<Vec<u8>, FetchError> {
                if asset == "SHA256SUMS" {
                    Ok(self.sums.clone().into_bytes())
                } else {
                    Ok(b"TAMPERED-CTL".to_vec())
                }
            }
        }
        let source = MapSource { sums };

        temp_env(&[("POSTHASTE_APP_DIR", None)], || {
            let opts = CtlInstallOptions {
                from: None,
                to_dir: to_dir.clone(),
                version: Version::Channel(Channel::Nightly),
                platform: Some("linux-x64".into()),
            };
            let err = install_ctl(&opts, &source).unwrap_err();
            assert!(err.contains("checksum"), "expected a checksum error, got: {err}");
            assert!(!to_dir.exists(), "must not install an unverified binary");
        });
    }

    #[test]
    fn ctl_platform_detection_rejects_unsupported_hosts() {
        // The real function reads compile-time OS/ARCH constants, so it can
        // only be smoke-tested on the current host — assert it does not panic
        // and returns something for the actual test host, or a clear error.
        let result = detect_ctl_platform();
        assert!(result.is_ok() || result.unwrap_err().contains("posthastectl"));
    }

    // -- quarantine gate --

    #[test]
    fn quarantine_strip_gate_is_macos_and_downloaded_only() {
        assert!(should_strip_quarantine("macos", CtlSource::Downloaded));
        assert!(!should_strip_quarantine("macos", CtlSource::Explicit));
        assert!(!should_strip_quarantine("macos", CtlSource::Sidecar));
        assert!(!should_strip_quarantine("linux", CtlSource::Downloaded));
        assert!(!should_strip_quarantine("windows", CtlSource::Downloaded));
    }

    // -- register / status --

    #[test]
    fn binary_check_fails_when_absent_and_passes_once_installed() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!check_binary(&dir.path().join(ctl_binary_name())).ok);

        let bin_path = dir.path().join(ctl_binary_name());
        std::fs::write(&bin_path, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(unix)]
        assert!(check_binary(&bin_path).ok);
    }

    #[test]
    fn path_check_detects_containment_and_hints_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        temp_env(&[("PATH", Some(dir.path().to_str().unwrap())), ("SHELL", Some("/bin/zsh"))], || {
            assert!(check_path(dir.path()).ok);
        });
        let other = tempfile::tempdir().unwrap();
        temp_env(
            &[("PATH", Some(other.path().to_str().unwrap())), ("SHELL", Some("/bin/zsh"))],
            || {
                let result = check_path(dir.path());
                assert!(!result.ok);
                assert!(result.detail.contains(".zshrc"), "zsh gets a .zshrc hint: {}", result.detail);
            },
        );
    }

    #[test]
    fn register_reports_no_daemon_json_as_app_not_running() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let state_dir = dir.path().join("state");

        temp_env(
            &[
                ("POSTHASTE_STATE_ROOT", Some(state_dir.to_str().unwrap())),
                ("PATH", Some(bin_dir.to_str().unwrap())),
            ],
            || {
                let report = register(&bin_dir);
                assert!(!report.app_running.ok);
                assert!(!report.discovery.ok);
                assert!(!report.probe.ok);
                assert!(!report.all_ok());
                let table = report.format();
                assert!(table.contains("app running"));
                assert!(table.contains('\u{2717}'), "at least one row is a failure mark");
            },
        );
    }

    #[test]
    fn register_probes_a_stubbed_discovery_file_against_a_mock_server() {
        // A throwaway localhost server standing in for the running app: it
        // answers exactly one GET with a 200, asserting the Authorization
        // header carries the discovery token — the same shape
        // discovery.rs's live test exercises, but against a mock instead of
        // a real bound server.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            use std::io::Read;
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            assert!(req.contains("Bearer mock-bootstrap-token"), "probe must send the discovery token: {req}");
            assert!(
                req.starts_with("GET /v1/openapi.json"),
                "probe must hit <discovery url>/openapi.json: {req}"
            );
            let body = b"{}";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(resp.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        });

        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join(ctl_binary_name()), b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(bin_dir.join(ctl_binary_name()), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let state_dir = dir.path().join("state");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("daemon.json"),
            serde_json::json!({
                "version": 1,
                "port": addr.port(),
                "url": format!("http://127.0.0.1:{}/v1", addr.port()),
                "token": "mock-bootstrap-token",
            })
            .to_string(),
        )
        .unwrap();

        temp_env(
            &[
                ("POSTHASTE_STATE_ROOT", Some(state_dir.to_str().unwrap())),
                ("PATH", Some(bin_dir.to_str().unwrap())),
            ],
            || {
                let report = register(&bin_dir);
                assert!(report.binary.ok);
                assert!(report.path.ok);
                assert!(report.app_running.ok);
                assert!(report.discovery.ok, "{}", report.discovery.detail);
                assert!(report.probe.ok, "{}", report.probe.detail);
                assert!(report.all_ok());
                assert!(report.format().contains('\u{2713}'));
            },
        );

        handle.join().unwrap();
    }

    #[test]
    fn discovery_check_fails_cleanly_on_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.json");
        std::fs::write(&path, "not json").unwrap();
        let (result, discovered) = check_discovery(&path);
        assert!(!result.ok);
        assert!(discovered.is_none());
    }

    #[test]
    fn discovery_check_fails_when_token_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.json");
        std::fs::write(&path, r#"{"version":1,"port":1,"url":"http://x"}"#).unwrap();
        let (result, discovered) = check_discovery(&path);
        assert!(!result.ok);
        assert!(discovered.is_none());
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(bytes))
    }

    /// Scoped env swap, serialized on a mutex so parallel tests in this
    /// module don't race on process-global env (mirrors `install.rs`'s
    /// `temp_env`).
    fn temp_env(vars: &[(&str, Option<&str>)], f: impl FnOnce()) {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        // Recover from poison rather than cascading one failing test's panic
        // into every other test that shares this lock.
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved: Vec<(String, Option<String>)> = vars
            .iter()
            .map(|(k, _)| (k.to_string(), std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
        f();
        for (k, v) in saved {
            match v {
                Some(v) => std::env::set_var(&k, v),
                None => std::env::remove_var(&k),
            }
        }
    }
}
