use std::net::TcpListener;
use std::path::PathBuf;

use crate::guard::TempDirGuard;

/// A disposable per-process temp directory under the system temp root (P6).
///
/// Returns a [`TempDirGuard`] — removed on drop, including during a
/// panicking unwind, so a failing test never leaves a `{prefix}-*` directory
/// behind in `$TMPDIR`. Keep the guard bound for as long as the directory
/// needs to exist; it derefs to `Path`, so `root.join(...)` and `&root` work
/// exactly like the `PathBuf` this used to return.
pub fn temp_root(prefix: &str) -> TempDirGuard {
    TempDirGuard::new(prefix)
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
