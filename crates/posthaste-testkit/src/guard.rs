//! The one reusable RAII tempdir guard (P6).
//!
//! Every test/harness site that used to hand-roll a unique directory under
//! `std::env::temp_dir()` and never clean it up routes through
//! [`TempDirGuard`] (via [`crate::temp_root`]) instead. It wraps
//! [`tempfile::TempDir`] — the actual unique-name generation and recursive
//! removal is `tempfile`'s, not reinvented here — and adds a `Deref<Target =
//! Path>` so call sites that used to hold a bare `PathBuf` (`root.join(...)`,
//! `&root`) need no further changes: only the binding's type changes, from
//! `PathBuf` to this guard.
//!
//! The directory is removed on drop, including during a panicking unwind:
//! `Drop::drop` still runs while a panic unwinds the stack, so a failing
//! assertion no longer leaves a `{prefix}-*` directory behind.

use std::ops::Deref;
use std::path::Path;

use tempfile::TempDir;

/// RAII guard for a disposable temp directory. See the module docs.
pub struct TempDirGuard(TempDir);

impl TempDirGuard {
    /// Creates a fresh temp directory named `{prefix}-<random>` under the
    /// system temp root.
    pub(crate) fn new(prefix: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix(&format!("{prefix}-"))
            .tempdir()
            .expect("temp dir should be created");
        Self(dir)
    }

    /// The directory's path. Prefer this over `&*guard` at call sites that
    /// already need an explicit `&Path`.
    pub fn path(&self) -> &Path {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic;

    #[test]
    fn directory_exists_while_the_guard_is_alive() {
        let guard = TempDirGuard::new("posthaste-testkit-guard-test");
        assert!(guard.path().is_dir());
    }

    #[test]
    fn directory_is_removed_on_drop() {
        let guard = TempDirGuard::new("posthaste-testkit-guard-test");
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
        let path = {
            let guard = TempDirGuard::new("posthaste-testkit-guard-test");
            let path = guard.path().to_path_buf();
            // Move the guard into the closure so it drops during unwind, the
            // same way a test's local `root` binding would on a failed
            // assertion.
            let result = panic::catch_unwind(move || {
                let _guard = guard;
                panic!("simulated test failure while the guard is in scope");
            });
            assert!(result.is_err(), "the closure should have panicked");
            path
        };
        assert!(
            !path.exists(),
            "temp dir should be removed even when the owning scope panics"
        );
    }
}
