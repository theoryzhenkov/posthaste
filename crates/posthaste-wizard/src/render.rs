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

/// The binary path this node runs (the installed `--exec`, or a sensible
/// default if unset).
fn exec_bin(plan: &Plan) -> String {
    plan.exec_path
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| format!("/usr/local/bin/{}", plan.role.binary()))
}

/// The full invocation (program + args). Only the all-in-one `posthaste_daemon`
/// is a multi-command binary needing the `serve` subcommand; the lean
/// backend/runtime daemons run directly. Shared by the systemd `ExecStart` and
/// the launchd `ProgramArguments`.
fn exec_invocation(plan: &Plan) -> Vec<String> {
    let bin = exec_bin(plan);
    match plan.role {
        Role::Daemon => vec![bin, "serve".to_string()],
        Role::Backend | Role::Runtime => vec![bin],
    }
}

fn role_description(plan: &Plan) -> &'static str {
    match plan.role {
        Role::Daemon => "Posthaste all-in-one daemon",
        Role::Backend => "Posthaste backend node",
        Role::Runtime => "Posthaste runtime node",
    }
}

/// The launchd job label / reverse-DNS id for a role, e.g. `com.posthaste.backend`.
pub fn launchd_label(role: Role) -> String {
    let suffix = match role {
        Role::Daemon => "daemon",
        Role::Backend => "backend",
        Role::Runtime => "runtime",
    };
    format!("com.posthaste.{suffix}")
}

/// Render a minimal systemd unit that runs the role binary against this config.
/// `service_user` is `Some` only for a system unit (`/etc/systemd/system`), where
/// the service must declare the user to run as; a `--user` unit inherits the
/// invoking user and leaves it `None`.
pub fn render_systemd_unit(plan: &Plan, service_user: Option<&str>) -> String {
    let exec = exec_invocation(plan).join(" ");
    let description = role_description(plan);
    // System units target multi-user.target and pin the run-as user; user units
    // target default.target and run as whoever owns the user manager.
    let (wanted_by, user_lines) = match service_user {
        Some(user) => ("multi-user.target", format!("User={user}\nGroup={user}\n")),
        None => ("default.target", String::new()),
    };
    format!(
        "[Unit]\n\
         Description={description}\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         {user_lines}\
         Environment=POSTHASTE_CONFIG_ROOT={config}\n\
         Environment=POSTHASTE_STATE_ROOT={state}\n\
         ExecStart={exec}\n\
         Restart=on-failure\n\
         RestartSec=2\n\
         \n\
         [Install]\n\
         WantedBy={wanted_by}\n",
        config = plan.config_root.display(),
        state = plan.state_root.display(),
    )
}

/// Render a launchd LaunchAgent plist (macOS) that keeps the role binary running
/// for the logged-in user. The macOS analogue of the `--user` systemd unit.
pub fn render_launchd_plist(plan: &Plan) -> String {
    let label = launchd_label(plan.role);
    let program_args = exec_invocation(plan)
        .iter()
        .map(|a| format!("    <string>{}</string>", xml_escape(a)))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \x20 <key>Label</key>\n\
         \x20 <string>{label}</string>\n\
         \x20 <key>ProgramArguments</key>\n\
         \x20 <array>\n\
         {program_args}\n\
         \x20 </array>\n\
         \x20 <key>EnvironmentVariables</key>\n\
         \x20 <dict>\n\
         \x20   <key>POSTHASTE_CONFIG_ROOT</key>\n\
         \x20   <string>{config}</string>\n\
         \x20   <key>POSTHASTE_STATE_ROOT</key>\n\
         \x20   <string>{state}</string>\n\
         \x20 </dict>\n\
         \x20 <key>RunAtLoad</key>\n\
         \x20 <true/>\n\
         \x20 <key>KeepAlive</key>\n\
         \x20 <true/>\n\
         </dict>\n\
         </plist>\n",
        config = xml_escape(&plan.config_root.display().to_string()),
        state = xml_escape(&plan.state_root.display().to_string()),
    )
}

/// Minimal XML escaping for plist string values (paths can contain `&`).
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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
