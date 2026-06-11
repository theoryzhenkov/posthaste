use super::*;

pub(crate) fn path_safe_suite_id(id: &str) -> String {
    id.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub(crate) fn exit_status_parts(status: &ExitStatus) -> (Option<i32>, Option<i32>) {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        (status.code(), status.signal())
    }
    #[cfg(not(unix))]
    {
        (status.code(), None)
    }
}

pub(crate) fn suite_status(
    timed_out: bool,
    exit_code: Option<i32>,
    signal: Option<i32>,
) -> (LabStatus, String) {
    if timed_out {
        return (LabStatus::Failed, "suite command timed out".to_string());
    }
    match exit_code {
        Some(0) => (
            LabStatus::Passed,
            "suite command exited successfully".to_string(),
        ),
        Some(77) => (
            LabStatus::Skipped,
            "suite command reported skipped status 77".to_string(),
        ),
        Some(78) => (
            LabStatus::Blocked,
            "suite command reported blocked status 78".to_string(),
        ),
        Some(code) => (
            LabStatus::Failed,
            format!("suite command exited with status {code}"),
        ),
        None => match signal {
            Some(signal) => (
                LabStatus::Failed,
                format!("suite command terminated by signal {signal}"),
            ),
            None => (
                LabStatus::Failed,
                "suite command ended without an exit status".to_string(),
            ),
        },
    }
}

pub(crate) fn aggregate_status(results: &[SuiteExecutionRecord]) -> LabStatus {
    if results
        .iter()
        .any(|result| result.status == LabStatus::Failed)
    {
        return LabStatus::Failed;
    }
    if results
        .iter()
        .any(|result| result.status == LabStatus::Blocked)
    {
        return LabStatus::Blocked;
    }
    if results
        .iter()
        .all(|result| result.status == LabStatus::Skipped)
    {
        return LabStatus::Skipped;
    }
    LabStatus::Passed
}

pub(crate) fn summary_reason(status: LabStatus, results: &[SuiteExecutionRecord]) -> String {
    match status {
        LabStatus::Passed => "all selected suites passed".to_string(),
        LabStatus::Failed | LabStatus::Blocked => results
            .iter()
            .find(|result| result.status == status)
            .map(|result| result.reason.clone())
            .unwrap_or_else(|| format!("verification ended with status {status:?}")),
        LabStatus::Skipped => "all selected suites were skipped".to_string(),
    }
}

pub(crate) fn important_log_excerpts(results: &[SuiteExecutionRecord]) -> Vec<String> {
    let mut excerpts = Vec::new();
    for result in results
        .iter()
        .filter(|result| result.status != LabStatus::Passed)
    {
        if let Some(excerpt) = tail_text_file(Path::new(&result.stderr_path), 20) {
            excerpts.push(format!("{} stderr:\n{}", result.suite_id, excerpt));
        }
        if let Some(excerpt) = tail_text_file(Path::new(&result.stdout_path), 20) {
            excerpts.push(format!("{} stdout:\n{}", result.suite_id, excerpt));
        }
        for artifact_path in &result.artifact_paths {
            let path = Path::new(artifact_path);
            if is_summary_artifact_path(path) {
                if let Some(excerpt) = summary_artifact_excerpt(path) {
                    excerpts.push(format!(
                        "{} nested summary {}:\n{}",
                        result.suite_id, artifact_path, excerpt
                    ));
                }
            }
            if is_log_excerpt_path(path) {
                if let Some(excerpt) = tail_text_file(path, 30) {
                    excerpts.push(format!(
                        "{} artifact log {}:\n{}",
                        result.suite_id, artifact_path, excerpt
                    ));
                }
            }
        }
    }
    excerpts
}

pub(crate) fn is_summary_artifact_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "summary.json")
}

pub(crate) fn is_log_excerpt_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "log" | "txt"))
}

pub(crate) fn summary_artifact_excerpt(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let mut lines = Vec::new();
    for key in ["status", "reason", "exitCode", "runDir"] {
        if let Some(value) = value.get(key) {
            lines.push(format!("{}: {}", key, summary_value(value)));
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

pub(crate) fn summary_value(value: &serde_json::Value) -> String {
    let text = value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string());
    redact_excerpt_text(&text)
}

pub(crate) fn tail_text_file(path: &Path, max_lines: usize) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let lines = text.lines().rev().take(max_lines).collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }
    Some(redact_excerpt_text(
        &lines.into_iter().rev().collect::<Vec<_>>().join("\n"),
    ))
}

pub(crate) fn redact_excerpt_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            if contains_secret_marker(line) {
                REDACTED
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn contains_secret_marker(text: &str) -> bool {
    let uppercase = text.to_ascii_uppercase();
    SECRET_MARKERS
        .iter()
        .any(|marker| uppercase.contains(marker))
}

pub(crate) fn create_dir(path: &Path) -> LabResult<()> {
    fs::create_dir_all(path).map_err(|source| LabError::CreateDir {
        path: path.display().to_string(),
        source,
    })
}

pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> LabResult<()> {
    let text = serde_json::to_string_pretty(value)?;
    fs::write(path, format!("{text}\n")).map_err(|source| LabError::WriteFile {
        path: path.display().to_string(),
        source,
    })
}

pub(crate) fn new_run_id() -> String {
    format!(
        "{}-{}",
        OffsetDateTime::now_utc().unix_timestamp(),
        Uuid::new_v4().simple()
    )
}
