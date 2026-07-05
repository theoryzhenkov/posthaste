//! The `install` flow: fetch a role binary, place it, provision the node, and
//! register a `systemd --user` service that keeps it running.
//!
//! This is the "press a button" path. [`crate::provision`] still does the config
//! and TLS work; `install` wraps it with the three steps that were previously
//! manual, in order: fetch and place the role binary, register and start the
//! `systemd --user` service, and (for a two-machine split) emit or consume a
//! one-line join string so the second node is a single command.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::fetch::{self, ReleaseSource, Version};
use crate::render::{launchd_label, render_launchd_plist, render_systemd_unit};
use crate::{provision, Plan, Provisioned, Role};

/// Which service manager keeps the node running. Defaults to the platform norm;
/// `--system` upgrades Linux to a root system unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceScope {
    /// Linux `systemctl --user` unit in `~/.config/systemd/user` (default).
    UserSystemd,
    /// Linux system unit in `/etc/systemd/system`, run as the invoking user
    /// (needs root to register).
    SystemSystemd,
    /// macOS launchd LaunchAgent in `~/Library/LaunchAgents` (default on macOS).
    Launchd,
    /// Write nothing, register nothing.
    None,
}

impl ServiceScope {
    /// The default scope for the host: launchd on macOS, user-systemd on Linux
    /// (system-systemd when `system` is set). Unknown platforms get `None`.
    pub fn detect(system: bool) -> ServiceScope {
        match std::env::consts::OS {
            "macos" => ServiceScope::Launchd,
            "linux" if system => ServiceScope::SystemSystemd,
            "linux" => ServiceScope::UserSystemd,
            _ => ServiceScope::None,
        }
    }

    /// The wire name recorded in the install manifest so `update` can recover
    /// the scope and drive the service around a swap.
    pub fn as_str(self) -> &'static str {
        match self {
            ServiceScope::UserSystemd => "user-systemd",
            ServiceScope::SystemSystemd => "system-systemd",
            ServiceScope::Launchd => "launchd",
            ServiceScope::None => "none",
        }
    }

    /// Parse a scope from its manifest wire name.
    pub fn parse(s: &str) -> ServiceScope {
        match s {
            "user-systemd" => ServiceScope::UserSystemd,
            "system-systemd" => ServiceScope::SystemSystemd,
            "launchd" => ServiceScope::Launchd,
            _ => ServiceScope::None,
        }
    }
}

/// Stop a running service before a binary swap. `unit` is the systemd unit name
/// or the launchd plist path (as recorded in the manifest). Best-effort: a
/// stop failure is returned so `update` can warn, never a hard error (the unit
/// may simply not be loaded).
pub fn stop_service(scope: ServiceScope, unit: &str) -> Result<(), String> {
    match scope {
        ServiceScope::UserSystemd => run("systemctl", &["--user", "stop", unit]),
        ServiceScope::SystemSystemd => run("systemctl", &["stop", unit]),
        ServiceScope::Launchd => run("launchctl", &["unload", unit]),
        ServiceScope::None => Ok(()),
    }
}

/// Start a service after a binary swap (the inverse of [`stop_service`]).
pub fn start_service(scope: ServiceScope, unit: &str) -> Result<(), String> {
    match scope {
        ServiceScope::UserSystemd => run("systemctl", &["--user", "start", unit]),
        ServiceScope::SystemSystemd => run("systemctl", &["start", unit]),
        ServiceScope::Launchd => run("launchctl", &["load", "-w", unit]),
        ServiceScope::None => Ok(()),
    }
}

/// Everything `install` needs beyond a provisioning [`Plan`].
pub struct InstallOptions {
    /// Which release to fetch.
    pub version: Version,
    /// Release platform suffix (e.g. `linux-x86_64`); detected when `None`.
    pub platform: Option<String>,
    /// Directory the binary is installed into (e.g. `~/.local/bin`).
    pub bin_dir: PathBuf,
    /// Which service manager to register the node with after provisioning.
    pub service: ServiceScope,
    /// Best-effort `loginctl enable-linger` so a user systemd service survives
    /// logout (needs root once; a failure is a warning). Ignored for other
    /// scopes.
    pub enable_linger: bool,
}

/// The outcome of an install, layered on top of [`Provisioned`].
pub struct Installed {
    pub binary_path: PathBuf,
    pub provisioned: Provisioned,
    /// Non-fatal problems (e.g. systemd not present, linger needs root) the CLI
    /// should surface without failing the install.
    pub warnings: Vec<String>,
    /// The service file written (unit or plist), if a scope was registered.
    pub service_path: Option<PathBuf>,
    /// For an authority-server/daemon node: the one-line join string a runtime node feeds
    /// to `--join` to wire itself up. `None` for a runtime node.
    pub join_string: Option<String>,
}

