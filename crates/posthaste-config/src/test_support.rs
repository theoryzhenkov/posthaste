//! The one reusable RAII tempdir guard for this crate's test suites (P6).
//!
//! `atomic::tests`, `repository::io::lossless_write_tests`, and
//! `repository::tests` each used to hand-roll their own `std::env::temp_dir()`
//! + pid/nanos/counter unique-naming helper and never clean up the directory.
//! All now share [`temp_root`] from here instead.
//!
//! Backed by [`tempfile::TempDir`] rather than reinventing unique-name
//! generation and recursive removal. [`TempDirGuard`] adds a
//! `Deref<Target = Path>` so the existing `let root = temp_root(); ...
//! root.join(...)` call sites need no further changes beyond the binding's
//! type going from `PathBuf` to this guard — the directory is removed when
//! `root` drops, including during a panicking unwind.

use std::ops::Deref;
use std::path::Path;

use tempfile::TempDir;

/// RAII guard for a disposable temp directory. See the module docs.
pub(crate) struct TempDirGuard(TempDir);

impl TempDirGuard {
    pub(crate) fn path(&self) -> &Path {
        self.0.path()
    }
}

impl Deref for TempDirGuard {
    type Target = Path;

    fn deref(&self) -> &Path {
        self.0.path()
    }
}

impl AsRef<Path> for TempDirGuard {
    fn as_ref(&self) -> &Path {
        self.0.path()
    }
}

/// A disposable per-test temp directory under the system temp root (P6).
/// Removed on drop, including a panicking unwind — keep the guard bound for
/// as long as the directory needs to exist.
pub(crate) fn temp_root() -> TempDirGuard {
    TempDirGuard(
        tempfile::Builder::new()
            .prefix("posthaste-config-test-")
            .tempdir()
            .expect("temp dir should be created"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic;

    #[test]
    fn directory_is_removed_on_drop() {
        let guard = temp_root();
        let path = guard.path().to_path_buf();
        assert!(path.is_dir());
        drop(guard);
        assert!(
            !path.exists(),
            "temp dir should be removed once its guard drops"
        );
    }

    #[test]
    fn directory_is_removed_even_when_a_panic_unwinds_through_it() {
        let guard = temp_root();
        let path = guard.path().to_path_buf();
        let result = panic::catch_unwind(move || {
            let _guard = guard;
            panic!("simulated test failure while the guard is in scope");
        });
        assert!(result.is_err(), "the closure should have panicked");
        assert!(
            !path.exists(),
            "temp dir should be removed even when the owning scope panics"
        );
    }
}
