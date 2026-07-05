//! `posthaste-wizard update` — the headless/self-host updater (RFC-L2-scripting
//! ruling 14). Desktop machines update through the app's Tauri updater; a
//! headless node has no such loop, so the wizard — the only actor that owns the
//! service units — updates the role binaries, `posthastectl`, and itself.
//!
//! The flow, per component recorded in the [`crate::manifest`]:
//!
//! 1. **Resolve** the channel's current version (the rolling release's updater
//!    manifest, [`crate::fetch::resolve_latest_version`]) and diff it against
//!    the installed version — printed as a table (`--check` stops here).
//! 2. **Swap** (with `--yes`, or an interactive confirm): download + verify the
//!    new binary ([`crate::fetch`]'s checksum machinery), stop the service if
//!    the wizard owns a unit for it, atomically rename the current binary aside
//!    to `<path>.bak`, move the new one into place, restart, record the new
//!    version. `--rollback <component>` swaps the `.bak` back.
//! 3. **Self-update last**: the wizard updates its own binary in the same pass.
//!    On unix, `rename(2)` over the running executable's path succeeds (the
//!    running process keeps its open inode; the next launch is the new binary)
//!    — so self-update is the identical atomic swap, no special-casing, and the
//!    crate is unix-first by its service units. On a Windows-style locked-file
//!    OS this would fail; not a supported update host.
//!
//! `--install-timer` renders an opt-in user timer that runs `update --yes` on a
//! schedule (never a daemon-resident self-updater — ruling 14).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::fetch::{self, Channel, ReleaseSource, Version};
use crate::install::{detect_platform, start_service, stop_service, user_unit_dir, ServiceScope};
use crate::manifest::{now_rfc3339, Component, Manifest};
use crate::Role;

// -- Check: resolve channel-latest and diff -----------------------------

/// Where an installed component sits relative to its channel's latest build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    /// Installed version equals the channel's current version.
    UpToDate,
    /// A newer (or simply different) version is published on the channel.
    UpdateAvailable,
    /// The channel's latest could not be resolved (offline, missing manifest).
    Unknown(String),
}

/// One row of the `update`/`update --check` table.
pub struct ComponentStatus {
    pub component: String,
    pub kind: String,
    pub channel: String,
    pub installed: String,
    /// The channel's current version, or `?` when [`UpdateState::Unknown`].
    pub latest: String,
    pub state: UpdateState,
}

impl ComponentStatus {
    /// Does this row call for a swap?
    pub fn actionable(&self) -> bool {
        self.state == UpdateState::UpdateAvailable
    }
}

/// Diff every recorded component against its channel's current version. Resolves
/// each channel's latest at most once (cached), so a mixed nightly/stable host
/// makes one manifest fetch per channel, not per component.
pub fn plan_updates(manifest: &Manifest, source: &dyn ReleaseSource) -> Vec<ComponentStatus> {
    let mut cache: Vec<(String, Result<String, String>)> = Vec::new();
    let mut out = Vec::new();
    for c in &manifest.components {
        let latest = match Channel::parse(&c.channel) {
            Some(channel) => resolve_cached(&mut cache, &c.channel, channel, source),
            None => Err(format!("unknown channel `{}`", c.channel)),
        };
        let (latest_str, state) = match latest {
            Ok(latest) if latest == c.version => (latest, UpdateState::UpToDate),
            Ok(latest) => (latest, UpdateState::UpdateAvailable),
            Err(why) => ("?".to_string(), UpdateState::Unknown(why)),
        };
        out.push(ComponentStatus {
            component: c.component.clone(),
            kind: c.kind.clone(),
            channel: c.channel.clone(),
            installed: c.version.clone(),
            latest: latest_str,
            state,
        });
    }
    out
}

fn resolve_cached(
    cache: &mut Vec<(String, Result<String, String>)>,
    key: &str,
    channel: Channel,
    source: &dyn ReleaseSource,
) -> Result<String, String> {
    if let Some((_, v)) = cache.iter().find(|(k, _)| k == key) {
        return v.clone();
    }
    let resolved = fetch::resolve_latest_version(source, channel).map_err(|e| e.to_string());
    cache.push((key.to_string(), resolved.clone()));
    resolved
}

