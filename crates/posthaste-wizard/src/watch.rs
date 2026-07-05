//! `posthaste-wizard ctl register-watch` / `unregister-watch` (RFC-L2-scripting
//! rulings 12 & 19): wrap a long-running `posthastectl` tap consumer in a
//! **user** service unit for always-on laptop automation — the shape between
//! the ad-hoc foreground `watch --exec` and the in-app rules engine.
//!
//! Two variants, one machinery:
//!   * `--exec <script> [filters]` wraps `posthastectl watch` (ruling 12/19 —
//!     the pull-based, NAT-friendly edge consumer).
//!   * `--serve-hook <script> [--port N]` wraps `posthastectl hook serve` (ruling
//!     17 — the localhost webhook receiver for GUI/`rules.toml` webhook rules).
//!
//! The unit points at the right `posthastectl` (manifest entry → install dir →
//! `PATH`), restarts on failure with a modest backoff, and passes discovery
//! environment through so the wrapped CLI finds `daemon.json`. `unregister-watch`
//! tears one down; `ctl status` lists them.
//!
//! **Consent (ruling 20b):** registering runs local code in response to
//! server-controlled events, so it prints a one-time warning and requires an
//! explicit confirm (or `--yes`). [`confirm_consent`] gates it.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use crate::install::{
    launch_agents_dir_pub, start_service, stop_service, user_unit_dir, ServiceScope,
};
use crate::manifest::Manifest;
use crate::render::xml_escape;

/// The wrapped `posthastectl` command a registered unit runs.
pub enum WatchCommand {
    /// `posthastectl watch --exec <script> [filters]`.
    Watch {
        exec: String,
        topic: Option<String>,
        rule: Option<String>,
        keyword: Option<String>,
        account: Option<String>,
    },
    /// `posthastectl hook serve --exec <script> [--port N]`.
    HookServe { exec: String, port: Option<u16> },
}

impl WatchCommand {
    /// A default unit-name suffix derived from the most specific selector, so
    /// `--name` is optional: the rule, else the keyword, else the topic, else
    /// `watch`/`hook`.
    pub fn default_name(&self) -> String {
        match self {
            WatchCommand::Watch {
                rule,
                keyword,
                topic,
                ..
            } => rule
                .clone()
                .or_else(|| keyword.clone())
                .or_else(|| topic.clone())
                .unwrap_or_else(|| "watch".to_string()),
            WatchCommand::HookServe { .. } => "hook".to_string(),
        }
    }

    /// The `posthastectl` argv this command runs (program first).
    pub fn argv(&self, ctl_path: &str) -> Vec<String> {
        let mut v = vec![ctl_path.to_string()];
        match self {
            WatchCommand::Watch {
                exec,
                topic,
                rule,
                keyword,
                account,
            } => {
                v.push("watch".into());
                v.push("--exec".into());
                v.push(exec.clone());
                push_opt(&mut v, "--topic", topic);
                push_opt(&mut v, "--rule", rule);
                push_opt(&mut v, "--keyword", keyword);
                push_opt(&mut v, "--account", account);
            }
            WatchCommand::HookServe { exec, port } => {
                v.push("hook".into());
                v.push("serve".into());
                v.push("--exec".into());
                v.push(exec.clone());
                if let Some(p) = port {
                    v.push("--port".into());
                    v.push(p.to_string());
                }
            }
        }
        v
    }
}

fn push_opt(v: &mut Vec<String>, flag: &str, value: &Option<String>) {
    if let Some(value) = value {
        v.push(flag.into());
        v.push(value.clone());
    }
}

/// Sanitize a user-supplied unit suffix to `[A-Za-z0-9._-]` (systemd/launchd
/// safe), collapsing anything else to `-`.
pub fn sanitize_name(raw: &str) -> String {
    let s: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "watch".to_string()
    } else {
        s
    }
}

/// The systemd unit file name for a registered watch, e.g.
/// `posthaste-watch-<name>.service`.
pub fn systemd_unit_name(name: &str) -> String {
    format!("posthaste-watch-{name}.service")
}

/// The launchd label / plist stem for a registered watch, e.g.
/// `com.posthaste.watch.<name>`.
pub fn launchd_label(name: &str) -> String {
    format!("com.posthaste.watch.{name}")
}

/// Locate the `posthastectl` a registered unit should run: the manifest's `ctl`
/// entry path (authoritative — what the wizard installed), else
/// `<bin_dir>/posthastectl` if present, else the bare name (resolved off `PATH`
/// at run time).
pub fn locate_ctl(manifest: &Manifest, bin_dir: &Path) -> String {
    if let Some(c) = manifest.get(crate::ctl_binary_name()) {
        return c.path.clone();
    }
    let in_bin = bin_dir.join(crate::ctl_binary_name());
    if in_bin.is_file() {
        return in_bin.display().to_string();
    }
    crate::ctl_binary_name().to_string()
}

