//! Guided install: an interactive prompt flow that fills an install plan by
//! asking, so a user need not know the flags up front. `main` wires real
//! stdin/stdout; the flow takes a generic reader + writer so the whole
//! question sequence is unit-testable with scripted input.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use crate::fetch::{Channel, Version};
use crate::install::ServiceScope;
use crate::{Plan, Role};

/// Everything `main` needs to run an install, gathered interactively. `join` is
/// applied by the caller (as with the `--join` flag) so a runtime node's link
/// fields come from the pasted string.
pub struct GuidedInstall {
    pub plan: Plan,
    pub version: Version,
    pub service: ServiceScope,
    pub bin_dir: PathBuf,
    pub join: Option<String>,
}

/// Run the guided prompt flow against `input`/`out`, returning a ready install.
/// Any read error or an aborted confirmation returns `Err`.
pub fn guided_install<R: BufRead, W: Write>(
    input: &mut R,
    out: &mut W,
) -> Result<GuidedInstall, String> {
    let home = std::env::var("HOME").unwrap_or_default();

    writeln!(out, "Posthaste install wizard — guided setup.\n").map_err(io_err)?;

    let role = ask_role(input, out)?;
    writeln!(out).map_err(io_err)?;

    let config_root = ask(
        input,
        out,
        "Config directory",
        Some(&join_home(&home, ".config/posthaste")),
    )?;
    let state_root = ask(
        input,
        out,
        "State directory",
        Some(&join_home(&home, ".local/share/posthaste")),
    )?;
    let bind = ask(input, out, "Bind address", Some(default_bind(role)))?;

    let tls = ask_yes_no(input, out, "Enable TLS (recommended for remote)?", false)?;
    let mut hosts = Vec::new();
    if tls {
        let raw = ask(
            input,
            out,
            "Hostnames/IPs this node is reached as (comma-separated)",
            None,
        )?;
        hosts = raw
            .split(',')
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty())
            .collect();
        if hosts.is_empty() {
            return Err("TLS needs at least one hostname".into());
        }
    }

    // Role-specific link wiring.
    let mut link_serve_token = None;
    let mut link_token = None;
    let mut link_authority_server_url = None;
    let mut join = None;
    match role {
        Role::AuthorityServer => {
            let token = ask_secret_or_generate(input, out, "Link token (blank to generate)")?;
            link_serve_token = Some(token.clone());
            link_token = Some(token);
        }
        Role::Runtime => {
            if ask_yes_no(
                input,
                out,
                "Do you have a join string from the authority server?",
                true,
            )? {
                join = Some(ask(input, out, "Join string", None)?);
            } else {
                link_authority_server_url = Some(ask(
                    input,
                    out,
                    "Authority server URL (e.g. https://authority-server:3002)",
                    None,
                )?);
                let token = ask(input, out, "Link token", None)?;
                link_token = Some(token);
            }
        }
        Role::Daemon => {}
    }

    let version = match ask(input, out, "Release version (blank = latest nightly)", None)? {
        v if v.is_empty() => Version::Channel(Channel::Nightly),
        v => Version::Pinned(v),
    };

    let bin_dir = PathBuf::from(ask(
        input,
        out,
        "Install directory",
        Some(&join_home(&home, ".local/bin")),
    )?);

    let service = ask_service(input, out)?;

    let plan = Plan {
        role,
        config_root: PathBuf::from(config_root),
        state_root: PathBuf::from(state_root),
        bind,
        tls,
        hosts,
        link_serve_token,
        link_authority_server_url,
        link_token,
        exec_path: None,
        systemd_unit_path: None,
    };

    // Summary + confirm before doing anything.
    write_summary(out, &plan, &service, join.is_some())?;
    if !ask_yes_no(input, out, "Proceed?", true)? {
        return Err("aborted".into());
    }

    Ok(GuidedInstall {
        plan,
        version,
        service,
        bin_dir,
        join,
    })
}