/// Render the ✓/↑/? status table.
pub fn format_status_table(statuses: &[ComponentStatus]) -> String {
    if statuses.is_empty() {
        return "no components recorded in the wizard manifest — nothing to update.\n\
                (install a node with `posthaste-wizard install` or the CLI with \
                `posthaste-wizard ctl install` first.)\n"
            .to_string();
    }
    let name_w = statuses
        .iter()
        .map(|s| s.component.len())
        .max()
        .unwrap_or(9)
        .max(9);
    let mut out = String::from("component updates:\n");
    for s in statuses {
        let (mark, note) = match &s.state {
            UpdateState::UpToDate => ("\u{2713}", "up to date".to_string()),
            UpdateState::UpdateAvailable => {
                ("\u{2191}", format!("{} \u{2192} {}", s.installed, s.latest))
            }
            UpdateState::Unknown(why) => ("?", format!("latest unknown ({why})")),
        };
        out.push_str(&format!(
            "  {mark} {name:<width$} [{channel}] {note}\n",
            name = s.component,
            width = name_w,
            channel = s.channel,
        ));
    }
    out
}

// -- Atomic swap / rollback (pure, unit-testable) -----------------------

/// A sibling path with `suffix` appended to the file name (`foo` + `.bak` ->
/// `foo.bak`). Unlike `Path::with_extension`, it never replaces an existing
/// extension — it appends, so `posthastectl` and `foo.exe` both round-trip.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// The `.bak` path a swap keeps the previous binary at.
pub fn bak_path(path: &Path) -> PathBuf {
    sibling(path, ".bak")
}

/// Atomically move `new_path` into `path`, first preserving the current binary
/// (if any) as `<path>.bak`. Both renames stay within the destination
/// directory, so on a single filesystem they are atomic. Returns the `.bak`
/// path kept (`None` if there was no prior binary). This is also exactly the
/// self-update mechanic — on unix, renaming over a running executable's path
/// succeeds, so the wizard swaps its own binary through this same function.
pub fn atomic_swap(path: &Path, new_path: &Path) -> Result<Option<PathBuf>, String> {
    let bak = if path.exists() {
        let bak = bak_path(path);
        std::fs::rename(path, &bak)
            .map_err(|e| format!("back up {} to {}: {e}", path.display(), bak.display()))?;
        Some(bak)
    } else {
        None
    };
    std::fs::rename(new_path, path)
        .map_err(|e| format!("move {} into {}: {e}", new_path.display(), path.display()))?;
    Ok(bak)
}

/// Restore `<path>.bak` over `path` (the inverse of the swap). Errors if no
/// `.bak` is present (nothing to roll back to).
pub fn rollback(path: &Path) -> Result<(), String> {
    let bak = bak_path(path);
    if !bak.exists() {
        return Err(format!(
            "no backup at {} — nothing to roll back to (was this component updated?)",
            bak.display()
        ));
    }
    std::fs::rename(&bak, path)
        .map_err(|e| format!("restore {} from {}: {e}", path.display(), bak.display()))
}

// -- Apply an update to one component -----------------------------------

/// What an applied swap did, for reporting.
pub struct ApplyOutcome {
    pub component: String,
    pub from: String,
    pub to: String,
    pub warnings: Vec<String>,
}

/// The channel-aware wizard release artifact base (mirrors
/// `channel-policy.sh`'s `wizard_name`).
pub(crate) fn wizard_artifact_base(channel: Channel) -> &'static str {
    match channel {
        Channel::Nightly => "PosthasteWizardNightly",
        Channel::Stable => "PosthasteWizard",
    }
}

/// Download + verify the new bytes for `entry` from its channel's rolling
/// release. Dispatches on the component kind to the right artifact naming.
fn download_component(
    entry: &Component,
    source: &dyn ReleaseSource,
    platform_override: Option<&str>,
) -> Result<Vec<u8>, String> {
    let channel = Channel::parse(&entry.channel)
        .ok_or_else(|| format!("unknown channel `{}`", entry.channel))?;
    let tag = channel.rolling_tag();
    match entry.kind.as_str() {
        "role" => {
            let role = Role::from_binary(&entry.component)
                .ok_or_else(|| format!("`{}` is not a known role binary", entry.component))?;
            let platform = platform_from(platform_override, detect_platform)?;
            let base = fetch::artifact_base_name(role, channel);
            fetch::download_verified_tarball_binary(source, &base, role.binary(), &platform, tag)
                .map_err(|e| e.to_string())
        }
        "wizard" => {
            let platform = platform_from(platform_override, detect_platform)?;
            let base = wizard_artifact_base(channel);
            fetch::download_verified_tarball_binary(
                source,
                base,
                "posthaste-wizard",
                &platform,
                tag,
            )
            .map_err(|e| e.to_string())
        }
        "ctl" => {
            // The CLI ships per-arch on every OS and may be tarball- or
            // bare-binary-packaged depending on the release; `fetch_ctl` picks
            // the right shape off SHA256SUMS.
            let platform = match platform_override {
                Some(p) => p.to_string(),
                None => crate::ctl::detect_ctl_platform()?,
            };
            crate::ctl::fetch_ctl(source, &Version::Channel(channel), &platform)
                .map_err(|e| e.to_string())
        }
        other => Err(format!(
            "component `{}` has unknown kind `{other}`",
            entry.component
        )),
    }
}

