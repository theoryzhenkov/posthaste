use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) fn temp_root(prefix: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{now}-{seq}"))
}

pub(super) fn free_loopback_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("free loopback port")
        .local_addr()
        .expect("local addr")
        .port()
}

pub(super) fn stalwart_bin() -> String {
    std::env::var("POSTHASTE_STALWART_BIN").unwrap_or_else(|_| "stalwart".to_string())
}

pub(super) fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root")
        .to_path_buf()
}
