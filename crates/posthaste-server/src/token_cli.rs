use std::io::Read;

use posthaste_http_api_adapter::config::resolve_roots;
use posthaste_http_api_adapter::token::attenuate;

fn token_usage() -> &'static str {
    "usage: posthaste token attenuate [--token <macaroon>] \
     [--action read,tag] [--account <id>] [--mailbox <id>] [--message <id>] \
     [--expires <rfc3339|duration like 1h>]\n\
     \n  The source macaroon is read from --token, else stdin, else daemon.json.\
     \n  Caveats are appended client-side (no root key). Prints the attenuated token."
}

/// Run the `token` command group. Returns a process exit code.
pub(crate) fn run_token_command(args: &[String]) -> i32 {
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