fn platform_from(
    over: Option<&str>,
    detect: fn() -> Result<String, String>,
) -> Result<String, String> {
    match over {
        Some(p) => Ok(p.to_string()),
        None => detect(),
    }
}

/// Perform the download → (stop) → swap → (start) sequence for one component.
/// The caller updates + persists the manifest afterward (this function does no
/// manifest IO, keeping the swap mechanics testable in isolation).
pub fn apply_update(
    entry: &Component,
    latest: &str,
    source: &dyn ReleaseSource,
    platform_override: Option<&str>,
) -> Result<ApplyOutcome, String> {
    let mut warnings = Vec::new();
    let bytes = download_component(entry, source, platform_override)?;

    let path = PathBuf::from(&entry.path);
    let new_path = sibling(&path, ".new");
    fetch_write_executable(&new_path, &bytes)?;

    // Stop the service around the swap so a running process is not swapped from
    // under itself. Best-effort — a stop failure (unit not loaded) is a warning.
    let service = entry.service.as_deref().map(ServiceScope::parse);
    if let (Some(scope), Some(unit)) = (service, entry.unit.as_deref()) {
        if scope != ServiceScope::None {
            if let Err(e) = stop_service(scope, unit) {
                warnings.push(format!("could not stop {unit} before swap: {e}"));
            }
        }
    }

    let swap = atomic_swap(&path, &new_path);
    // Whatever happens, restart the service so a failed swap does not leave the
    // node down.
    let restart = |warnings: &mut Vec<String>| {
        if let (Some(scope), Some(unit)) = (service, entry.unit.as_deref()) {
            if scope != ServiceScope::None {
                if let Err(e) = start_service(scope, unit) {
                    warnings.push(format!("could not restart {unit} after swap: {e}"));
                }
            }
        }
    };
    if let Err(e) = swap {
        let _ = std::fs::remove_file(&new_path);
        restart(&mut warnings);
        return Err(e);
    }
    restart(&mut warnings);

    Ok(ApplyOutcome {
        component: entry.component.clone(),
        from: entry.version.clone(),
        to: latest.to_string(),
        warnings,
    })
}

/// Write `bytes` to `dest` with the executable bit set (unix), creating parent
/// dirs. A thin wrapper over the fetch module's helper so `update` writes the
/// staged `.new` binary exactly as an install writes a fresh one.
fn fetch_write_executable(dest: &Path, bytes: &[u8]) -> Result<(), String> {
    crate::fetch::write_executable(dest, bytes).map_err(|e| e.to_string())
}

/// Roll a component back to its `.bak`, driving the service around the swap the
/// same way [`apply_update`] does. Returns any best-effort warnings.
pub fn rollback_component(entry: &Component) -> Result<Vec<String>, String> {
    let mut warnings = Vec::new();
    let path = PathBuf::from(&entry.path);
    let service = entry.service.as_deref().map(ServiceScope::parse);
    if let (Some(scope), Some(unit)) = (service, entry.unit.as_deref()) {
        if scope != ServiceScope::None {
            if let Err(e) = stop_service(scope, unit) {
                warnings.push(format!("could not stop {unit} before rollback: {e}"));
            }
        }
    }
    let result = rollback(&path);
    if let (Some(scope), Some(unit)) = (service, entry.unit.as_deref()) {
        if scope != ServiceScope::None {
            if let Err(e) = start_service(scope, unit) {
                warnings.push(format!("could not restart {unit} after rollback: {e}"));
            }
        }
    }
    result?;
    Ok(warnings)
}

// -- Auto-update timer rider (--install-timer) --------------------------