/// The discovery env vars to pass through into the unit, read from the
/// registering process's environment — so a non-default state root or an
/// explicit API URL/token carries into the always-on service (the CLI still
/// auto-discovers `daemon.json` when none is set).
pub fn discovery_env() -> Vec<(String, String)> {
    [
        "POSTHASTE_STATE_ROOT",
        "POSTHASTE_API_URL",
        "POSTHASTE_TOKEN",
    ]
    .iter()
    .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
    .collect()
}

/// Render a `systemd --user` unit wrapping the `posthastectl` argv, restarting
/// on failure with a modest backoff.
pub fn render_watch_systemd(argv: &[String], name: &str, env: &[(String, String)]) -> String {
    let exec = argv
        .iter()
        .map(|a| shell_quote_systemd(a))
        .collect::<Vec<_>>()
        .join(" ");
    let env_lines: String = env
        .iter()
        .map(|(k, v)| format!("Environment={k}={v}\n"))
        .collect();
    format!(
        "[Unit]\n\
         Description=Posthaste registered watch ({name})\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         {env_lines}\
         ExecStart={exec}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

/// Render a launchd LaunchAgent wrapping the argv, restarting on failure.
pub fn render_watch_launchd(argv: &[String], name: &str, env: &[(String, String)]) -> String {
    let program_args = argv
        .iter()
        .map(|a| format!("    <string>{}</string>", xml_escape(a)))
        .collect::<Vec<_>>()
        .join("\n");
    let env_dict = if env.is_empty() {
        String::new()
    } else {
        let inner: String = env
            .iter()
            .map(|(k, v)| {
                format!(
                    "\x20   <key>{}</key>\n\x20   <string>{}</string>\n",
                    xml_escape(k),
                    xml_escape(v)
                )
            })
            .collect();
        format!("\x20 <key>EnvironmentVariables</key>\n\x20 <dict>\n{inner}\x20 </dict>\n")
    };
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
         {env_dict}\
         \x20 <key>RunAtLoad</key>\n\
         \x20 <true/>\n\
         \x20 <key>KeepAlive</key>\n\
         \x20 <dict>\n\
         \x20   <key>SuccessfulExit</key>\n\
         \x20   <false/>\n\
         \x20 </dict>\n\
         \x20 <key>ThrottleInterval</key>\n\
         \x20 <integer>5</integer>\n\
         </dict>\n\
         </plist>\n",
        label = launchd_label(name),
    )
}

/// Quote a systemd `ExecStart` token: wrap in double quotes if it contains
/// whitespace (systemd honors double-quoted argv words), escaping any embedded
/// quotes/backslashes.
fn shell_quote_systemd(arg: &str) -> String {
    if arg.chars().any(|c| c.is_whitespace() || c == '"') {
        let escaped = arg.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        arg.to_string()
    }
}

/// The file the register step writes, plus its contents, for a host's init
/// system. `None` for an unsupported host.
pub fn watch_unit_file(
    scope: ServiceScope,
    argv: &[String],
    name: &str,
    env: &[(String, String)],
) -> Result<(PathBuf, String), String> {
    match scope {
        ServiceScope::UserSystemd | ServiceScope::SystemSystemd => {
            // A registered watch is always a *user* service (never system) — it
            // runs the user's handler with the user's discovery.
            let dir = user_unit_dir()?;
            Ok((
                dir.join(systemd_unit_name(name)),
                render_watch_systemd(argv, name, env),
            ))
        }
        ServiceScope::Launchd => {
            let dir = launch_agents_dir_pub()?;
            Ok((
                dir.join(format!("{}.plist", launchd_label(name))),
                render_watch_launchd(argv, name, env),
            ))
        }
        ServiceScope::None => {
            Err("no supported user init system (systemd --user or launchd) on this host".into())
        }
    }
}

/// Write + enable the registered watch unit. Refuses (never sudo) if the unit
/// dir is not writable. Returns the path written.
pub fn register_watch(
    scope: ServiceScope,
    argv: &[String],
    name: &str,
    env: &[(String, String)],
) -> Result<PathBuf, String> {
    let (path, body) = watch_unit_file(scope, argv, name, env)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "the unit directory {} is not writable ({e}); the wizard never uses sudo — \
                 create it or fix its ownership and re-run",
                parent.display()
            )
        })?;
    }
    std::fs::write(&path, body).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            format!(
                "refusing to write {} ({e}); the wizard never escalates to sudo",
                path.display()
            )
        } else {
            format!("write {}: {e}", path.display())
        }
    })?;
    enable_watch(scope, name, &path);
    Ok(path)
}

