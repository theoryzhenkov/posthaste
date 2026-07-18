//! The tracing subscriber stack for the shell process: human-readable stderr
//! plus JSON lines with daily rotation under `<state_root>/logs/`, shared by
//! the embedded backend, the shell, and the forwarded frontend entries.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, Registry};

/// Initialize the subscriber stack. `RUST_LOG` overrides the default filter.
/// Returns the [`WorkerGuard`] that flushes pending file writes on drop; the
/// caller must hold it for the lifetime of the application.
pub(crate) fn init(state_root: &Path) -> WorkerGuard {
    let log_dir = state_root.join("logs");

    // EnvFilter is not Clone, so it is constructed separately per layer.
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_filter(env_filter());

    let file_appender = rolling::daily(&log_dir, "posthaste");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let json_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_target(true)
        .with_current_span(true)
        .with_span_list(true)
        .with_writer(non_blocking)
        .with_filter(env_filter());

    Registry::default()
        .with(stderr_layer)
        .with(json_layer)
        .init();

    guard
}

fn env_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(
            [
                "warn",
                "posthaste_client_desktop_lib=info",
                "posthaste_client_backend=info",
                "posthaste_engine=info",
                "posthaste_imap=info",
                "posthaste_store=info",
                "posthaste_domain_service=info",
                "posthaste_config=info",
                "frontend=info",
            ]
            .join(","),
        )
    })
}