fn ask_role<R: BufRead, W: Write>(input: &mut R, out: &mut W) -> Result<Role, String> {
    let roles = [
        (Role::Daemon, "daemon — all-in-one on one machine"),
        (Role::AuthorityServer, "authority-server — far node, serves the link only"),
        (Role::Runtime, "runtime — near node over a remote authority server"),
    ];
    writeln!(out, "Which role is this node?").map_err(io_err)?;
    for (i, (_, desc)) in roles.iter().enumerate() {
        writeln!(out, "  {}) {desc}", i + 1).map_err(io_err)?;
    }
    loop {
        let line = ask(input, out, "Choice", Some("1"))?;
        match line.parse::<usize>() {
            Ok(n) if (1..=roles.len()).contains(&n) => return Ok(roles[n - 1].0),
            _ => writeln!(out, "  enter 1, 2, or 3").map_err(io_err)?,
        }
    }
}

fn ask_service<R: BufRead, W: Write>(input: &mut R, out: &mut W) -> Result<ServiceScope, String> {
    if !ask_yes_no(
        input,
        out,
        "Register a background service to keep it running?",
        true,
    )? {
        return Ok(ServiceScope::None);
    }
    // launchd on macOS has no user/system split here; only Linux offers it.
    if cfg!(target_os = "macos") {
        return Ok(ServiceScope::Launchd);
    }
    if ask_yes_no(
        input,
        out,
        "System-wide (survives logout; needs sudo to register)?",
        false,
    )? {
        Ok(ServiceScope::SystemSystemd)
    } else {
        Ok(ServiceScope::UserSystemd)
    }
}

fn write_summary<W: Write>(
    out: &mut W,
    plan: &Plan,
    service: &ServiceScope,
    has_join: bool,
) -> Result<(), String> {
    writeln!(out, "\n— Summary —").map_err(io_err)?;
    writeln!(out, "  role:    {}", plan.role.binary()).map_err(io_err)?;
    writeln!(out, "  config:  {}", plan.config_root.display()).map_err(io_err)?;
    writeln!(out, "  state:   {}", plan.state_root.display()).map_err(io_err)?;
    writeln!(out, "  bind:    {}", plan.bind).map_err(io_err)?;
    writeln!(
        out,
        "  tls:     {}",
        if plan.tls {
            format!("yes ({})", plan.hosts.join(", "))
        } else {
            "no".into()
        }
    )
    .map_err(io_err)?;
    if has_join {
        writeln!(out, "  link:    from join string").map_err(io_err)?;
    } else if let Some(url) = &plan.link_authority_server_url {
        writeln!(out, "  authority-server: {url}").map_err(io_err)?;
    }
    let svc = match service {
        ServiceScope::UserSystemd => "systemctl --user",
        ServiceScope::SystemSystemd => "systemctl (system, sudo)",
        ServiceScope::Launchd => "launchd",
        ServiceScope::None => "none",
    };
    writeln!(out, "  service: {svc}").map_err(io_err)?;
    Ok(())
}

/// Prompt `question` (showing `default` in brackets) and read a trimmed line;
/// an empty answer falls back to `default`.
fn ask<R: BufRead, W: Write>(
    input: &mut R,
    out: &mut W,
    question: &str,
    default: Option<&str>,
) -> Result<String, String> {
    match default {
        Some(d) => write!(out, "{question} [{d}]: ").map_err(io_err)?,
        None => write!(out, "{question}: ").map_err(io_err)?,
    }
    out.flush().map_err(io_err)?;
    let mut line = String::new();
    let n = input.read_line(&mut line).map_err(io_err)?;
    if n == 0 {
        return Err("unexpected end of input".into());
    }
    let line = line.trim();
    if line.is_empty() {
        Ok(default.unwrap_or("").to_string())
    } else {
        Ok(line.to_string())
    }
}

fn ask_yes_no<R: BufRead, W: Write>(
    input: &mut R,
    out: &mut W,
    question: &str,
    default_yes: bool,
) -> Result<bool, String> {
    let hint = if default_yes { "Y/n" } else { "y/N" };
    loop {
        let ans = ask(input, out, &format!("{question} ({hint})"), None)?;
        match ans.to_ascii_lowercase().as_str() {
            "" => return Ok(default_yes),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => writeln!(out, "  please answer y or n").map_err(io_err)?,
        }
    }
}

