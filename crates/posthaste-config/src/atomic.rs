use std::fs;
use std::io::Write;
use std::path::Path;

use posthaste_domain::ConfigError;

/// Writes `content` to `path` atomically via write-fsync-rename to prevent
/// corruption on crash.
///
/// @spec docs/L1-accounts#atomic-writes
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<(), ConfigError> {
    let parent = path
        .parent()
        .ok_or_else(|| ConfigError::Io("cannot determine parent directory".to_string()))?;
    fs::create_dir_all(parent).map_err(io_error)?;

    let temp_path = path.with_extension("toml.tmp");
    let mut file = fs::File::create(&temp_path).map_err(io_error)?;
    file.write_all(content).map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    drop(file);

    fs::rename(&temp_path, path).map_err(io_error)?;
    Ok(())
}

/// Wraps an I/O error into `ConfigError::Io`.
fn io_error(err: std::io::Error) -> ConfigError {
    ConfigError::Io(err.to_string())
}