/// The systemd `.service` + `.timer` pair (unit-file bodies) that run
/// `posthaste-wizard update --yes` on a daily schedule.
pub fn render_update_timer_systemd(wizard_exe: &Path) -> (String, String) {
    let exec = wizard_exe.display();
    let service = format!(
        "[Unit]\n\
         Description=Posthaste wizard auto-update (headless self-host)\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={exec} update --yes\n"
    );
    let timer = "[Unit]\n\
         Description=Run the Posthaste wizard auto-update daily\n\
         \n\
         [Timer]\n\
         OnCalendar=daily\n\
         Persistent=true\n\
         RandomizedDelaySec=1h\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n"
        .to_string();
    (service, timer)
}

/// The launchd LaunchAgent plist that runs `update --yes` on an interval
/// (macOS has no timer-unit split; a `StartInterval` agent is the analogue).
pub fn render_update_timer_launchd(wizard_exe: &Path) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \x20 <key>Label</key>\n\
         \x20 <string>com.posthaste.wizard-update</string>\n\
         \x20 <key>ProgramArguments</key>\n\
         \x20 <array>\n\
         \x20   <string>{exec}</string>\n\
         \x20   <string>update</string>\n\
         \x20   <string>--yes</string>\n\
         \x20 </array>\n\
         \x20 <key>StartInterval</key>\n\
         \x20 <integer>86400</integer>\n\
         </dict>\n\
         </plist>\n",
        exec = crate::render::xml_escape(&wizard_exe.display().to_string()),
    )
}

/// Install the opt-in auto-update timer for the host's init system. Writes into
/// the *user* unit dir (never system; never sudo). Refuses-and-explains if that
/// directory is not writable — the crate's existing posture. Returns the files
/// written.
pub fn install_update_timer(wizard_exe: &Path) -> Result<Vec<PathBuf>, String> {
    match ServiceScope::detect(false) {
        ServiceScope::UserSystemd => {
            let dir = user_unit_dir()?;
            ensure_writable_dir(&dir)?;
            let (service, timer) = render_update_timer_systemd(wizard_exe);
            let service_path = dir.join("posthaste-wizard-update.service");
            let timer_path = dir.join("posthaste-wizard-update.timer");
            write_refusing(&service_path, &service)?;
            write_refusing(&timer_path, &timer)?;
            // Best-effort enable; the files are written regardless.
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .output();
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "enable", "--now", "posthaste-wizard-update.timer"])
                .output();
            Ok(vec![service_path, timer_path])
        }
        ServiceScope::Launchd => {
            let dir = crate::install::launch_agents_dir_pub()?;
            ensure_writable_dir(&dir)?;
            let plist = render_update_timer_launchd(wizard_exe);
            let plist_path = dir.join("com.posthaste.wizard-update.plist");
            write_refusing(&plist_path, &plist)?;
            let _ = std::process::Command::new("launchctl")
                .args(["load", "-w"])
                .arg(&plist_path)
                .output();
            Ok(vec![plist_path])
        }
        _ => Err(
            "no supported user init system on this host (systemd --user or launchd); \
                  cannot install an auto-update timer"
                .to_string(),
        ),
    }
}

/// Create the unit dir and confirm it is writable, refusing (never sudo) with a
/// clear explanation otherwise.
fn ensure_writable_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| {
        format!(
            "the unit directory {} is not writable ({e}); the wizard never uses sudo — \
             create it yourself or fix its ownership, then re-run",
            dir.display()
        )
    })
}

fn write_refusing(path: &Path, body: &str) -> Result<(), String> {
    std::fs::write(path, body).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            format!(
                "refusing to write {} ({e}); the wizard never escalates to sudo — \
                 fix the directory's ownership and re-run",
                path.display()
            )
        } else {
            format!("write {}: {e}", path.display())
        }
    })
}

// -- Recording helper (used by the install paths to retrofit the manifest) --

/// Build + persist a manifest entry for a freshly installed component. Called
/// by both install paths so the manifest is populated from now on (ruling 14
/// retrofit). Best-effort resolution of the concrete version from the channel.
#[allow(clippy::too_many_arguments)]
pub fn record_install(
    manifest_path: &Path,
    component: &str,
    kind: &str,
    installed_path: &Path,
    version: &Version,
    source: &dyn ReleaseSource,
    service: Option<ServiceScope>,
    unit: Option<String>,
) -> Result<(), String> {
    let channel = version.channel();
    let concrete = concrete_version(version, source);
    let mut manifest = Manifest::load(manifest_path)?;
    manifest.record(Component {
        component: component.to_string(),
        kind: kind.to_string(),
        path: installed_path.display().to_string(),
        version: concrete,
        channel: channel.as_str().to_string(),
        installed_at: now_rfc3339(),
        service: service
            .filter(|s| *s != ServiceScope::None)
            .map(|s| s.as_str().to_string()),
        unit,
        previous_version: None,
    });
    manifest.save(manifest_path)
}