/// Ask for a secret; a blank answer generates a random one and echoes it so the
/// operator can copy it to the peer node.
fn ask_secret_or_generate<R: BufRead, W: Write>(
    input: &mut R,
    out: &mut W,
    question: &str,
) -> Result<String, String> {
    let answer = ask(input, out, question, None)?;
    if !answer.is_empty() {
        return Ok(answer);
    }
    let token = random_token()?;
    writeln!(out, "  generated link token: {token}").map_err(io_err)?;
    Ok(token)
}

/// A 24-byte random token, hex-encoded. Reads the OS CSPRNG directly to avoid a
/// dependency; the self-host target is unix, where `/dev/urandom` is standard.
fn random_token() -> Result<String, String> {
    #[cfg(unix)]
    {
        use std::io::Read;
        let mut f =
            std::fs::File::open("/dev/urandom").map_err(|e| format!("open /dev/urandom: {e}"))?;
        let mut buf = [0u8; 24];
        f.read_exact(&mut buf)
            .map_err(|e| format!("read /dev/urandom: {e}"))?;
        Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
    }
    #[cfg(not(unix))]
    {
        Err("cannot generate a token on this platform; enter one explicitly".into())
    }
}

fn default_bind(role: Role) -> &'static str {
    match role {
        // A far node listens broadly; the all-in-one/near node default to loopback.
        Role::AuthorityServer => "0.0.0.0:3002",
        Role::Daemon | Role::Runtime => "127.0.0.1:3001",
    }
}

fn join_home(home: &str, rest: &str) -> String {
    if home.is_empty() {
        rest.to_string()
    } else {
        format!("{home}/{rest}")
    }
}

fn io_err(e: std::io::Error) -> String {
    format!("io: {e}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Drive the flow with scripted answers (one per prompt line).
    fn run(script: &str) -> Result<GuidedInstall, String> {
        let mut input = Cursor::new(script.to_string());
        let mut out = Vec::new();
        guided_install(&mut input, &mut out)
    }

    #[test]
    fn guides_a_authority_server_install_with_generated_token() {
        std::env::set_var("HOME", "/home/tester");
        // role=2 (authority server), config(default), state(default), bind(default),
        // tls=y, hosts, token(blank→generate), version(blank), bin(default),
        // service=y, system=n, proceed=y
        let script = "2\n\n\n\ny\nauthority-server.lan\n\n\n\ny\nn\ny\n";
        let g = run(script).expect("guided authority server install");
        assert_eq!(g.plan.role, Role::AuthorityServer);
        assert_eq!(
            g.plan.config_root,
            PathBuf::from("/home/tester/.config/posthaste")
        );
        assert_eq!(g.plan.bind, "0.0.0.0:3002");
        assert!(g.plan.tls);
        assert_eq!(g.plan.hosts, vec!["authority-server.lan".to_string()]);
        // Blank token → generated 24-byte hex (48 chars).
        let token = g.plan.link_serve_token.unwrap();
        assert_eq!(token.len(), 48);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(g.service, ServiceScope::UserSystemd);
        assert!(matches!(g.version, Version::Channel(Channel::Nightly)));
    }

    #[test]
    fn guides_a_runtime_install_via_join() {
        std::env::set_var("HOME", "/home/tester");
        // role=3 (runtime), config, state, bind, tls=n, have-join=y, join,
        // version, bin, service=n, proceed=y
        let script = "3\n\n\n\nn\ny\nDEADBEEFJOIN\n\n\nn\ny\n";
        let g = run(script).expect("guided runtime install");
        assert_eq!(g.plan.role, Role::Runtime);
        assert!(!g.plan.tls);
        assert_eq!(g.join.as_deref(), Some("DEADBEEFJOIN"));
        assert_eq!(g.service, ServiceScope::None);
    }

    #[test]
    fn abort_at_confirmation_is_an_error() {
        std::env::set_var("HOME", "/home/tester");
        // daemon, defaults, tls=n, version, bin, service=n, proceed=n
        let script = "1\n\n\n\nn\n\n\nn\nn\n";
        assert!(run(script).is_err());
    }

    #[test]
    fn eof_mid_flow_errors_cleanly() {
        std::env::set_var("HOME", "/home/tester");
        assert!(run("").is_err());
    }
}
