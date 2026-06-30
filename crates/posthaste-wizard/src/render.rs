//! Rendering: turn a [`Plan`] into the three artifacts a node needs — its
//! `app.toml`, a systemd service unit, and the client connection profile. The
//! TOML keys mirror the daemon's own schema (`[daemon]`/`[tls]`/`[link]`); a
//! round-trip test reads the output back through `posthaste-config` to keep them
//! in lock-step.

use std::path::Path;

use serde::Serialize;

use crate::{Plan, Role};

#[derive(Serialize)]
struct AppToml<'a> {
    schema_version: u32,
    daemon: DaemonSection<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tls: Option<TlsSection<'a>>,
    #[serde(skip_serializing_if = "LinkSection::is_empty")]
    link: LinkSection<'a>,
}

#[derive(Serialize)]
struct DaemonSection<'a> {
    bind: &'a str,
    require_auth: bool,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    allowed_hosts: &'a [String],
}

#[derive(Serialize)]
struct TlsSection<'a> {
    cert: &'a Path,
    key: &'a Path,
}

#[derive(Serialize, Default)]
struct LinkSection<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    serve: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    backend_url: Option<&'a str>,
}

impl LinkSection<'_> {
    fn is_empty(&self) -> bool {
        self.serve.is_none() && self.token.is_none() && self.backend_url.is_none()
    }
}

/// Render the node's `app.toml`. `cert`/`key` are the leaf paths when TLS is on.
pub fn render_app_toml(plan: &Plan, cert: Option<&Path>, key: Option<&Path>) -> String {
    let link = match plan.role {
        Role::Backend => LinkSection {
            serve: Some(true),
            token: plan.link_serve_token.as_deref(),
            backend_url: None,
        },
        Role::Runtime => LinkSection {
            serve: None,
            token: plan.link_token.as_deref(),
            backend_url: plan.link_backend_url.as_deref(),
        },
        Role::Daemon => LinkSection::default(),
    };

    let doc = AppToml {
        schema_version: 1,
        daemon: DaemonSection {
            bind: &plan.bind,
            require_auth: true,
            allowed_hosts: &plan.hosts,
        },
        tls: match (cert, key) {
            (Some(cert), Some(key)) => Some(TlsSection { cert, key }),
            _ => None,
        },
        link,
    };

    // toml requires scalar fields before tables; the derive order already does
    // that (schema_version, then [daemon]/[tls]/[link]).
    let mut header = String::from(
        "# Provisioned by posthaste-wizard. Edit through the app's settings or\n\
         # re-run the wizard; the [tls] paths and [link] secret are load-bearing.\n\n",
    );
    header.push_str(&toml::to_string_pretty(&doc).expect("app.toml serialization is infallible"));
    header
}

/// Render a minimal systemd unit that runs the role binary against this config.
pub fn render_systemd_unit(plan: &Plan) -> String {
    let exec_bin = plan
        .exec_path
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| format!("/usr/local/bin/{}", plan.role.binary()));
    // Only the all-in-one `posthaste_daemon` is a multi-command binary; it needs
    // the `serve` subcommand. The lean backend/runtime daemons run directly.
    let exec = match plan.role {
        Role::Daemon => format!("{exec_bin} serve"),
        Role::Backend | Role::Runtime => exec_bin,
    };
    let description = match plan.role {
        Role::Daemon => "Posthaste all-in-one daemon",
        Role::Backend => "Posthaste backend node",
        Role::Runtime => "Posthaste runtime node",
    };
    format!(
        "[Unit]\n\
         Description={description}\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         Environment=POSTHASTE_CONFIG_ROOT={config}\n\
         Environment=POSTHASTE_STATE_ROOT={state}\n\
         ExecStart={exec}\n\
         Restart=on-failure\n\
         RestartSec=2\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        config = plan.config_root.display(),
        state = plan.state_root.display(),
    )
}

/// The connection profile a client uses to reach this node — the three-mode
/// `remote` profile shape (`connection/resolve.ts`). The bearer token is handed
/// out separately (it is minted by the daemon on first start, then attenuated
/// per client), so it is intentionally absent here.
pub fn client_profile_json(plan: &Plan, ca_cert: Option<&Path>) -> String {
    let scheme = if plan.tls { "https" } else { "http" };
    // Prefer a named host (TLS cert SAN / Host allowlist) over the bind address,
    // which may be a wildcard like 0.0.0.0.
    let authority = plan
        .hosts
        .first()
        .cloned()
        .map(|h| format!("{h}:{}", port_of(&plan.bind)))
        .unwrap_or_else(|| plan.bind.clone());
    let base_url = format!("{scheme}://{authority}/v1");

    let profile = Profile {
        mode: "remote",
        base_url,
        host_header: plan.hosts.first().cloned(),
        ca_cert_path: ca_cert.map(|p| p.display().to_string()),
    };
    serde_json::to_string_pretty(&profile).expect("profile serialization is infallible")
}

fn port_of(bind: &str) -> &str {
    bind.rsplit(':').next().unwrap_or(bind)
}

#[derive(Serialize)]
struct Profile<'a> {
    mode: &'a str,
    #[serde(rename = "baseUrl")]
    base_url: String,
    #[serde(rename = "hostHeader", skip_serializing_if = "Option::is_none")]
    host_header: Option<String>,
    #[serde(rename = "caCertPath", skip_serializing_if = "Option::is_none")]
    ca_cert_path: Option<String>,
}