/// Fetch + install the binary, provision the node, and register the service.
/// `plan.exec_path` is filled in here; `install` owns the service file, so
/// `plan.systemd_unit_path` is left unset (provision writes no unit).
pub fn install(
    mut plan: Plan,
    opts: &InstallOptions,
    source: &dyn ReleaseSource,
) -> Result<Installed, String> {
    let platform = match &opts.platform {
        Some(p) => p.clone(),
        None => detect_platform()?,
    };

    // Place the binary first: the service file references this path, so it must
    // exist before we render the unit/plist.
    let binary_path = opts.bin_dir.join(plan.role.binary());
    fetch::fetch_and_install(source, plan.role, &opts.version, &platform, &binary_path)
        .map_err(|e| format!("fetch {}: {e}", plan.role.binary()))?;
    plan.exec_path = Some(binary_path.clone());

    let provisioned = provision(&plan)?;

    let mut warnings = Vec::new();
    let service_path = if opts.service == ServiceScope::None {
        None
    } else {
        match write_and_register_service(opts.service, &plan, opts.enable_linger, &mut warnings) {
            Ok(path) => Some(path),
            Err(e) => {
                // The binary + config are installed; a service-step failure is a
                // warning the operator can act on, not a failed install.
                warnings.push(e);
                None
            }
        }
    };

    // A authority server/daemon node emits the join string a runtime node consumes. The
    // link token is operator-supplied at provision time (not the daemon's
    // first-start API token), so it is known now.
    let join_string = match plan.role {
        Role::AuthorityServer | Role::Daemon => emit_join(&plan, &provisioned),
        Role::Runtime => None,
    };

    Ok(Installed {
        binary_path,
        provisioned,
        warnings,
        service_path,
        join_string,
    })
}

/// Write the scope's service file (creating its dir) and register it with the
/// service manager. Returns the path written.
fn write_and_register_service(
    scope: ServiceScope,
    plan: &Plan,
    enable_linger: bool,
    warnings: &mut Vec<String>,
) -> Result<PathBuf, String> {
    let (path, contents) = service_file(scope, plan)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create service dir {}: {e}", parent.display()))?;
    }
    fs::write(&path, contents).map_err(|e| {
        let hint = if scope == ServiceScope::SystemSystemd {
            " (a system unit needs root — re-run with sudo, or use the default --user scope)"
        } else {
            ""
        };
        format!("write service file {}: {e}{hint}", path.display())
    })?;
    register(scope, &path, plan.role, enable_linger, warnings);
    Ok(path)
}

/// The service file path + contents for a scope.
fn service_file(scope: ServiceScope, plan: &Plan) -> Result<(PathBuf, String), String> {
    match scope {
        ServiceScope::UserSystemd => Ok((
            user_unit_dir()?.join(unit_name(plan.role)),
            render_systemd_unit(plan, None),
        )),
        ServiceScope::SystemSystemd => {
            let user = std::env::var("USER").ok();
            Ok((
                PathBuf::from("/etc/systemd/system").join(unit_name(plan.role)),
                render_systemd_unit(plan, user.as_deref()),
            ))
        }
        ServiceScope::Launchd => Ok((
            launch_agents_dir()?.join(plist_name(plan.role)),
            render_launchd_plist(plan),
        )),
        ServiceScope::None => Err("no service scope to write".into()),
    }
}

/// Register + start the written service file with its manager. Manager absence
/// or a missing privilege is a warning (the file is written either way).
fn register(
    scope: ServiceScope,
    path: &Path,
    role: Role,
    enable_linger: bool,
    warnings: &mut Vec<String>,
) {
    match scope {
        ServiceScope::UserSystemd => {
            register_user_service(&unit_name(role), warnings);
            if enable_linger {
                self::enable_linger(warnings);
            }
        }
        ServiceScope::SystemSystemd => register_system_service(&unit_name(role), warnings),
        ServiceScope::Launchd => register_launchd(path, &launchd_label(role), warnings),
        ServiceScope::None => {}
    }
}

/// `~/Library/LaunchAgents`, where per-user launchd jobs live.
fn launch_agents_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library").join("LaunchAgents"))
        .ok_or_else(|| "HOME is not set; cannot place the launchd agent".into())
}

/// Public accessor for [`launch_agents_dir`] so `update`/`watch` can place their
/// own LaunchAgents (the auto-update timer, a registered watch) alongside the
/// role agents.
pub fn launch_agents_dir_pub() -> Result<PathBuf, String> {
    launch_agents_dir()
}

