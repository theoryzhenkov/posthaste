use std::fs;
use std::io::Write;
use std::path::Path;

use mail_domain::ConfigError;

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

fn io_error(err: std::io::Error) -> ConfigError {
    ConfigError::Io(err.to_string())
}