/// Enable + start the just-written unit (best-effort; the file is written
/// regardless, so a manager-absent host can start it by hand).
fn enable_watch(scope: ServiceScope, name: &str, path: &Path) {
    match scope {
        ServiceScope::UserSystemd | ServiceScope::SystemSystemd => {
            let unit = systemd_unit_name(name);
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .output();
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "enable", "--now", &unit])
                .output();
        }
        ServiceScope::Launchd => {
            let _ = start_service(ServiceScope::Launchd, &path.display().to_string());
        }
        ServiceScope::None => {}
    }
}

/// Stop, disable, and remove a registered watch unit. Returns the removed path.
pub fn unregister_watch(scope: ServiceScope, name: &str) -> Result<PathBuf, String> {
    match scope {
        ServiceScope::UserSystemd | ServiceScope::SystemSystemd => {
            let unit = systemd_unit_name(name);
            let path = user_unit_dir()?.join(&unit);
            if !path.exists() {
                return Err(format!(
                    "no registered watch named `{name}` at {}",
                    path.display()
                ));
            }
            let _ = stop_service(ServiceScope::UserSystemd, &unit);
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "disable", &unit])
                .output();
            std::fs::remove_file(&path).map_err(|e| format!("remove {}: {e}", path.display()))?;
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .output();
            Ok(path)
        }
        ServiceScope::Launchd => {
            let path = launch_agents_dir_pub()?.join(format!("{}.plist", launchd_label(name)));
            if !path.exists() {
                return Err(format!(
                    "no registered watch named `{name}` at {}",
                    path.display()
                ));
            }
            let _ = stop_service(ServiceScope::Launchd, &path.display().to_string());
            std::fs::remove_file(&path).map_err(|e| format!("remove {}: {e}", path.display()))?;
            Ok(path)
        }
        ServiceScope::None => Err("no supported user init system on this host".into()),
    }
}

