use super::*;

pub(crate) fn shell_command(command: &str) -> ProcessCommand {
    #[cfg(windows)]
    {
        let mut process = ProcessCommand::new("cmd");
        process.args(["/C", command]);
        process
    }
    #[cfg(not(windows))]
    {
        let mut process = ProcessCommand::new("sh");
        process.args(["-c", command]);
        process
    }
}

pub(crate) fn configure_command_for_timeout(command: &mut ProcessCommand) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

pub(crate) fn terminate_timed_out_child(
    child: &mut Child,
    suite_id: &str,
) -> LabResult<ExitStatus> {
    #[cfg(unix)]
    {
        terminate_unix_process_group(child, suite_id)?;
    }
    #[cfg(not(unix))]
    {
        child.kill().map_err(|source| LabError::RunSuite {
            suite_id: suite_id.to_string(),
            action: "kill timed-out",
            source,
        })?;
    }
    child.wait().map_err(|source| LabError::RunSuite {
        suite_id: suite_id.to_string(),
        action: "wait for timed-out",
        source,
    })
}

#[cfg(unix)]
pub(crate) fn terminate_unix_process_group(child: &mut Child, suite_id: &str) -> LabResult<()> {
    let process_group = format!("-{}", child.id());
    let _ = ProcessCommand::new("kill")
        .args(["-TERM", "--", &process_group])
        .status();

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if child
            .try_wait()
            .map_err(|source| LabError::RunSuite {
                suite_id: suite_id.to_string(),
                action: "poll timed-out",
                source,
            })?
            .is_some()
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }

    let kill_status = ProcessCommand::new("kill")
        .args(["-KILL", "--", &process_group])
        .status();
    if kill_status.is_err() {
        child.kill().map_err(|source| LabError::RunSuite {
            suite_id: suite_id.to_string(),
            action: "kill timed-out",
            source,
        })?;
    }
    Ok(())
}

pub(crate) fn read_stream_in_background<R>(mut reader: R) -> thread::JoinHandle<io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut output = Vec::new();
        reader.read_to_end(&mut output)?;
        Ok(output)
    })
}

pub(crate) fn join_stream_reader(
    handle: thread::JoinHandle<io::Result<Vec<u8>>>,
) -> Result<Vec<u8>, ()> {
    handle.join().map_err(|_| ())?.map_err(|_| ())
}

pub(crate) fn write_bytes(path: &Path, bytes: &[u8]) -> LabResult<()> {
    fs::write(path, bytes).map_err(|source| LabError::WriteFile {
        path: path.display().to_string(),
        source,
    })
}

pub(crate) fn discover_suite_artifact_paths(
    stdout: &[u8],
    stderr: &[u8],
    run_dir: &Path,
) -> Vec<String> {
    let mut paths = Vec::new();
    for output in [stdout, stderr] {
        let text = String::from_utf8_lossy(output);
        for line in text.lines() {
            if let Some(raw_path) = line.strip_prefix(ARTIFACT_PATH_MARKER) {
                if let Some(path) = existing_report_artifact_path(raw_path, run_dir) {
                    if !paths.contains(&path) {
                        paths.push(path);
                    }
                }
            }
        }
    }
    paths
}

pub(crate) fn existing_report_artifact_path(raw_path: &str, run_dir: &Path) -> Option<String> {
    if raw_path.is_empty() || raw_path.contains('\0') {
        return None;
    }
    let path = Path::new(raw_path);
    if !path.exists() {
        return None;
    }
    let canonical_path = path.canonicalize().ok()?;
    if !is_allowed_report_artifact_path(&canonical_path, run_dir) {
        return None;
    }
    Some(canonical_path.display().to_string())
}

pub(crate) fn is_allowed_report_artifact_path(canonical_path: &Path, run_dir: &Path) -> bool {
    run_dir
        .canonicalize()
        .ok()
        .is_some_and(|canonical_run_dir| canonical_path.starts_with(canonical_run_dir))
        && !has_secret_like_path_segment(canonical_path)
}

pub(crate) fn has_secret_like_path_segment(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(is_secret_env_name)
    })
}
