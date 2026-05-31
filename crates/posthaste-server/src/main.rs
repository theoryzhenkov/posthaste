use std::io::Read;
use std::net::SocketAddr;
use std::path::PathBuf;

use posthaste_server::config::resolve_roots;
use posthaste_server::token::attenuate;
use posthaste_server::{start_server, write_secure_file, ServerConfig};

struct ServeOptions {
    bind: Option<String>,
    frontend_dist: Option<PathBuf>,
    api_only: bool,
    open: bool,
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // The `token` command group is synchronous and key-free (client-side
    // attenuation). Dispatch it before the serve path so `--help` for `token`
    // is handled there.
    if args.first().map(String::as_str) == Some("token") {
        std::process::exit(run_token_command(&args[1..]));
    }

    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", usage());
        return;
    }

    let options = parse_args(args).unwrap_or_else(|message| {
        eprintln!("{message}");
        eprintln!("{}", usage());
        std::process::exit(2);
    });

    let frontend_dist = if options.api_only {
        None
    } else {
        Some(
            resolve_frontend_dist(options.frontend_dist).unwrap_or_else(|message| {
                eprintln!("{message}");
                std::process::exit(2);
            }),
        )
    };

    let handle = start_server(ServerConfig {
        bind_address_override: options.bind,
        frontend_dist,
        ..ServerConfig::default()
    })
    .await;

    // Write the daemon discovery port-file `{ port, token }` (0600) so external
    // clients can find the bound port and bearer token. Only written when auth
    // is enabled — we never persist an unused credential to disk. Best-effort:
    // a failure here must not bring the daemon down.
    if handle.require_auth {
        write_port_file(handle.addr, &handle.auth_token);
    }

    if options.open {
        open_browser(&format!("http://{}", handle.addr));
    }
    handle
        .join_handle
        .await
        .expect("posthaste server task panicked");
}

fn parse_args(args: Vec<String>) -> Result<ServeOptions, String> {
    let Some(command) = args.first() else {
        return Err("missing command".to_string());
    };
    if command != "serve" {
        return Err(format!("unknown command: {command}"));
    }

    let mut bind = None;
    let mut frontend_dist = None;
    let mut api_only = false;
    let mut open = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--bind" => {
                index += 1;
                bind = Some(
                    args.get(index)
                        .ok_or_else(|| "--bind requires an address".to_string())?
                        .clone(),
                );
            }
            "--frontend-dist" => {
                index += 1;
                frontend_dist =
                    Some(PathBuf::from(args.get(index).ok_or_else(|| {
                        "--frontend-dist requires a directory".to_string()
                    })?));
            }
            "--api-only" => api_only = true,
            "--open" => open = true,
            other => return Err(format!("unknown option: {other}")),
        }
        index += 1;
    }

    Ok(ServeOptions {
        bind,
        frontend_dist,
        api_only,
        open,
    })
}

/// Write `<state_root>/daemon.json` = `{ version, port, token }`, the documented
/// discovery mechanism for external clients. `version` is the schema version
/// (currently `1`); readers tolerate unknown fields so the schema can grow
/// (e.g. a macaroon id or tailnet hostname) without breaking older clients.
/// The file carries a live
/// credential, so it is created with mode `0600` on unix (and the state dir
/// best-effort `0700`). `fs::write` would NOT tighten an already
/// world-readable file, so we open with explicit restrictive permissions and
/// truncate. Overwrites any prior file. Best-effort: logs on failure, never
/// panics.
///
/// @spec docs/eph/DESIGN-L1-trust-model
fn write_port_file(addr: SocketAddr, token: &str) {
    let roots = resolve_roots();
    let path = roots.state_root.join("daemon.json");
    let body = serde_json::json!({ "version": 1, "port": addr.port(), "token": token });
    let contents = match serde_json::to_string_pretty(&body) {
        Ok(contents) => contents,
        Err(error) => {
            eprintln!("failed to serialize daemon.json: {error}");
            return;
        }
    };
    if let Err(error) = std::fs::create_dir_all(&roots.state_root) {
        eprintln!("failed to create state root for daemon.json: {error}");
        return;
    }
    // Best-effort tighten the state dir to 0700 on unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&roots.state_root, std::fs::Permissions::from_mode(0o700));
    }

    if let Err(error) = write_secure_file(&path, contents.as_bytes()) {
        eprintln!("failed to write daemon.json at {}: {error}", path.display());
    }
}

fn resolve_frontend_dist(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    let candidate = explicit
        .or_else(|| {
            std::env::var("POSTHASTE_FRONTEND_DIST")
                .ok()
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from("apps/web/dist"));

    let index = candidate.join("index.html");
    if !index.is_file() {
        return Err(format!(
            "frontend distribution is missing index.html: {}",
            candidate.display()
        ));
    }

    Ok(candidate)
}

fn usage() -> &'static str {
    "usage: posthaste serve [--api-only] [--open] [--bind 127.0.0.1:3001] [--frontend-dist apps/web/dist]"
}

fn token_usage() -> &'static str {
    "usage: posthaste token attenuate [--token <macaroon>] \
     [--action read,tag] [--account <id>] [--mailbox <id>] [--message <id>] \
     [--expires <rfc3339|duration like 1h>]\n\
     \n  The source macaroon is read from --token, else stdin, else daemon.json.\
     \n  Caveats are appended client-side (no root key). Prints the attenuated token."
}

