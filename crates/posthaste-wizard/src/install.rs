//! The `install` flow: fetch a role binary, place it, provision the node, and
//! register a `systemd --user` service that keeps it running.
//!
//! This is the "press a button" path. [`crate::provision`] still does the config
//! and TLS work; `install` wraps it with the three steps that were previously
//! manual, in order: fetch and place the role binary, register and start the
//! `systemd --user` service, and (for a two-machine split) emit or consume a
//! one-line join string so the second node is a single command.

use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::fetch::{self, ReleaseSource, Version};
use crate::{provision, Plan, Provisioned, Role};

/// Everything `install` needs beyond a provisioning [`Plan`].
pub struct InstallOptions {
    /// Which release to fetch.
    pub version: Version,
    /// Release platform suffix (e.g. `linux-x86_64`); detected when `None`.
    pub platform: Option<String>,
    /// Directory the binary is installed into (e.g. `~/.local/bin`).
    pub bin_dir: PathBuf,
    /// Register + start a `systemd --user` service after provisioning.
    pub register_service: bool,
    /// Best-effort `loginctl enable-linger` so the user service survives logout
    /// (needs root once; a failure is a warning, not an error).
    pub enable_linger: bool,
}

/// The outcome of an install, layered on top of [`Provisioned`].
pub struct Installed {
    pub binary_path: PathBuf,
    pub provisioned: Provisioned,
    /// Non-fatal problems (e.g. systemd not present, linger needs root) the CLI
    /// should surface without failing the install.
    pub warnings: Vec<String>,
    /// For a backend/daemon node: the one-line join string a runtime node feeds
    /// to `--join` to wire itself up. `None` for a runtime node.
    pub join_string: Option<String>,
}

/// Fetch + install the binary, provision the node, and (optionally) register the
/// service. `plan.exec_path` and `plan.systemd_unit_path` are filled in here from
/// `opts`, so callers leave them unset.
pub fn install(
    mut plan: Plan,
    opts: &InstallOptions,
    source: &dyn ReleaseSource,
) -> Result<Installed, String> {
    let platform = match &opts.platform {
        Some(p) => p.clone(),
        None => detect_platform()?,
    };

    // Place the binary first: provisioning's service unit references this path,
    // so it must exist before we render the unit.
    let binary_path = opts.bin_dir.join(plan.role.binary());
    fetch::fetch_and_install(source, plan.role, &opts.version, &platform, &binary_path)
        .map_err(|e| format!("fetch {}: {e}", plan.role.binary()))?;
    plan.exec_path = Some(binary_path.clone());

    let unit_dir = user_unit_dir()?;
    plan.systemd_unit_path = Some(unit_dir.join(unit_name(plan.role)));

    let provisioned = provision(&plan)?;

    let mut warnings = Vec::new();
    if opts.register_service {
        register_user_service(&unit_name(plan.role), &mut warnings);
        if opts.enable_linger {
            enable_linger(&mut warnings);
        }
    }

    // A backend/daemon node emits the join string a runtime node consumes. The
    // link token is operator-supplied at provision time (not the daemon's
    // first-start API token), so it is known now.
    let join_string = match plan.role {
        Role::Backend | Role::Daemon => emit_join(&plan, &provisioned),
        Role::Runtime => None,
    };

    Ok(Installed {
        binary_path,
        provisioned,
        warnings,
        join_string,
    })
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

/// The unit file name for a role, e.g. `posthaste-backend.service`.
pub fn unit_name(role: Role) -> String {
    let suffix = match role {
        Role::Daemon => "daemon",
        Role::Backend => "backend",
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

/// The wire form of a join string: what a runtime node needs to reach a backend.
#[derive(Serialize, Deserialize)]
struct Join {
    backend_url: String,
    token: String,
    /// CA certificate (PEM) the runtime must trust for a TLS backend.
    #[serde(skip_serializing_if = "Option::is_none")]
    ca_cert_pem: Option<String>,
}

/// Build the join string a runtime node consumes — `backend_url` + link token
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
    let backend_url = format!("{scheme}://{host}:{port}");

    let ca_cert_pem = provisioned
        .ca_cert_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok());

    let join = Join {
        backend_url,
        token,
        ca_cert_pem,
    };
    let json = serde_json::to_string(&join).ok()?;
    Some(hex_encode(json.as_bytes()))
}

/// Decode a `--join` string and apply it to a runtime plan: set the backend URL
/// and link token, and (for TLS) write the CA cert into the config root and
/// point the plan's trust at it. Returns the CA path written, if any.
pub fn apply_join(plan: &mut Plan, join: &str) -> Result<Option<PathBuf>, String> {
    let bytes = hex_decode(join).map_err(|e| format!("invalid --join string: {e}"))?;
    let join: Join = serde_json::from_slice(&bytes)
        .map_err(|e| format!("invalid --join string: not a recognized join payload ({e})"))?;

    plan.link_backend_url = Some(join.backend_url);
    plan.link_token = Some(join.token);

    let ca_path = match join.ca_cert_pem {
        Some(pem) => {
            std::fs::create_dir_all(&plan.config_root)
                .map_err(|e| format!("create config root: {e}"))?;
            let path = plan.config_root.join("backend-ca.crt");
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
            link_backend_url: None,
            link_token: None,
            exec_path: None,
            systemd_unit_path: None,
        }
    }

    #[test]
    fn unit_names_are_role_scoped() {
        assert_eq!(unit_name(Role::Daemon), "posthaste-daemon.service");
        assert_eq!(unit_name(Role::Backend), "posthaste-backend.service");
        assert_eq!(unit_name(Role::Runtime), "posthaste-runtime.service");
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
    fn join_round_trips_backend_to_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let mut backend = base_plan(Role::Backend, dir.path());
        backend.link_serve_token = Some("link-secret".into());

        // Provisioned with no CA (TLS off) — join carries url + token only.
        let provisioned = Provisioned {
            app_toml_path: PathBuf::new(),
            ca_cert_path: None,
            leaf_cert_path: None,
            systemd_unit_path: None,
            client_profile_json: String::new(),
        };
        let join = emit_join(&backend, &provisioned).expect("backend emits a join string");

        let mut runtime = base_plan(Role::Runtime, dir.path());
        let ca = apply_join(&mut runtime, &join).unwrap();
        assert_eq!(runtime.link_token.as_deref(), Some("link-secret"));
        assert_eq!(
            runtime.link_backend_url.as_deref(),
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

        let mut backend = base_plan(Role::Backend, dir.path());
        backend.tls = true;
        backend.link_serve_token = Some("s".into());
        let provisioned = Provisioned {
            app_toml_path: PathBuf::new(),
            ca_cert_path: Some(ca_file),
            leaf_cert_path: None,
            systemd_unit_path: None,
            client_profile_json: String::new(),
        };
        let join = emit_join(&backend, &provisioned).unwrap();

        let mut runtime = base_plan(Role::Runtime, dir.path());
        let ca = apply_join(&mut runtime, &join)
            .unwrap()
            .expect("CA written");
        assert!(ca.ends_with("backend-ca.crt"));
        assert!(std::fs::read_to_string(&ca)
            .unwrap()
            .contains("BEGIN CERTIFICATE"));
        assert_eq!(
            runtime.link_backend_url.as_deref(),
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
