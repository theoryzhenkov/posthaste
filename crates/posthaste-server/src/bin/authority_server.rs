//! `posthaste-authority-server`: the standalone authority server role binary (L5 §4).
//!
//! Builds the authority server far node (store + service + account supervisor) and serves
//! ONLY the authenticated runtime↔authority-server link — no `/v1` client API, no
//! renderer. A remote `posthaste-runtime` connects to it over the link.
//!
//! Config is the usual `app.toml` + `POSTHASTE_*` env (own `POSTHASTE_CONFIG_ROOT`
//! / `POSTHASTE_STATE_ROOT` per install); the link credential is `[link] token`
//! (`POSTHASTE_LINK_TOKEN`). Bind with `--bind` or `POSTHASTE_BIND`.

use posthaste_http_api_adapter::ServerConfig;
use posthaste_server::start_authority_server;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", usage());
        return;
    }

    let bind = parse_bind(&args).unwrap_or_else(|message| {
        eprintln!("{message}");
        eprintln!("{}", usage());
        std::process::exit(2);
    });

    let handle = start_authority_server(ServerConfig {
        bind_address_override: bind,
        ..ServerConfig::default()
    })
    .await;

    handle
        .join_handle
        .await
        .expect("posthaste-authority-server task panicked");
}

/// Parse the optional `--bind <addr>` flag; any other argument is an error.
fn parse_bind(args: &[String]) -> Result<Option<String>, String> {
    let mut bind = None;
    let mut index = 0;
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
            other => return Err(format!("unknown option: {other}")),
        }
        index += 1;
    }
    Ok(bind)
}

fn usage() -> &'static str {
    "usage: posthaste-authority-server [--bind 127.0.0.1:3002]"
}