/// Record the *running wizard* as a manifest component, so `update` can
/// self-update it last (ruling 14). Called from the install paths: the wizard
/// that performed an install is on the same channel release line as what it
/// installed, so its own entry appears as soon as any install happens.
/// Best-effort — a failure to resolve `current_exe` or the version is a no-op,
/// never a failed install.
pub fn record_self_wizard(
    manifest_path: &Path,
    version: &Version,
    source: &dyn ReleaseSource,
) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("resolve wizard path: {e}"))?;
    record_install(
        manifest_path,
        "posthaste-wizard",
        "wizard",
        &exe,
        version,
        source,
        None,
        None,
    )
}

/// The concrete version to record: a pinned tag resolves locally (strip the
/// leading `v`); a channel install resolves the rolling manifest's `version`,
/// falling back to the rolling tag name if that fetch fails (so a mirror
/// without the updater manifest still records *something* and never fails the
/// install over manifest bookkeeping).
fn concrete_version(version: &Version, source: &dyn ReleaseSource) -> String {
    match version {
        Version::Pinned(tag) => tag.trim_start_matches('v').to_string(),
        Version::Channel(channel) => fetch::resolve_latest_version(source, *channel)
            .unwrap_or_else(|_| channel.rolling_tag().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::FetchError;
    use std::collections::HashMap;
    use std::os::unix::fs::PermissionsExt;

    /// A release source that answers `latest.json`/`latest-stable.json` with a
    /// canned version, plus any explicitly stocked assets.
    struct MockSource {
        latest: HashMap<String, String>, // manifest asset name -> version
        assets: HashMap<(String, String), Vec<u8>>,
    }
    impl ReleaseSource for MockSource {
        fn fetch(&self, tag: &str, asset: &str) -> Result<Vec<u8>, FetchError> {
            if let Some(v) = self.latest.get(asset) {
                return Ok(format!("{{\"version\":\"{v}\"}}").into_bytes());
            }
            self.assets
                .get(&(tag.to_string(), asset.to_string()))
                .cloned()
                .ok_or_else(|| FetchError::Http(format!("{tag}/{asset}"), "not found".into()))
        }
    }

    fn role_entry(version: &str) -> Component {
        Component {
            component: "posthaste-runtime".into(),
            kind: "role".into(),
            path: "/nonexistent/posthaste-runtime".into(),
            version: version.into(),
            channel: "nightly".into(),
            installed_at: "t".into(),
            service: None,
            unit: None,
            previous_version: None,
        }
    }

    #[test]
    fn check_table_classifies_newer_same_and_unknown() {
        let mut latest = HashMap::new();
        latest.insert("latest.json".to_string(), "0.2.0-nightly.50".to_string());
        let source = MockSource {
            latest,
            assets: HashMap::new(),
        };

        let mut m = Manifest::default();
        m.record(role_entry("0.2.0-nightly.44")); // older -> update available
        m.record(Component {
            component: "posthastectl".into(),
            kind: "ctl".into(),
            version: "0.2.0-nightly.50".into(), // same -> up to date
            ..role_entry("x")
        });
        m.record(Component {
            component: "posthaste-authority-server".into(),
            channel: "stable".into(), // no latest-stable.json stocked -> unknown
            ..role_entry("1.0.0")
        });

        let plan = plan_updates(&m, &source);
        let by = |name: &str| plan.iter().find(|s| s.component == name).unwrap();
        assert_eq!(by("posthaste-runtime").state, UpdateState::UpdateAvailable);
        assert_eq!(by("posthastectl").state, UpdateState::UpToDate);
        assert!(matches!(
            by("posthaste-authority-server").state,
            UpdateState::Unknown(_)
        ));

        let table = format_status_table(&plan);
        assert!(table.contains("posthaste-runtime"));
        assert!(table.contains("0.2.0-nightly.44 \u{2192} 0.2.0-nightly.50"));
    }

    #[test]
    fn empty_manifest_table_is_graceful() {
        let table = format_status_table(&[]);
        assert!(table.contains("nothing to update"));
    }

    #[test]
    fn swap_keeps_bak_and_rollback_restores() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("posthaste-runtime");
        std::fs::write(&path, b"OLD-V1").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let new_path = sibling(&path, ".new");
        std::fs::write(&new_path, b"NEW-V2").unwrap();

        let bak = atomic_swap(&path, &new_path).unwrap().expect("bak kept");
        assert_eq!(std::fs::read(&path).unwrap(), b"NEW-V2");
        assert_eq!(std::fs::read(&bak).unwrap(), b"OLD-V1");
        assert!(!new_path.exists(), ".new consumed by the rename");

        rollback(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"OLD-V1");
        assert!(!bak.exists(), ".bak consumed by rollback");
    }

    #[test]
    fn swap_into_a_fresh_path_keeps_no_bak() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("posthastectl");
        let new_path = sibling(&path, ".new");
        std::fs::write(&new_path, b"FRESH").unwrap();
        let bak = atomic_swap(&path, &new_path).unwrap();
        assert!(bak.is_none(), "no prior binary -> no .bak");
        assert_eq!(std::fs::read(&path).unwrap(), b"FRESH");
    }

    #[test]
    fn rollback_without_a_bak_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("posthaste-runtime");
        std::fs::write(&path, b"x").unwrap();
        assert!(rollback(&path).is_err());
    }

    #[test]
    fn self_update_rename_over_a_running_copy_swaps_and_keeps_bak() {
        // Model the wizard rewriting its own binary: the "running" file is held
        // open, and the unix rename still replaces its path while the open
        // handle keeps the old inode. Proves the self-update swap mechanic.
        let dir = tempfile::tempdir().unwrap();
        let running = dir.path().join("posthaste-wizard");
        std::fs::write(&running, b"WIZARD-OLD").unwrap();
        std::fs::set_permissions(&running, std::fs::Permissions::from_mode(0o755)).unwrap();
        let open = std::fs::File::open(&running).unwrap(); // pretend it is executing

        let staged = sibling(&running, ".new");
        std::fs::write(&staged, b"WIZARD-NEW").unwrap();
        let bak = atomic_swap(&running, &staged).unwrap().expect("bak kept");

        assert_eq!(std::fs::read(&running).unwrap(), b"WIZARD-NEW");
        assert_eq!(std::fs::read(&bak).unwrap(), b"WIZARD-OLD");
        drop(open);
    }

    #[test]
    fn apply_update_downloads_verifies_and_swaps_a_role_binary() {
        use flate2::write::GzEncoder;
        use sha2::{Digest, Sha256};
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let installed = dir.path().join("posthaste-runtime");
        std::fs::write(&installed, b"RUNTIME-OLD").unwrap();
        std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Build a real PosthasteRuntimeNightly tarball with bin/posthaste-runtime.
        let base = "PosthasteRuntimeNightly";
        let platform = "linux-x86_64";
        let inner = b"RUNTIME-NEW".to_vec();
        let mut tar = tar::Builder::new(Vec::new());
        let mut h = tar::Header::new_gnu();
        h.set_size(inner.len() as u64);
        h.set_mode(0o755);
        h.set_cksum();
        tar.append_data(
            &mut h,
            format!("{base}-{platform}/bin/posthaste-runtime"),
            &inner[..],
        )
        .unwrap();
        let tar_bytes = tar.into_inner().unwrap();
        let mut gz = GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&tar_bytes).unwrap();
        let tarball = gz.finish().unwrap();
        let tarball_name = format!("{base}-{platform}.tar.gz");
        let sums = format!("{:x}  {}\n", Sha256::digest(&tarball), tarball_name);

        let mut assets = HashMap::new();
        assets.insert(("nightly".to_string(), tarball_name), tarball);
        assets.insert(
            ("nightly".to_string(), "SHA256SUMS".to_string()),
            sums.into_bytes(),
        );
        let source = MockSource {
            latest: HashMap::new(),
            assets,
        };

        let entry = Component {
            path: installed.display().to_string(),
            ..role_entry("0.2.0-nightly.44")
        };
        let out = apply_update(&entry, "0.2.0-nightly.50", &source, Some(platform)).unwrap();
        assert_eq!(out.to, "0.2.0-nightly.50");
        assert_eq!(std::fs::read(&installed).unwrap(), b"RUNTIME-NEW");
        assert_eq!(std::fs::read(bak_path(&installed)).unwrap(), b"RUNTIME-OLD");
    }
}
