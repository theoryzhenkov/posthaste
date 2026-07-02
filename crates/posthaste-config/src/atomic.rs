use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use posthaste_domain_model::ConfigError;

/// Monotonic counter that makes each `atomic_write` temp path unique within a
/// process (the pid separates processes). Without it, concurrent writes shared a
/// fixed `app.toml.tmp`: one writer's `rename` won, the other's hit `ENOENT` →
/// `ConfigError::Io` → HTTP 500. With unique temp paths each writer renames its
/// OWN temp file, so both succeed (last-writer-wins) and no write 500s.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Writes `content` to `path` atomically via write-fsync-rename to prevent
/// corruption on crash. The temp file is uniquely named so concurrent writes to
/// the same `path` don't race on a shared temp file (last-writer-wins, no 500).
///
/// @spec docs/L1-accounts#atomic-writes
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<(), ConfigError> {
    let parent = path
        .parent()
        .ok_or_else(|| ConfigError::Io("cannot determine parent directory".to_string()))?;
    fs::create_dir_all(parent).map_err(io_error)?;

    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config");
    let temp_path = parent.join(format!("{file_name}.tmp.{}.{}", std::process::id(), unique));

    // Write fsync-rename. On any failure, best-effort remove our temp file so a
    // failed write doesn't leave an orphan (a successful rename consumes it).
    let result: Result<(), std::io::Error> = (|| {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(content)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp_path, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result.map_err(io_error)
}

/// Wraps an I/O error into `ConfigError::Io`.
fn io_error(err: std::io::Error) -> ConfigError {
    ConfigError::Io(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn concurrent_writes_to_same_path_all_succeed() {
        // Regression: a fixed temp path (`app.toml.tmp`) let concurrent writers
        // race on the rename — one won, the other hit ENOENT → ConfigError::Io
        // → HTTP 500 (~30% under a PATCH storm from the appearance sliders).
        // Unique temp paths make every write succeed (last-writer-wins).
        let dir = std::env::temp_dir().join(format!(
            "ph-atomic-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = Arc::new(dir.join("app.toml"));

        let n = 50;
        let mut handles = Vec::with_capacity(n);
        for i in 0..n {
            let path = Arc::clone(&path);
            handles.push(thread::spawn(move || {
                atomic_write(&path, format!("value = {i}\n").as_bytes())
            }));
        }
        for (i, handle) in handles.into_iter().enumerate() {
            assert!(
                handle.join().unwrap().is_ok(),
                "concurrent write {i} failed (temp-path race regression)"
            );
        }

        // The final content is one of the written values (last-writer-wins).
        let after = std::fs::read_to_string(&*path).unwrap();
        assert!(after.starts_with("value = "), "unexpected content: {after}");

        // No orphaned temp files remain (successful renames consumed them; the
        // best-effort cleanup drops any from a failed write).
        let orphans: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(orphans.is_empty(), "orphaned temp files: {orphans:?}");

        std::fs::remove_dir_all(&dir).ok();
    }
}
