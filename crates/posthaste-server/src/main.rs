use std::path::PathBuf;

use posthaste_http_api_adapter::{write_discovery_file, ServerConfig};
use posthaste_server::start_server;

mod token_cli;

use token_cli::run_token_command;

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

    // Write the daemon discovery file `daemon.json` (`{ version, port, url, token }`,
    // 0600) so external clients (posthastectl) find the bound port + bearer token.
    // Only written when auth is enabled — we never persist an unused credential.
    // Best-effort; removed on clean shutdown via the M20 sequence below.
    let discovery_file = if handle.require_auth {
        write_discovery_file(handle.addr, &handle.auth_token)
    } else {
        None
    };

    if options.open {
        open_browser(&format!("http://{}", handle.addr));
    }

    // Serve until a shutdown signal (ctrl_c / SIGTERM), then run the ordered,
    // deadline-bounded teardown (D60/M20) in place of the old bare
    // `join_handle.await`. The discovery file (if any) is removed as the final
    // teardown step.
    let mut sequence = handle.into_shutdown_sequence();
    if let Some(path) = discovery_file {
        sequence = sequence.with_discovery_file(path);
    }
    sequence.run_until_signal().await;
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