/// The launchd plist file name for a role, e.g. `com.posthaste.authority-server.plist`.
fn plist_name(role: Role) -> String {
    format!("{}.plist", launchd_label(role))
}

/// Reload + `enable --now` a system unit (needs root). Failure is a warning with
/// a sudo hint.
fn register_system_service(unit: &str, warnings: &mut Vec<String>) {
    if run("systemctl", &["daemon-reload"]).is_err() {
        warnings.push(format!(
            "could not reload systemd; the unit is written — run \
             `sudo systemctl enable --now {unit}` to start it"
        ));
        return;
    }
    if let Err(e) = run("systemctl", &["enable", "--now", unit]) {
        warnings.push(format!(
            "could not enable/start {unit} ({e}); run `sudo systemctl enable --now {unit}`"
        ));
    }
}

/// Load a launchd agent. `launchctl load -w` works across macOS versions; its
/// absence (e.g. on Linux) is a warning.
fn register_launchd(plist: &Path, label: &str, warnings: &mut Vec<String>) {
    let plist = plist.display().to_string();
    if let Err(e) = run("launchctl", &["load", "-w", &plist]) {
        warnings.push(format!(
            "could not load {label} ({e}); the plist is written — run \
             `launchctl load -w {plist}` to start it"
        ));
    }
}

/// Map the host triple to the release platform suffix used by the build matrix
/// (`linux-x86_64`, `macos`, `windows-x86_64`). macOS publishes a single arch,
/// so it carries no arch suffix — matching `release.yml`.
pub fn detect_platform() -> Result<String, String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("linux", "x86_64") => Ok("linux-x86_64".into()),
        ("macos", _) => Ok("macos".into()),
        ("windows", "x86_64") => Ok("windows-x86_64".into()),
        _ => Err(format!(
            "no published binary for {os}/{arch}; pass --platform to override or build from source"
        )),
    }
}

/// The `systemd --user` unit directory (`$XDG_CONFIG_HOME/systemd/user`, or
/// `~/.config/systemd/user`).
pub fn user_unit_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or("neither XDG_CONFIG_HOME nor HOME is set; cannot place the user service unit")?;
    Ok(base.join("systemd").join("user"))
}

/// The unit file name for a role, e.g. `posthaste-authority-server.service`.
pub fn unit_name(role: Role) -> String {
    let suffix = match role {
        // The bundled all-in-one runs the `posthaste-authority-runtime-server`
        // binary (D18); the unit is named after the binary it runs.
        Role::Daemon => "authority-runtime-server",
        Role::AuthorityServer => "authority-server",
        Role::Runtime => "runtime",
    };
    format!("posthaste-{suffix}.service")
}

/// Reload the user manager and `enable --now` the unit. systemd absence (e.g.
/// macOS, a minimal container) is a warning, not a failure: the unit file is
/// written regardless, so the operator can start it by hand.
fn register_user_service(unit: &str, warnings: &mut Vec<String>) {
    if let Err(e) = run("systemctl", &["--user", "daemon-reload"]) {
        warnings.push(format!(
            "could not reload the user service manager ({e}); the unit file is written — \
             start it manually with `systemctl --user enable --now {unit}`"
        ));
        return;
    }
    if let Err(e) = run("systemctl", &["--user", "enable", "--now", unit]) {
        warnings.push(format!(
            "could not enable/start {unit} ({e}); start it manually with \
             `systemctl --user enable --now {unit}`"
        ));
    }
}

