use std::net::SocketAddr;
use std::path::PathBuf;

use posthaste_server::config::resolve_roots;
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

/// Write `<state_root>/daemon.json` = `{ port, token }`, the documented
/// discovery mechanism for external clients. The file carries a live
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
    let body = serde_json::json!({ "port": addr.port(), "token": token });
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