/// Run the `token` command group. Returns a process exit code.
fn run_token_command(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("attenuate") => run_token_attenuate(&args[1..]),
        Some("--help") | Some("-h") => {
            println!("{}", token_usage());
            0
        }
        Some(other) => {
            eprintln!("unknown token subcommand: {other}");
            eprintln!("{}", token_usage());
            2
        }
        None => {
            eprintln!("missing token subcommand");
            eprintln!("{}", token_usage());
            2
        }
    }
}

/// Append caveats from flags to a source macaroon and print the result.
fn run_token_attenuate(args: &[String]) -> i32 {
    let mut token: Option<String> = None;
    let mut predicates: Vec<String> = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = || {
            args.get(index + 1)
                .cloned()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        let result = match flag {
            "--token" => value().map(|v| token = Some(v)),
            "--action" => value().and_then(|v| {
                parse_action_list(&v).map(|verbs| predicates.push(format!("action = {verbs}")))
            }),
            "--account" => value().map(|v| predicates.push(format!("account = {v}"))),
            "--mailbox" => value().map(|v| predicates.push(format!("mailbox = {v}"))),
            "--message" => value().map(|v| predicates.push(format!("message = {v}"))),
            "--expires" => value().and_then(|v| {
                parse_expires(&v).map(|ts| predicates.push(format!("expires = {ts}")))
            }),
            "--help" | "-h" => {
                println!("{}", token_usage());
                return 0;
            }
            other => Err(format!("unknown option: {other}")),
        };
        if let Err(message) = result {
            eprintln!("{message}");
            eprintln!("{}", token_usage());
            return 2;
        }
        index += 2;
    }

    let source = match token
        .or_else(read_token_from_stdin)
        .or_else(read_token_from_daemon_file)
    {
        Some(source) => source,
        None => {
            eprintln!(
                "no source macaroon: pass --token, pipe one on stdin, or run where daemon.json exists"
            );
            return 2;
        }
    };

    let mut current = source.trim().to_string();
    for predicate in &predicates {
        match attenuate(&current, predicate) {
            Ok(next) => current = next,
            Err(_) => {
                eprintln!("input is not a valid macaroon");
                return 2;
            }
        }
    }
    println!("{current}");
    0
}

/// Validate and normalize a comma-separated action list against the verb set.
fn parse_action_list(value: &str) -> Result<String, String> {
    const VERBS: &[&str] = &["read", "send", "tag", "move", "delete", "manage"];
    let mut verbs = Vec::new();
    for raw in value.split(',') {
        let verb = raw.trim();
        if !VERBS.contains(&verb) {
            return Err(format!(
                "unknown action verb: {verb} (expected one of {})",
                VERBS.join(", ")
            ));
        }
        verbs.push(verb.to_string());
    }
    if verbs.is_empty() {
        return Err("--action requires at least one verb".to_string());
    }
    Ok(verbs.join(","))
}

/// Parse `--expires` as either an RFC3339 timestamp (passed through) or a short
/// duration like `1h`/`30m`/`45s`/`7d`, converted to an absolute RFC3339 UTC
/// instant from now.
fn parse_expires(value: &str) -> Result<String, String> {
    use time::format_description::well_known::Rfc3339;
    use time::{Duration, OffsetDateTime};

    let value = value.trim();
    if let Ok(parsed) = OffsetDateTime::parse(value, &Rfc3339) {
        return parsed
            .format(&Rfc3339)
            .map_err(|error| format!("failed to format expiry: {error}"));
    }
    // Duration form: <number><unit> where unit ∈ s/m/h/d.
    let (num, unit) = value.split_at(
        value
            .find(|c: char| !c.is_ascii_digit())
            .ok_or_else(|| format!("invalid --expires value: {value}"))?,
    );
    let amount: i64 = num
        .parse()
        .map_err(|_| format!("invalid --expires duration: {value}"))?;
    let delta = match unit {
        "s" => Duration::seconds(amount),
        "m" => Duration::minutes(amount),
        "h" => Duration::hours(amount),
        "d" => Duration::days(amount),
        other => return Err(format!("unknown --expires unit: {other} (use s/m/h/d)")),
    };
    (OffsetDateTime::now_utc() + delta)
        .format(&Rfc3339)
        .map_err(|error| format!("failed to format expiry: {error}"))
}

/// Read a macaroon from stdin when it is not a TTY (i.e. something was piped).
/// Returns `None` for an empty read so the daemon.json fallback can run.
fn read_token_from_stdin() -> Option<String> {
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        return None;
    }
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_ok() {
        let trimmed = buf.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Read the full-scope macaroon from `<state_root>/daemon.json`
/// (`{version, port, token}`); only `token` is consumed here.
fn read_token_from_daemon_file() -> Option<String> {
    let path = resolve_roots().state_root.join("daemon.json");
    let contents = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&contents).ok()?;
    json.get("token")?.as_str().map(str::to_string)
}

fn open_browser(url: &str) {
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };

    if let Err(error) = result {
        eprintln!("failed to open browser: {error}");
    }
}
