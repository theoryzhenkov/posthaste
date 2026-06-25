/// Write `contents` to `path`, creating/truncating it with owner-only (`0600`)
/// permissions on unix. Used for the `daemon.json` port-file, which carries a
/// live bearer token. `fs::write` would NOT tighten an already world-readable
/// file, so this opens with restrictive mode and re-asserts `0600` to cover the
/// overwrite case. On non-unix platforms it falls back to a plain write
/// (filesystem ACLs are the protection there).
///
/// @spec docs/eph/DESIGN-L1-trust-model
pub fn write_secure_file(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // `mode` applies only when the file is newly created.
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    // Re-assert 0600 because `mode` above is ignored when the file already
    // existed (e.g. a prior, looser-permissioned daemon.json).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(contents)?;
    file.flush()
}