/// Best-effort linger so the user service runs without an active login session.
/// Needs root once; a failure is informational.
fn enable_linger(warnings: &mut Vec<String>) {
    let user = std::env::var("USER").unwrap_or_default();
    if user.is_empty() {
        warnings.push("USER not set; skipped `loginctl enable-linger`".into());
        return;
    }
    if let Err(e) = run("loginctl", &["enable-linger", &user]) {
        warnings.push(format!(
            "could not enable linger for {user} ({e}); the service will only run while you are \
             logged in — run `sudo loginctl enable-linger {user}` for an always-on node"
        ));
    }
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("`{program}` failed to launch: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// The wire form of a join string: what a runtime node needs to reach an authority server.
#[derive(Serialize, Deserialize)]
struct Join {
    authority_server_url: String,
    token: String,
    /// CA certificate (PEM) the runtime must trust for a TLS authority server.
    #[serde(skip_serializing_if = "Option::is_none")]
    ca_cert_pem: Option<String>,
}

/// Build the join string a runtime node consumes — `authority_server_url` + link token
/// (+ CA for TLS), hex-encoded so it is a single copy-pasteable token with no
/// extra dependency.
fn emit_join(plan: &Plan, provisioned: &Provisioned) -> Option<String> {
    let token = plan
        .link_serve_token
        .clone()
        .or_else(|| plan.link_token.clone())?;
    let scheme = if plan.tls { "https" } else { "http" };
    let host = plan.hosts.first().cloned()?;
    let port = plan.bind.rsplit(':').next().unwrap_or(&plan.bind);
    let authority_server_url = format!("{scheme}://{host}:{port}");

    let ca_cert_pem = provisioned
        .ca_cert_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok());

    let join = Join {
        authority_server_url,
        token,
        ca_cert_pem,
    };
    let json = serde_json::to_string(&join).ok()?;
    Some(hex_encode(json.as_bytes()))
}

/// Decode a `--join` string and apply it to a runtime plan: set the authority server URL
/// and link token, and (for TLS) write the CA cert into the config root and
/// point the plan's trust at it. Returns the CA path written, if any.
pub fn apply_join(plan: &mut Plan, join: &str) -> Result<Option<PathBuf>, String> {
    let bytes = hex_decode(join).map_err(|e| format!("invalid --join string: {e}"))?;
    let join: Join = serde_json::from_slice(&bytes)
        .map_err(|e| format!("invalid --join string: not a recognized join payload ({e})"))?;

    plan.link_authority_server_url = Some(join.authority_server_url);
    plan.link_token = Some(join.token);

    let ca_path = match join.ca_cert_pem {
        Some(pem) => {
            std::fs::create_dir_all(&plan.config_root)
                .map_err(|e| format!("create config root: {e}"))?;
            let path = plan.config_root.join("authority-server-ca.crt");
            std::fs::write(&path, pem).map_err(|e| format!("write {}: {e}", path.display()))?;
            Some(path)
        }
        None => None,
    };
    Ok(ca_path)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return Err("odd-length hex".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| "non-hex character".to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn base_plan(role: Role, dir: &Path) -> Plan {
        Plan {
            role,
            config_root: dir.join("config"),
            state_root: dir.join("state"),
            bind: "0.0.0.0:3443".into(),
            tls: false,
            hosts: vec!["node.example".into()],
            link_serve_token: None,
            link_authority_server_url: None,
            link_token: None,
            exec_path: None,
            systemd_unit_path: None,
        }
    }

    #[test]
    fn unit_names_are_role_scoped() {
        assert_eq!(
            unit_name(Role::Daemon),
            "posthaste-authority-runtime-server.service"
        );
        assert_eq!(
            unit_name(Role::AuthorityServer),
            "posthaste-authority-server.service"
        );
        assert_eq!(unit_name(Role::Runtime), "posthaste-runtime.service");
        assert_eq!(
            plist_name(Role::AuthorityServer),
            "com.posthaste.authority-server.plist"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn detect_picks_systemd_scope_on_linux() {
        assert_eq!(ServiceScope::detect(false), ServiceScope::UserSystemd);
        assert_eq!(ServiceScope::detect(true), ServiceScope::SystemSystemd);
    }

    #[test]
    fn user_systemd_file_targets_default_and_omits_user() {
        let dir = tempfile::tempdir().unwrap();
        let plan = base_plan(Role::AuthorityServer, dir.path());
        temp_env(
            &[("XDG_CONFIG_HOME", Some("/x/cfg")), ("HOME", None)],
            || {
                let (path, body) = service_file(ServiceScope::UserSystemd, &plan).unwrap();
                assert_eq!(
                    path,
                    PathBuf::from("/x/cfg/systemd/user/posthaste-authority-server.service")
                );
                assert!(body.contains("WantedBy=default.target"));
                assert!(!body.contains("User="), "a user unit must not pin User=");
            },
        );
    }

    #[test]
    fn system_systemd_file_pins_user_and_multi_user_target() {
        let dir = tempfile::tempdir().unwrap();
        let plan = base_plan(Role::Runtime, dir.path());
        temp_env(&[("USER", Some("mailsvc"))], || {
            let (path, body) = service_file(ServiceScope::SystemSystemd, &plan).unwrap();
            assert_eq!(
                path,
                PathBuf::from("/etc/systemd/system/posthaste-runtime.service")
            );
            assert!(body.contains("WantedBy=multi-user.target"));
            assert!(body.contains("User=mailsvc"));
            assert!(body.contains("Group=mailsvc"));
        });
    }

    #[test]
    fn launchd_file_is_a_plist_with_label_and_args() {
        let dir = tempfile::tempdir().unwrap();
        // Daemon role exercises the `serve` subcommand in ProgramArguments.
        let mut plan = base_plan(Role::Daemon, dir.path());
        plan.exec_path = Some(PathBuf::from("/opt/bin/posthaste-authority-runtime-server"));
        temp_env(&[("HOME", Some("/home/u"))], || {
            let (path, body) = service_file(ServiceScope::Launchd, &plan).unwrap();
            assert_eq!(
                path,
                PathBuf::from("/home/u/Library/LaunchAgents/com.posthaste.daemon.plist")
            );
            assert!(body.contains("<key>Label</key>"));
            assert!(body.contains("<string>com.posthaste.daemon</string>"));
            assert!(body.contains("<string>/opt/bin/posthaste-authority-runtime-server</string>"));
            assert!(
                body.contains("<string>serve</string>"),
                "daemon needs the serve arg"
            );
            assert!(body.contains("POSTHASTE_CONFIG_ROOT"));
        });
    }

    #[test]
    fn user_unit_dir_prefers_xdg() {
        // Set both; XDG_CONFIG_HOME must win.
        temp_env(
            &[
                ("XDG_CONFIG_HOME", Some("/x/cfg")),
                ("HOME", Some("/home/u")),
            ],
            || {
                assert_eq!(
                    user_unit_dir().unwrap(),
                    PathBuf::from("/x/cfg/systemd/user")
                );
            },
        );
        temp_env(
            &[("XDG_CONFIG_HOME", None), ("HOME", Some("/home/u"))],
            || {
                assert_eq!(
                    user_unit_dir().unwrap(),
                    PathBuf::from("/home/u/.config/systemd/user")
                );
            },
        );
    }

    #[test]
    fn join_round_trips_authority_server_to_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let mut authority_server = base_plan(Role::AuthorityServer, dir.path());
        authority_server.link_serve_token = Some("link-secret".into());

        // Provisioned with no CA (TLS off) — join carries url + token only.
        let provisioned = Provisioned {
            app_toml_path: PathBuf::new(),
            ca_cert_path: None,
            leaf_cert_path: None,
            systemd_unit_path: None,
            client_profile_json: String::new(),
        };
        let join = emit_join(&authority_server, &provisioned)
            .expect("authority_server emits a join string");

        let mut runtime = base_plan(Role::Runtime, dir.path());
        let ca = apply_join(&mut runtime, &join).unwrap();
        assert_eq!(runtime.link_token.as_deref(), Some("link-secret"));
        assert_eq!(
            runtime.link_authority_server_url.as_deref(),
            Some("http://node.example:3443")
        );
        assert!(ca.is_none(), "no CA in a non-TLS join");
    }

    #[test]
    fn join_carries_ca_for_tls() {
        let dir = tempfile::tempdir().unwrap();
        let ca_file = dir.path().join("ca.crt");
        std::fs::write(
            &ca_file,
            "-----BEGIN CERTIFICATE-----\nXXXX\n-----END CERTIFICATE-----\n",
        )
        .unwrap();

        let mut authority_server = base_plan(Role::AuthorityServer, dir.path());
        authority_server.tls = true;
        authority_server.link_serve_token = Some("s".into());
        let provisioned = Provisioned {
            app_toml_path: PathBuf::new(),
            ca_cert_path: Some(ca_file),
            leaf_cert_path: None,
            systemd_unit_path: None,
            client_profile_json: String::new(),
        };
        let join = emit_join(&authority_server, &provisioned).unwrap();

        let mut runtime = base_plan(Role::Runtime, dir.path());
        let ca = apply_join(&mut runtime, &join)
            .unwrap()
            .expect("CA written");
        assert!(ca.ends_with("authority-server-ca.crt"));
        assert!(std::fs::read_to_string(&ca)
            .unwrap()
            .contains("BEGIN CERTIFICATE"));
        assert_eq!(
            runtime.link_authority_server_url.as_deref(),
            Some("https://node.example:3443")
        );
    }

    #[test]
    fn rejects_a_garbled_join() {
        let dir = tempfile::tempdir().unwrap();
        let mut runtime = base_plan(Role::Runtime, dir.path());
        assert!(apply_join(&mut runtime, "not-hex!!").is_err());
        assert!(apply_join(&mut runtime, "abcd").is_err()); // valid hex, not JSON
    }

    /// Minimal scoped env swap for the unit-dir test. Serializes on a mutex so
    /// the two cases don't race on process-global env.
    fn temp_env(vars: &[(&str, Option<&str>)], f: impl FnOnce()) {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _guard = LOCK.lock().unwrap();
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
