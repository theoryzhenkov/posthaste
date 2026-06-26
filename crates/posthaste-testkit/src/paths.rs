use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique per-process temp directory under the system temp root.
///
/// Combines a monotonic counter with a nanosecond timestamp so concurrent
/// tests in the same process never collide.
pub fn temp_root(prefix: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{now}-{seq}"))
}

/// Claims a free loopback TCP port by binding to `127.0.0.1:0` and reading
/// back the assigned port. The listener is dropped before returning, so the
/// port is immediately reusable by the fixture under test.
pub fn free_loopback_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("free loopback port")
        .local_addr()
        .expect("local addr")
        .port()
}

/// Resolves the `stalwart` binary path, overridable via `POSTHASTE_STALWART_BIN`.
pub fn stalwart_bin() -> String {
    std::env::var("POSTHASTE_STALWART_BIN").unwrap_or_else(|_| "stalwart".to_string())
}

/// The workspace root, derived from this crate's manifest dir
/// (`crates/posthaste-testkit` -> up two levels). Used to locate repo-relative
/// dev assets such as `tools/dev/stalwart/config.toml`.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root")
        .to_path_buf()
}
