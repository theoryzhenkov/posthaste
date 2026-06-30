//! `posthaste-wizard`: a one-shot local composition installer.
//!
//! Provisions a single node for a chosen role — writes its `app.toml`, optional
//! TLS material (a local CA + server leaf), and an optional service unit — then
//! emits the connection profile a client uses to reach it. It does not manage
//! connections over time; once a node is provisioned the wizard can be deleted.
//!
//! Lean by design: it links neither the store/engine graph nor the daemon's
//! config crate at runtime (a round-trip dev-test proves the daemon reads what
//! it writes). See [`PLAN-L2-install-wizard`].
//!
//! @spec docs/eph/PLAN-L2-install-wizard

mod certs;
mod render;

use std::fs;
use std::path::{Path, PathBuf};

pub use render::{client_profile_json, render_app_toml, render_systemd_unit};

/// The role a node plays — selects which binary it runs and which config the
/// wizard writes. Mirrors the build matrix in the self-host plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// `posthaste_daemon`: the all-in-one headless daemon (backend + runtime +
    /// `/v1`). The GUI `posthaste_fused` desktop bundle is the app-download path,
    /// not a wizard-provisioned node.
    Daemon,
    /// `posthaste_backend`: the far node; serves the runtime↔backend link only.
    Backend,
    /// `posthaste_runtime_daemon`: the near node; serves `/v1` over a remote
    /// backend.
    Runtime,
}

impl Role {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "daemon" | "posthaste_daemon" => Ok(Role::Daemon),
            "backend" | "posthaste_backend" => Ok(Role::Backend),
            "runtime" | "posthaste_runtime_daemon" => Ok(Role::Runtime),
            other => Err(format!(
                "unknown role '{other}' (expected: daemon | backend | runtime)"
            )),
        }
    }

    /// The binary name this role runs.
    pub fn binary(self) -> &'static str {
        match self {
            Role::Daemon => "posthaste_daemon",
            Role::Backend => "posthaste_backend",
            Role::Runtime => "posthaste_runtime_daemon",
        }
    }
}

/// A fully-specified provisioning request (parsed from CLI flags).
pub struct Plan {
    pub role: Role,
    pub config_root: PathBuf,
    pub state_root: PathBuf,
    /// Address the node binds, e.g. `0.0.0.0:3001`.
    pub bind: String,
    /// Enable in-daemon TLS (generate a CA + leaf, write `[tls]`).
    pub tls: bool,
    /// Hostnames/IPs the node is reached as — leaf SANs + `Host` allowlist.
    pub hosts: Vec<String>,
    /// Backend role: serve the link with this shared token.
    pub link_serve_token: Option<String>,
    /// Runtime role: connect to this backend URL with `link_token`.
    pub link_backend_url: Option<String>,
    pub link_token: Option<String>,
    /// Path to the role binary, used in the service unit's `ExecStart`.
    pub exec_path: Option<PathBuf>,
    /// When set, write a systemd unit to this path.
    pub systemd_unit_path: Option<PathBuf>,
}

/// The result of provisioning: what was written, plus the client connection
/// profile (the operator hands the token out separately, after first start).
pub struct Provisioned {
    pub app_toml_path: PathBuf,
    pub ca_cert_path: Option<PathBuf>,
    pub leaf_cert_path: Option<PathBuf>,
    pub systemd_unit_path: Option<PathBuf>,
    pub client_profile_json: String,
}

/// Run the plan: lay out directories, generate TLS material, render + write the
/// config and (optionally) the service unit, and compute the client profile.
pub fn provision(plan: &Plan) -> Result<Provisioned, String> {
    validate(plan)?;

    fs::create_dir_all(&plan.config_root)
        .map_err(|e| format!("create config root {}: {e}", plan.config_root.display()))?;

    // TLS material (CA + leaf) under <config-root>/tls.
    let mut tls_paths = None;
    if plan.tls {
        let tls_dir = plan.config_root.join("tls");
        fs::create_dir_all(&tls_dir).map_err(|e| format!("create tls dir: {e}"))?;
        let material = certs::generate(&plan.hosts)?;
        let ca_cert = tls_dir.join("ca.crt");
        let leaf_cert = tls_dir.join("leaf.crt");
        let leaf_key = tls_dir.join("leaf.key");
        write(&ca_cert, &material.ca_cert_pem)?;
        // The CA key can mint certs trusted by every client that trusts ca.crt,
        // so it is a secret (0600) just like the leaf key — never world-readable.
        // It is retained only so an operator can re-issue a leaf later; a
        // security-conscious deployment can delete it after provisioning.
        write_private(&tls_dir.join("ca.key"), &material.ca_key_pem)?;
        write(&leaf_cert, &material.leaf_cert_pem)?;
        write_private(&leaf_key, &material.leaf_key_pem)?;
        tls_paths = Some((ca_cert, leaf_cert, leaf_key));
    }

    let (leaf_cert_path, leaf_key_path) = match &tls_paths {
        Some((_, cert, key)) => (Some(cert.clone()), Some(key.clone())),
        None => (None, None),
    };

    let app_toml = render_app_toml(plan, leaf_cert_path.as_deref(), leaf_key_path.as_deref());
    let app_toml_path = plan.config_root.join("app.toml");
    write(&app_toml_path, &app_toml)?;

    let systemd_unit_path = match &plan.systemd_unit_path {
        Some(path) => {
            let unit = render_systemd_unit(plan);
            write(path, &unit)?;
            Some(path.clone())
        }
        None => None,
    };

    let ca_cert_path = tls_paths.as_ref().map(|(ca, _, _)| ca.clone());
    let client_profile_json = client_profile_json(plan, ca_cert_path.as_deref());

    Ok(Provisioned {
        app_toml_path,
        ca_cert_path,
        leaf_cert_path,
        systemd_unit_path,
        client_profile_json,
    })
}

/// Reject incoherent plans up front (fail closed before writing anything).
fn validate(plan: &Plan) -> Result<(), String> {
    if plan.tls && plan.hosts.is_empty() {
        return Err("--tls requires at least one --host (the cert's SAN)".into());
    }
    match plan.role {
        Role::Runtime if plan.link_backend_url.is_none() => {
            Err("the runtime role requires --link-backend-url (the backend it connects to)".into())
        }
        Role::Backend if plan.link_serve_token.is_none() => {
            Err("the backend role requires --link-token (the shared link secret)".into())
        }
        _ => Ok(()),
    }
}

fn write(path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Write a secret (private key) with 0600 permissions where the platform
/// supports it, so a provisioned key is not world-readable.
fn write_private(path: &Path, contents: &str) -> Result<(), String> {
    write(path, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod {}: {e}", path.display()))?;
    }
    Ok(())
}