/// List the names of currently registered watches (scanning the unit dir for
/// the `posthaste-watch-*` / `com.posthaste.watch.*` naming), for `ctl status`.
pub fn list_watches(scope: ServiceScope) -> Vec<String> {
    let (dir, prefix, suffix) = match scope {
        ServiceScope::UserSystemd | ServiceScope::SystemSystemd => (
            user_unit_dir(),
            "posthaste-watch-".to_string(),
            ".service".to_string(),
        ),
        ServiceScope::Launchd => (
            launch_agents_dir_pub(),
            "com.posthaste.watch.".to_string(),
            ".plist".to_string(),
        ),
        ServiceScope::None => return Vec::new(),
    };
    let Ok(dir) = dir else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for e in entries.flatten() {
        let file = e.file_name();
        let file = file.to_string_lossy();
        if let Some(rest) = file.strip_prefix(&prefix) {
            if let Some(name) = rest.strip_suffix(&suffix) {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    names
}

// -- Consent (ruling 20b) ------------------------------------------------

/// The one-time consent warning shown before a watch is registered.
pub const CONSENT_WARNING: &str = "\
CONSENT — registering a watch runs LOCAL CODE on your machine in response to
server-controlled events (incoming mail / rule firings). The handler receives
attacker-influenced input (email is untrusted). Only register a handler you
trust, and scope the rule/watch by sender. See docs/scripting-security.md.";

/// Print the consent warning and require an explicit `yes`. `assume_yes`
/// (the `--yes` flag) short-circuits to `true` without prompting. Any other
/// answer declines. Returns whether to proceed.
pub fn confirm_consent<R: BufRead, W: Write>(
    input: &mut R,
    out: &mut W,
    assume_yes: bool,
) -> Result<bool, String> {
    writeln!(out, "{CONSENT_WARNING}").map_err(io_err)?;
    if assume_yes {
        writeln!(out, "\n(--yes) proceeding.").map_err(io_err)?;
        return Ok(true);
    }
    write!(out, "\nType 'yes' to register this watch: ").map_err(io_err)?;
    out.flush().map_err(io_err)?;
    let mut line = String::new();
    let n = input.read_line(&mut line).map_err(io_err)?;
    if n == 0 {
        return Ok(false);
    }
    Ok(line.trim().eq_ignore_ascii_case("yes"))
}

fn io_err(e: std::io::Error) -> String {
    format!("io: {e}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn watch_cmd() -> WatchCommand {
        WatchCommand::Watch {
            exec: "sh ./handler.sh".into(),
            topic: Some("rule.fired".into()),
            rule: Some("flag-for-edge".into()),
            keyword: None,
            account: None,
        }
    }

    #[test]
    fn watch_argv_carries_flags_and_default_name_prefers_rule() {
        let cmd = watch_cmd();
        assert_eq!(cmd.default_name(), "flag-for-edge");
        let argv = cmd.argv("/home/u/.local/bin/posthastectl");
        assert_eq!(argv[0], "/home/u/.local/bin/posthastectl");
        assert_eq!(argv[1], "watch");
        assert!(argv.contains(&"--exec".to_string()));
        assert!(argv.contains(&"sh ./handler.sh".to_string()));
        assert!(argv.contains(&"--topic".to_string()));
        assert!(argv.contains(&"rule.fired".to_string()));
        assert!(argv.contains(&"--rule".to_string()));
    }

    #[test]
    fn hook_serve_argv_and_name() {
        let cmd = WatchCommand::HookServe {
            exec: "./h.sh".into(),
            port: Some(8787),
        };
        assert_eq!(cmd.default_name(), "hook");
        let argv = cmd.argv("posthastectl");
        assert_eq!(
            argv,
            vec![
                "posthastectl",
                "hook",
                "serve",
                "--exec",
                "./h.sh",
                "--port",
                "8787"
            ]
        );
    }

    #[test]
    fn systemd_unit_snapshot() {
        let argv = watch_cmd().argv("/opt/bin/posthastectl");
        let env = vec![("POSTHASTE_STATE_ROOT".to_string(), "/srv/state".to_string())];
        let unit = render_watch_systemd(&argv, "flag-for-edge", &env);
        assert!(unit.contains("Description=Posthaste registered watch (flag-for-edge)"));
        // The exec script has a space and must be quoted in ExecStart.
        assert!(unit.contains("ExecStart=/opt/bin/posthastectl watch --exec \"sh ./handler.sh\""));
        assert!(unit.contains("--topic rule.fired"));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("RestartSec=5"));
        assert!(unit.contains("Environment=POSTHASTE_STATE_ROOT=/srv/state"));
        assert!(unit.contains("WantedBy=default.target"));
    }

    #[test]
    fn launchd_unit_snapshot() {
        let argv = watch_cmd().argv("/opt/bin/posthastectl");
        let plist = render_watch_launchd(&argv, "flag-for-edge", &[]);
        assert!(plist.contains("<string>com.posthaste.watch.flag-for-edge</string>"));
        assert!(plist.contains("<string>/opt/bin/posthastectl</string>"));
        assert!(plist.contains("<string>watch</string>"));
        assert!(plist.contains("<string>sh ./handler.sh</string>")); // one argv element, not split
        assert!(plist.contains("<key>KeepAlive</key>"));
        assert!(plist.contains("<key>SuccessfulExit</key>"));
    }

    #[test]
    fn sanitize_name_is_unit_safe() {
        assert_eq!(sanitize_name("flag/for edge!"), "flag-for-edge");
        assert_eq!(sanitize_name("  ---  "), "watch");
        assert_eq!(sanitize_name("ok.name_1"), "ok.name_1");
    }

    #[test]
    fn locate_ctl_prefers_manifest_then_bin_then_bare() {
        use crate::manifest::Component;
        let dir = tempfile::tempdir().unwrap();
        // 1) manifest entry wins.
        let mut m = Manifest::default();
        m.record(Component {
            component: "posthastectl".into(),
            kind: "ctl".into(),
            path: "/from/manifest/posthastectl".into(),
            version: "1".into(),
            channel: "nightly".into(),
            installed_at: "t".into(),
            service: None,
            unit: None,
            previous_version: None,
        });
        assert_eq!(locate_ctl(&m, dir.path()), "/from/manifest/posthastectl");
        // 2) no manifest: an existing bin_dir/posthastectl.
        let empty = Manifest::default();
        let bin = dir.path().join(crate::ctl_binary_name());
        std::fs::write(&bin, b"x").unwrap();
        assert_eq!(locate_ctl(&empty, dir.path()), bin.display().to_string());
        // 3) nothing: the bare name.
        let empty_dir = tempfile::tempdir().unwrap();
        assert_eq!(
            locate_ctl(&empty, empty_dir.path()),
            crate::ctl_binary_name()
        );
    }

    #[test]
    fn consent_requires_explicit_yes() {
        // --yes short-circuits.
        let mut out = Vec::new();
        assert!(confirm_consent(&mut Cursor::new(""), &mut out, true).unwrap());
        assert!(String::from_utf8_lossy(&out).contains("CONSENT"));

        // Typing "yes" proceeds.
        let mut out = Vec::new();
        assert!(confirm_consent(&mut Cursor::new("yes\n"), &mut out, false).unwrap());

        // Anything else declines.
        let mut out = Vec::new();
        assert!(!confirm_consent(&mut Cursor::new("no\n"), &mut out, false).unwrap());
        let mut out = Vec::new();
        assert!(!confirm_consent(&mut Cursor::new("\n"), &mut out, false).unwrap());
        // EOF declines (never registers without a clear yes).
        let mut out = Vec::new();
        assert!(!confirm_consent(&mut Cursor::new(""), &mut out, false).unwrap());
    }
}
