//! Binary entry point: config load → store → service → account runtimes →
//! API bind → connection-info file. Shutdown reverses it: remove the
//! connection info, stop runtimes, flush, close the store.

use posthaste_client_backend::{serve, AppState, BuildOptions, ConnectionInfo};

/// Parse `--port <n>` (a fixed dev port) from the process arguments.
/// Defaults to 0: an ephemeral port, resolved at bind and published via the
/// connection-info file.
fn parse_port_flag() -> Result<u16, String> {
    fn parse(value: &str) -> Result<u16, String> {
        value
            .parse::<u16>()
            .map_err(|_| format!("invalid --port value: {value}"))
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => Ok(0),
        [flag] if flag.starts_with("--port=") => parse(&flag["--port=".len()..]),
        [flag, value] if flag == "--port" => parse(value),
        _ => Err(format!("unknown arguments: {}", args.join(" "))),
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber_init();

    let port = match parse_port_flag() {
        Ok(port) => port,
        Err(message) => {
            eprintln!("{message}");
            eprintln!("usage: posthaste-client-backend [--port <n>]");
            std::process::exit(2);
        }
    };

    let state = match AppState::assemble(BuildOptions::default()).await {
        Ok(state) => state,
        Err(error) => {
            eprintln!("failed to start backend: {error}");
            std::process::exit(1);
        }
    };

    // The session token is minted before the bind so the router can enforce
    // it from the first request; the real port lands on the document after.
    let mut info = ConnectionInfo::generate(0);
    let mut server = match serve(state.clone(), port, info.token.clone()).await {
        Ok(server) => server,
        Err(error) => {
            eprintln!("failed to bind API port {port}: {error}");
            state.shutdown().await;
            std::process::exit(1);
        }
    };

    // The API is bound: publish the connection info (real port + the session
    // token) for local clients to discover.
    info.port = server.addr.port();
    let info_path = state.paths.connection_info_path();
    if let Err(error) = info.write(&info_path) {
        eprintln!(
            "failed to write connection info at {}: {error}",
            info_path.display()
        );
        state.shutdown().await;
        std::process::exit(1);
    }
    tracing::info!(addr = %server.addr, info = %info_path.display(), "backend serving");

    // Serve until interrupted, then run the ordered teardown: stop intake
    // first (remove the connection info, stop the server), then stop
    // runtimes and close the store — no request can reach a closed store.
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        () = server.wait() => {}
    }
    if let Err(error) = ConnectionInfo::remove(&info_path) {
        tracing::warn!(%error, "failed to remove connection info during shutdown");
    }
    server.abort();
    server.join().await;
    state.shutdown().await;
}

fn tracing_subscriber_init() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
