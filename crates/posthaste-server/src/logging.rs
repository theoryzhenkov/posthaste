use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, Registry};

/// Initialize the tracing subscriber stack.
///
/// - Stderr: human-readable with ANSI colors
/// - File: JSON lines, daily rotation, stored in `<state_root>/logs/`
/// - Filter: `RUST_LOG` env var takes precedence; falls back to `config_level`
///
/// Returns a [`WorkerGuard`] that flushes pending log writes on drop.
/// The caller must hold this guard for the lifetime of the application.
pub fn init(state_root: &Path, config_level: &str) -> WorkerGuard {
    let log_dir = state_root.join("logs");

    // EnvFilter is not Clone, so we construct it separately for each layer.
    let stderr_filter = env_filter(config_level);

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_filter(stderr_filter);

    let file_appender = rolling::daily(&log_dir, "posthaste");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let file_filter = env_filter(config_level);
    let json_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_target(true)
        .with_writer(non_blocking)
        .with_filter(file_filter);

    Registry::default()
        .with(stderr_layer)
        .with(json_layer)
        .init();

    guard
}

fn env_filter(config_level: &str) -> EnvFilter {
    EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_filter_directives(config_level)))
}

fn default_filter_directives(config_level: &str) -> String {
    let level = match config_level.trim().to_ascii_lowercase().as_str() {
        "error" | "warn" | "info" | "debug" | "trace" => config_level.trim().to_ascii_lowercase(),
        _ => "info".to_string(),
    };

    [
        "warn".to_string(),
        format!("posthaste_server={level}"),
        format!("posthaste_engine={level}"),
        format!("posthaste_imap={level}"),
        format!("posthaste_store={level}"),
        format!("posthaste_domain={level}"),
        "tower_http=info".to_string(),
    ]
    .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_filter_keeps_dependencies_quiet() {
        let directives = default_filter_directives("debug");

        assert!(directives.contains("warn,"));
        assert!(directives.contains("posthaste_server=debug"));
        assert!(!directives.contains("imap_next=debug"));
        assert!(!directives.contains("rustls=debug"));
    }

    #[test]
    fn default_filter_sanitizes_invalid_levels() {
        let directives = default_filter_directives("verbose");

        assert!(directives.contains("posthaste_server=info"));
    }
}
