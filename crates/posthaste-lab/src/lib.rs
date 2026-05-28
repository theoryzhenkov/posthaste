use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

const DEFAULT_SUITE_TIMEOUT_SECONDS: u64 = 300;
const ARTIFACT_PATH_MARKER: &str = "POSTHASTE_LAB_ARTIFACT_PATH=";
const REDACTED: &str = "<redacted>";
const KNOWN_ID_TYPES: &[&str] = &[
    "suite", "runner", "profile", "fixture", "artifact", "log", "state", "cmd",
];
const SECRET_MARKERS: &[&str] = &[
    "PASSWORD",
    "PASSWD",
    "TOKEN",
    "SECRET",
    "KEY",
    "CREDENTIAL",
    "AUTH",
    "COOKIE",
    "SESSION",
];

pub type LabResult<T> = Result<T, LabError>;

#[derive(Debug, Error)]
pub enum LabError {
    #[error("failed to read {path}: {source}")]
    ReadFile { path: String, source: io::Error },
    #[error("failed to write {path}: {source}")]
    WriteFile { path: String, source: io::Error },
    #[error("failed to create directory {path}: {source}")]
    CreateDir { path: String, source: io::Error },
    #[error("failed to spawn suite {suite_id}: {source}")]
    SpawnSuite { suite_id: String, source: io::Error },
    #[error("failed to {action} suite {suite_id}: {source}")]
    RunSuite {
        suite_id: String,
        action: &'static str,
        source: io::Error,
    },
    #[error("failed to capture {stream} for suite {suite_id}")]
    CaptureSuiteStream {
        suite_id: String,
        stream: &'static str,
    },
    #[error("failed to parse registry TOML: {0}")]
    ParseToml(#[from] toml::de::Error),
    #[error("failed to serialize lab artifact JSON: {0}")]
    SerializeJson(#[from] serde_json::Error),
    #[error("registry is missing top-level [suite] tables")]
    MissingSuiteTable,
    #[error("suite table {id} does not contain suite fields")]
    EmptySuiteTable { id: String },
    #[error("invalid lab id {id:?}: {reason}")]
    InvalidLabId { id: String, reason: String },
    #[error("suite {0} was not found in the registry")]
    SuiteNotFound(String),
    #[error("no suites matched the requested selection")]
    NoSuitesSelected,
    #[error("changed-file suite selection is not implemented")]
    ChangedSelectionUnsupported,
    #[error("usage error: {0}")]
    Usage(String),
    #[error("verification failed; summary: {summary_path}")]
    VerificationFailed { summary_path: String },
    #[error("verification blocked; summary: {summary_path}")]
    VerificationBlocked { summary_path: String },
    #[error("verification skipped; summary: {summary_path}")]
    VerificationSkipped { summary_path: String },
}

impl LabError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::VerificationFailed { .. } => 1,
            Self::VerificationSkipped { .. } => 77,
            Self::VerificationBlocked { .. } => 78,
            _ => 2,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SuiteEntry {
    pub level: String,
    pub targets: Vec<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub fixture: Option<String>,
    #[serde(default)]
    pub runners: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    pub command: String,
    #[serde(default, alias = "timeout_seconds")]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub risk: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuiteRegistry {
    suites: BTreeMap<String, SuiteEntry>,
}

impl SuiteRegistry {
    pub fn load(path: impl AsRef<Path>) -> LabResult<Self> {
        let path = path.as_ref();
        let text = fs::read_to_string(path).map_err(|source| LabError::ReadFile {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_toml_str(&text)
    }

    pub fn from_toml_str(text: &str) -> LabResult<Self> {
        let value = text.parse::<toml::Value>()?;
        let table = value.as_table().ok_or(LabError::MissingSuiteTable)?;
        let suite_value = table.get("suite").ok_or(LabError::MissingSuiteTable)?;
        let suite_table = suite_value.as_table().ok_or(LabError::MissingSuiteTable)?;

        let mut suites = BTreeMap::new();
        for (name, value) in suite_table {
            let nested = value.as_table().ok_or_else(|| LabError::EmptySuiteTable {
                id: format!("suite.{name}"),
            })?;
            flatten_suite_table(&format!("suite.{name}"), nested, &mut suites)?;
        }

        Ok(Self { suites })
    }

    pub fn suites(&self) -> &BTreeMap<String, SuiteEntry> {
        &self.suites
    }

    pub fn select(&self, criteria: &SelectionCriteria) -> LabResult<Vec<SelectedSuite>> {
        if criteria.changed {
            return Err(LabError::ChangedSelectionUnsupported);
        }

        let candidates: Vec<(&String, &SuiteEntry)> = if let Some(id) = &criteria.suite_id {
            validate_lab_id_with_type(id, Some("suite"))?;
            let entry = self
                .suites
                .get(id)
                .ok_or_else(|| LabError::SuiteNotFound(id.clone()))?;
            vec![(id, entry)]
        } else {
            self.suites.iter().collect()
        };

        Ok(candidates
            .into_iter()
            .filter(|(_, entry)| criteria.tags.iter().all(|tag| entry.tags.contains(tag)))
            .filter(|(_, entry)| {
                criteria
                    .targets
                    .iter()
                    .all(|target| entry.targets.contains(target))
            })
            .map(|(id, entry)| SelectedSuite::from_entry(id, entry))
            .collect())
    }
}

fn flatten_suite_table(
    id: &str,
    table: &toml::map::Map<String, toml::Value>,
    suites: &mut BTreeMap<String, SuiteEntry>,
) -> LabResult<()> {
    if is_suite_leaf(table) {
        validate_lab_id_with_type(id, Some("suite"))?;
        let entry: SuiteEntry = toml::Value::Table(table.clone()).try_into()?;
        validate_suite_entry_refs(&entry)?;
        suites.insert(id.to_string(), entry);
        return Ok(());
    }

    if table.is_empty() {
        return Err(LabError::EmptySuiteTable { id: id.to_string() });
    }

    for (name, value) in table {
        let nested = value.as_table().ok_or_else(|| LabError::EmptySuiteTable {
            id: format!("{id}.{name}"),
        })?;
        flatten_suite_table(&format!("{id}.{name}"), nested, suites)?;
    }

    Ok(())
}

fn is_suite_leaf(table: &toml::map::Map<String, toml::Value>) -> bool {
    table.contains_key("level") || table.contains_key("command") || table.contains_key("targets")
}

fn validate_suite_entry_refs(entry: &SuiteEntry) -> LabResult<()> {
    if let Some(profile) = &entry.profile {
        validate_lab_id_with_type(profile, Some("profile"))?;
    }
    if let Some(fixture) = &entry.fixture {
        validate_lab_id_with_type(fixture, Some("fixture"))?;
    }
    for runner in &entry.runners {
        validate_lab_id_with_type(runner, Some("runner"))?;
    }
    for artifact in &entry.artifacts {
        validate_lab_id(artifact)?;
    }
    Ok(())
}

pub fn validate_lab_id(id: &str) -> LabResult<()> {
    validate_lab_id_with_type(id, None)
}

fn validate_lab_id_with_type(id: &str, expected_type: Option<&str>) -> LabResult<()> {
    if id.is_empty() {
        return Err(invalid_id(id, "id is empty"));
    }
    if !id.contains('.') {
        return Err(invalid_id(
            id,
            "id must include a type and name separated by '.'",
        ));
    }
    if id.starts_with('.') || id.ends_with('.') || id.contains("..") {
        return Err(invalid_id(id, "id has an empty dotted segment"));
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | ':' | '_' | '-'))
    {
        return Err(invalid_id(
            id,
            "id contains unsupported characters; allowed: letters, digits, '.', ':', '_', '-'",
        ));
    }
    for segment in id.split('.') {
        if segment.is_empty() {
            return Err(invalid_id(id, "id has an empty dotted segment"));
        }
        if segment.starts_with(':') || segment.ends_with(':') || segment.contains("::") {
            return Err(invalid_id(id, "id has an invalid ':' segment"));
        }
    }

    let first_segment = id.split('.').next().expect("id contains '.'");
    let id_type = first_segment.split(':').next().unwrap_or(first_segment);
    if !KNOWN_ID_TYPES.contains(&id_type) {
        return Err(invalid_id(id, "id type is not a known lab prefix"));
    }
    if let Some(expected_type) = expected_type {
        if id_type != expected_type {
            return Err(invalid_id(
                id,
                format!("expected {expected_type} id, found {id_type}"),
            ));
        }
    }
    Ok(())
}

fn invalid_id(id: &str, reason: impl Into<String>) -> LabError {
    LabError::InvalidLabId {
        id: id.to_string(),
        reason: reason.into(),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionCriteria {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suite_id: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub changed: bool,
}

impl SelectionCriteria {
    pub fn rationale(&self) -> String {
        if self.changed {
            return "changed-file selection requested".to_string();
        }

        let mut parts = Vec::new();
        if let Some(id) = &self.suite_id {
            parts.push(format!("explicit suite {id}"));
        }
        if !self.tags.is_empty() {
            parts.push(format!("tags {}", self.tags.join(",")));
        }
        if !self.targets.is_empty() {
            parts.push(format!("targets {}", self.targets.join(",")));
        }
        if parts.is_empty() {
            "all registered suites".to_string()
        } else {
            parts.join(" AND ")
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedSuite {
    pub id: String,
    pub level: String,
    pub targets: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixture: Option<String>,
    pub runners: Vec<String>,
    pub tags: Vec<String>,
    pub paths: Vec<String>,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    pub artifacts: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
}

impl SelectedSuite {
    fn from_entry(id: &str, entry: &SuiteEntry) -> Self {
        Self {
            id: id.to_string(),
            level: entry.level.clone(),
            targets: entry.targets.clone(),
            profile: entry.profile.clone(),
            fixture: entry.fixture.clone(),
            runners: entry.runners.clone(),
            tags: entry.tags.clone(),
            paths: entry.paths.clone(),
            command: entry.command.clone(),
            timeout_seconds: entry.timeout_seconds,
            artifacts: entry.artifacts.clone(),
            risk: entry.risk.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct VerifyOptions {
    pub run_root: PathBuf,
    pub registry_path: PathBuf,
    pub argv: Vec<String>,
    pub criteria: SelectionCriteria,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyOutput {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub summary_path: PathBuf,
    pub selected_suite_count: usize,
    pub status: LabStatus,
}

pub fn write_verify_run(
    registry: &SuiteRegistry,
    options: VerifyOptions,
) -> LabResult<VerifyOutput> {
    write_verify_run_with_env(registry, options, std::env::vars())
}

pub fn write_verify_run_with_env<I, K, V>(
    registry: &SuiteRegistry,
    options: VerifyOptions,
    env: I,
) -> LabResult<VerifyOutput>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    let selected_suites = registry.select(&options.criteria)?;
    if selected_suites.is_empty() {
        return Err(LabError::NoSuitesSelected);
    }

    let run_id = new_run_id();
    let run_dir = options.run_root.join(&run_id);
    let config_dir = run_dir.join("state.config");
    let data_dir = run_dir.join("state.data");
    let secrets_dir = run_dir.join("state.secrets");
    let artifact_dir = run_dir.join("artifacts");
    create_dir(&run_dir)?;
    create_dir(&config_dir)?;
    create_dir(&data_dir)?;
    create_dir(&secrets_dir)?;
    create_dir(&artifact_dir)?;

    let profiles = selected_suites
        .iter()
        .filter_map(|suite| suite.profile.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let fixtures = selected_suites
        .iter()
        .filter_map(|suite| suite.fixture.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let declared_artifacts = selected_suites
        .iter()
        .flat_map(|suite| suite.artifacts.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let reproduction_command = reproduction_command(&options.argv);

    let mut suite_results = Vec::new();
    for suite in &selected_suites {
        suite_results.push(execute_suite(
            suite,
            &run_dir,
            &artifact_dir,
            &config_dir,
            &data_dir,
            &secrets_dir,
        )?);
    }

    let status = aggregate_status(&suite_results);
    let first_failure = suite_results
        .iter()
        .find(|result| result.status != LabStatus::Passed)
        .map(|result| result.suite_id.clone());
    let mut artifacts = declared_artifacts.clone();
    for result in &suite_results {
        artifacts.push(result.stdout_path.clone());
        artifacts.push(result.stderr_path.clone());
        artifacts.extend(result.artifact_paths.clone());
    }
    artifacts.sort();
    artifacts.dedup();

    let manifest = LabManifest {
        schema_version: 1,
        run_id: run_id.clone(),
        created_at_unix: OffsetDateTime::now_utc().unix_timestamp(),
        command_id: "cmd.lab.verify.local".to_string(),
        argv: options.argv.clone(),
        reproduction_command: reproduction_command.clone(),
        registry_path: options.registry_path.display().to_string(),
        selected_suites: selected_suites.clone(),
        suite_results: suite_results.clone(),
        selection: SelectionRecord::from_criteria(&options.criteria),
        commit_id: best_effort_commit_id(),
        platform: PlatformInfo::current(),
        tool_versions: collect_tool_versions(),
        profiles,
        fixtures,
        env_redaction_policy:
            "allowlisted environment names are recorded; secret-like names are redacted".to_string(),
        env: redacted_env_snapshot_from(env),
        process_tree: Vec::new(),
        ports: Vec::new(),
        sockets: Vec::new(),
        artifacts: artifacts.clone(),
    };

    let summary = LabSummary {
        schema_version: 1,
        run_id: run_id.clone(),
        status,
        reason: summary_reason(status, &suite_results),
        selected_suite_count: selected_suites.len(),
        selected_suites: selected_suites
            .iter()
            .map(|suite| suite.id.clone())
            .collect(),
        suite_results,
        first_failure,
        reproduction_command,
        important_log_excerpts: important_log_excerpts(&manifest.suite_results),
        artifacts,
    };

    let manifest_path = run_dir.join("manifest.json");
    let summary_path = run_dir.join("summary.json");
    write_json(&manifest_path, &manifest)?;
    write_json(&summary_path, &summary)?;

    Ok(VerifyOutput {
        run_id,
        run_dir,
        manifest_path,
        summary_path,
        selected_suite_count: selected_suites.len(),
        status,
    })
}

fn execute_suite(
    suite: &SelectedSuite,
    run_dir: &Path,
    artifact_dir: &Path,
    config_dir: &Path,
    data_dir: &Path,
    secrets_dir: &Path,
) -> LabResult<SuiteExecutionRecord> {
    let suite_artifact_dir = artifact_dir.join(path_safe_suite_id(&suite.id));
    create_dir(&suite_artifact_dir)?;
    let stdout_path = suite_artifact_dir.join("stdout.log");
    let stderr_path = suite_artifact_dir.join("stderr.log");
    let timeout_seconds = suite
        .timeout_seconds
        .unwrap_or(DEFAULT_SUITE_TIMEOUT_SECONDS);
    let timeout = Duration::from_secs(timeout_seconds);

    let mut command = shell_command(&suite.command);
    configure_command_for_timeout(&mut command);
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("POSTHASTE_LAB_RUN_DIR", run_dir)
        .env("POSTHASTE_CONFIG_ROOT", config_dir)
        .env("POSTHASTE_STATE_ROOT", data_dir)
        .env("POSTHASTE_SECRETS_ROOT", secrets_dir);

    let started = Instant::now();
    let mut child = command.spawn().map_err(|source| LabError::SpawnSuite {
        suite_id: suite.id.clone(),
        source,
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| LabError::CaptureSuiteStream {
            suite_id: suite.id.clone(),
            stream: "stdout",
        })?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| LabError::CaptureSuiteStream {
            suite_id: suite.id.clone(),
            stream: "stderr",
        })?;
    let stdout_reader = read_stream_in_background(stdout);
    let stderr_reader = read_stream_in_background(stderr);

    let mut timed_out = false;
    let exit_status = loop {
        if let Some(status) = child.try_wait().map_err(|source| LabError::RunSuite {
            suite_id: suite.id.clone(),
            action: "poll",
            source,
        })? {
            break Some(status);
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            break Some(terminate_timed_out_child(&mut child, &suite.id)?);
        }
        thread::sleep(Duration::from_millis(50));
    };

    let duration_ms = started.elapsed().as_millis();
    let stdout = join_stream_reader(stdout_reader).map_err(|()| LabError::CaptureSuiteStream {
        suite_id: suite.id.clone(),
        stream: "stdout",
    })?;
    let stderr = join_stream_reader(stderr_reader).map_err(|()| LabError::CaptureSuiteStream {
        suite_id: suite.id.clone(),
        stream: "stderr",
    })?;
    write_bytes(&stdout_path, &stdout)?;
    write_bytes(&stderr_path, &stderr)?;
    let artifact_paths = discover_suite_artifact_paths(&stdout, &stderr, run_dir);

    let (exit_code, signal) = exit_status
        .as_ref()
        .map(exit_status_parts)
        .unwrap_or((None, None));
    let (status, reason) = suite_status(timed_out, exit_code, signal);

    Ok(SuiteExecutionRecord {
        suite_id: suite.id.clone(),
        command: suite.command.clone(),
        status,
        reason,
        exit_code,
        signal,
        timed_out,
        duration_ms,
        timeout_seconds,
        stdout_path: stdout_path.display().to_string(),
        stderr_path: stderr_path.display().to_string(),
        artifact_paths,
    })
}

fn shell_command(command: &str) -> ProcessCommand {
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

fn configure_command_for_timeout(command: &mut ProcessCommand) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

fn terminate_timed_out_child(child: &mut Child, suite_id: &str) -> LabResult<ExitStatus> {
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
fn terminate_unix_process_group(child: &mut Child, suite_id: &str) -> LabResult<()> {
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

fn read_stream_in_background<R>(mut reader: R) -> thread::JoinHandle<io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut output = Vec::new();
        reader.read_to_end(&mut output)?;
        Ok(output)
    })
}

fn join_stream_reader(handle: thread::JoinHandle<io::Result<Vec<u8>>>) -> Result<Vec<u8>, ()> {
    handle.join().map_err(|_| ())?.map_err(|_| ())
}

fn write_bytes(path: &Path, bytes: &[u8]) -> LabResult<()> {
    fs::write(path, bytes).map_err(|source| LabError::WriteFile {
        path: path.display().to_string(),
        source,
    })
}

fn discover_suite_artifact_paths(stdout: &[u8], stderr: &[u8], run_dir: &Path) -> Vec<String> {
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

fn existing_report_artifact_path(raw_path: &str, run_dir: &Path) -> Option<String> {
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

fn is_allowed_report_artifact_path(canonical_path: &Path, run_dir: &Path) -> bool {
    run_dir
        .canonicalize()
        .ok()
        .is_some_and(|canonical_run_dir| canonical_path.starts_with(canonical_run_dir))
        && !has_secret_like_path_segment(canonical_path)
}

fn has_secret_like_path_segment(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|segment| is_secret_env_name(segment))
    })
}

fn path_safe_suite_id(id: &str) -> String {
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

fn exit_status_parts(status: &ExitStatus) -> (Option<i32>, Option<i32>) {
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

fn suite_status(
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

fn aggregate_status(results: &[SuiteExecutionRecord]) -> LabStatus {
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

fn summary_reason(status: LabStatus, results: &[SuiteExecutionRecord]) -> String {
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

fn important_log_excerpts(results: &[SuiteExecutionRecord]) -> Vec<String> {
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

fn is_summary_artifact_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "summary.json")
}

fn is_log_excerpt_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "log" | "txt"))
}

fn summary_artifact_excerpt(path: &Path) -> Option<String> {
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

fn summary_value(value: &serde_json::Value) -> String {
    let text = value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string());
    redact_excerpt_text(&text)
}

fn tail_text_file(path: &Path, max_lines: usize) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let lines = text.lines().rev().take(max_lines).collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }
    Some(redact_excerpt_text(
        &lines.into_iter().rev().collect::<Vec<_>>().join("\n"),
    ))
}

fn redact_excerpt_text(text: &str) -> String {
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

fn contains_secret_marker(text: &str) -> bool {
    let uppercase = text.to_ascii_uppercase();
    SECRET_MARKERS
        .iter()
        .any(|marker| uppercase.contains(marker))
}

fn create_dir(path: &Path) -> LabResult<()> {
    fs::create_dir_all(path).map_err(|source| LabError::CreateDir {
        path: path.display().to_string(),
        source,
    })
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> LabResult<()> {
    let text = serde_json::to_string_pretty(value)?;
    fs::write(path, format!("{text}\n")).map_err(|source| LabError::WriteFile {
        path: path.display().to_string(),
        source,
    })
}

fn new_run_id() -> String {
    format!(
        "{}-{}",
        OffsetDateTime::now_utc().unix_timestamp(),
        Uuid::new_v4().simple()
    )
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LabManifest {
    schema_version: u32,
    run_id: String,
    created_at_unix: i64,
    command_id: String,
    argv: Vec<String>,
    reproduction_command: String,
    registry_path: String,
    selected_suites: Vec<SelectedSuite>,
    suite_results: Vec<SuiteExecutionRecord>,
    selection: SelectionRecord,
    commit_id: Option<String>,
    platform: PlatformInfo,
    tool_versions: BTreeMap<String, String>,
    profiles: Vec<String>,
    fixtures: Vec<String>,
    env_redaction_policy: String,
    env: BTreeMap<String, String>,
    process_tree: Vec<String>,
    ports: Vec<String>,
    sockets: Vec<String>,
    artifacts: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LabSummary {
    schema_version: u32,
    run_id: String,
    status: LabStatus,
    reason: String,
    selected_suite_count: usize,
    selected_suites: Vec<String>,
    suite_results: Vec<SuiteExecutionRecord>,
    first_failure: Option<String>,
    reproduction_command: String,
    important_log_excerpts: Vec<String>,
    artifacts: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LabStatus {
    Passed,
    Failed,
    Blocked,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SuiteExecutionRecord {
    suite_id: String,
    command: String,
    status: LabStatus,
    reason: String,
    exit_code: Option<i32>,
    signal: Option<i32>,
    timed_out: bool,
    duration_ms: u128,
    timeout_seconds: u64,
    stdout_path: String,
    stderr_path: String,
    artifact_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectionRecord {
    requested_suite_id: Option<String>,
    tags: Vec<String>,
    targets: Vec<String>,
    changed: bool,
    rationale: String,
}

impl SelectionRecord {
    fn from_criteria(criteria: &SelectionCriteria) -> Self {
        Self {
            requested_suite_id: criteria.suite_id.clone(),
            tags: criteria.tags.clone(),
            targets: criteria.targets.clone(),
            changed: criteria.changed,
            rationale: criteria.rationale(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformInfo {
    os: String,
    arch: String,
    family: String,
}

impl PlatformInfo {
    fn current() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            family: std::env::consts::FAMILY.to_string(),
        }
    }
}

fn best_effort_commit_id() -> Option<String> {
    command_output("jj", &["log", "-r", "@", "--no-graph", "-T", "commit_id"])
        .or_else(|| command_output("git", &["rev-parse", "HEAD"]))
}

fn collect_tool_versions() -> BTreeMap<String, String> {
    [
        ("cargo", &["--version"][..]),
        ("rustc", &["--version"][..]),
        ("bun", &["--version"][..]),
        ("just", &["--version"][..]),
        ("node", &["--version"][..]),
    ]
    .into_iter()
    .filter_map(|(tool, args)| {
        command_output(tool, args).map(|version| (tool.to_string(), version))
    })
    .collect()
}

fn command_output(command: &str, args: &[&str]) -> Option<String> {
    let output = ProcessCommand::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.lines().next().unwrap_or(trimmed).to_string())
    }
}

pub fn redacted_env_snapshot_from<I, K, V>(env: I) -> BTreeMap<String, String>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    env.into_iter()
        .filter_map(|(key, value)| {
            let key = key.into();
            if !should_record_env_name(&key) {
                return None;
            }
            let value = value.into();
            let value = if is_secret_env_name(&key) {
                REDACTED.to_string()
            } else {
                value
            };
            Some((key, value))
        })
        .collect()
}

fn should_record_env_name(name: &str) -> bool {
    name == "CI"
        || name == "USER"
        || name == "SHELL"
        || name == "RUST_LOG"
        || name.starts_with("POSTHASTE_")
}

fn is_secret_env_name(name: &str) -> bool {
    let uppercase = name.to_ascii_uppercase();
    SECRET_MARKERS
        .iter()
        .any(|marker| uppercase.contains(marker))
}

fn reproduction_command(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':' | '='))
    {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

pub fn default_registry_path() -> PathBuf {
    PathBuf::from("tools/lab/suites.toml")
}

pub fn default_run_root() -> PathBuf {
    PathBuf::from("target/lab/runs")
}

pub fn run_cli(argv: Vec<String>) -> LabResult<()> {
    let program = argv
        .first()
        .cloned()
        .unwrap_or_else(|| "posthaste-lab".to_string());
    let args = argv.iter().skip(1).cloned().collect::<Vec<_>>();
    if let Some(usage_kind) = usage_kind_for_args(&args) {
        print_usage_kind(&program, usage_kind);
        return Ok(());
    }

    match args.first().map(String::as_str) {
        Some("suite") => run_suite_command(&program, &args[1..]),
        Some("verify") => run_verify_command(argv, &args[1..]),
        Some(other) => Err(LabError::Usage(format!(
            "unknown command {other:?}; expected 'suite' or 'verify'"
        ))),
        None => {
            print_usage(&program);
            Ok(())
        }
    }
}

fn run_suite_command(program: &str, args: &[String]) -> LabResult<()> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_suite_usage(program);
        return Ok(());
    }
    match args.first().map(String::as_str) {
        Some("list") => {
            let options = parse_list_options(&args[1..])?;
            let registry = SuiteRegistry::load(&options.registry_path)?;
            let selected = registry.select(&SelectionCriteria {
                tags: options.tags,
                targets: options.targets,
                ..SelectionCriteria::default()
            })?;
            for suite in selected {
                println!("{}", suite.id);
            }
            Ok(())
        }
        Some(other) => Err(LabError::Usage(format!(
            "unknown suite command {other:?}; expected 'list'"
        ))),
        None => Err(LabError::Usage(
            "missing suite command; expected 'list'".to_string(),
        )),
    }
}

fn run_verify_command(argv: Vec<String>, args: &[String]) -> LabResult<()> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_verify_usage(argv.first().map(String::as_str).unwrap_or("posthaste-lab"));
        return Ok(());
    }
    let options = parse_verify_options(args, argv)?;
    let registry = SuiteRegistry::load(&options.registry_path)?;
    let output = write_verify_run(&registry, options)?;
    println!("Lab run: {}", output.run_dir.display());
    println!("Manifest: {}", output.manifest_path.display());
    println!("Summary: {}", output.summary_path.display());
    println!("Selected suites: {}", output.selected_suite_count);
    println!("Status: {:?}", output.status);
    match output.status {
        LabStatus::Passed => Ok(()),
        LabStatus::Blocked => Err(LabError::VerificationBlocked {
            summary_path: output.summary_path.display().to_string(),
        }),
        LabStatus::Skipped => Err(LabError::VerificationSkipped {
            summary_path: output.summary_path.display().to_string(),
        }),
        LabStatus::Failed => Err(LabError::VerificationFailed {
            summary_path: output.summary_path.display().to_string(),
        }),
    }
}

#[derive(Debug, Clone)]
struct ListOptions {
    registry_path: PathBuf,
    tags: Vec<String>,
    targets: Vec<String>,
}

fn parse_list_options(args: &[String]) -> LabResult<ListOptions> {
    let mut registry_path = default_registry_path();
    let mut tags = Vec::new();
    let mut targets = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--registry" => {
                index += 1;
                registry_path = PathBuf::from(required_value(args, index, "--registry")?);
            }
            "--tag" => {
                index += 1;
                tags.push(required_value(args, index, "--tag")?);
            }
            "--target" => {
                index += 1;
                targets.push(required_value(args, index, "--target")?);
            }
            _ if arg.starts_with("--registry=") => {
                registry_path = PathBuf::from(arg.trim_start_matches("--registry=").to_string());
            }
            _ if arg.starts_with("--tag=") => {
                tags.push(arg.trim_start_matches("--tag=").to_string());
            }
            _ if arg.starts_with("--target=") => {
                targets.push(arg.trim_start_matches("--target=").to_string());
            }
            _ if arg.starts_with("--") => {
                return Err(LabError::Usage(format!("unknown option {arg:?}")));
            }
            _ => {
                return Err(LabError::Usage(format!(
                    "unexpected positional argument {arg:?} for suite list"
                )));
            }
        }
        index += 1;
    }

    Ok(ListOptions {
        registry_path,
        tags,
        targets,
    })
}

fn parse_verify_options(args: &[String], argv: Vec<String>) -> LabResult<VerifyOptions> {
    let mut registry_path = default_registry_path();
    let mut run_root = default_run_root();
    let mut criteria = SelectionCriteria::default();
    let mut positional_suite_id = None;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--registry" => {
                index += 1;
                registry_path = PathBuf::from(required_value(args, index, "--registry")?);
            }
            "--run-root" => {
                index += 1;
                run_root = PathBuf::from(required_value(args, index, "--run-root")?);
            }
            "--tag" => {
                index += 1;
                criteria.tags.push(required_value(args, index, "--tag")?);
            }
            "--target" => {
                index += 1;
                criteria
                    .targets
                    .push(required_value(args, index, "--target")?);
            }
            "--changed" => {
                criteria.changed = true;
            }
            _ if arg.starts_with("--registry=") => {
                registry_path = PathBuf::from(arg.trim_start_matches("--registry=").to_string());
            }
            _ if arg.starts_with("--run-root=") => {
                run_root = PathBuf::from(arg.trim_start_matches("--run-root=").to_string());
            }
            _ if arg.starts_with("--tag=") => {
                criteria
                    .tags
                    .push(arg.trim_start_matches("--tag=").to_string());
            }
            _ if arg.starts_with("--target=") => {
                criteria
                    .targets
                    .push(arg.trim_start_matches("--target=").to_string());
            }
            _ if arg.starts_with("--") => {
                return Err(LabError::Usage(format!("unknown option {arg:?}")));
            }
            _ => {
                if positional_suite_id.is_some() {
                    return Err(LabError::Usage(format!(
                        "multiple suite ids supplied; unexpected {arg:?}"
                    )));
                }
                positional_suite_id = Some(arg.clone());
            }
        }
        index += 1;
    }
    criteria.suite_id = positional_suite_id;
    if criteria.changed {
        return Err(LabError::ChangedSelectionUnsupported);
    }

    Ok(VerifyOptions {
        run_root,
        registry_path,
        argv,
        criteria,
    })
}

fn required_value(args: &[String], index: usize, option: &str) -> LabResult<String> {
    args.get(index)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .ok_or_else(|| LabError::Usage(format!("{option} requires a value")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UsageKind {
    TopLevel,
    SuiteList,
    Verify,
}

fn usage_kind_for_args(args: &[String]) -> Option<UsageKind> {
    match args.first().map(String::as_str) {
        None => Some(UsageKind::TopLevel),
        Some(arg) if is_help_arg(arg) => Some(UsageKind::TopLevel),
        Some("suite") if args.iter().any(|arg| is_help_arg(arg)) => Some(UsageKind::SuiteList),
        Some("verify") if args.iter().any(|arg| is_help_arg(arg)) => Some(UsageKind::Verify),
        _ => None,
    }
}

fn is_help_arg(arg: &str) -> bool {
    arg == "--help" || arg == "-h"
}

fn print_usage_kind(program: &str, usage_kind: UsageKind) {
    match usage_kind {
        UsageKind::TopLevel => print_usage(program),
        UsageKind::SuiteList => print_suite_usage(program),
        UsageKind::Verify => print_verify_usage(program),
    }
}

fn print_usage(program: &str) {
    println!("Usage:");
    println!("  {program} suite list [--tag TAG] [--target TARGET] [--registry PATH]");
    println!("  {program} verify [SUITE_ID] [--tag TAG] [--target TARGET] [--registry PATH] [--run-root PATH] [--changed]");
}

fn print_suite_usage(program: &str) {
    println!("Usage: {program} suite list [--tag TAG] [--target TARGET] [--registry PATH]");
}

fn print_verify_usage(program: &str) {
    println!("Usage: {program} verify [SUITE_ID] [--tag TAG] [--target TARGET] [--registry PATH] [--run-root PATH] [--changed]");
    println!("Note: --changed is parsed but currently unsupported.");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_registry_toml() -> &'static str {
        r#"
[suite.api.settings.dev]
level = "integration"
targets = ["daemon"]
profile = "profile.lab.empty.dev"
fixture = "fixture.mail.basic.test"
runners = ["runner.cargo.test.dev"]
tags = ["api", "settings", "fast"]
paths = ["crates/posthaste-server/tests/settings_patch.rs"]
command = "printf 'settings stdout\\n'; printf 'settings stderr\\n' >&2"
timeout_seconds = 5
artifacts = ["log.backend.jsonl.dev"]

[suite.dev.smoke.local]
level = "smoke"
targets = ["dev"]
profile = "profile.lab.empty.local"
runners = ["runner.just.dev.local"]
tags = ["dev", "smoke", "fast"]
paths = ["tools/dev/smoke.sh", "justfile"]
command = "printf 'dev smoke\\n'"
timeout_seconds = 5
artifacts = ["artifact.summary.dev.local"]
"#
    }

    #[test]
    fn loads_docs_style_nested_suite_registry() {
        // spec: docs/L1-lab#registry-thin-orchestrator
        let registry = SuiteRegistry::from_toml_str(sample_registry_toml()).unwrap();

        let settings = registry.suites().get("suite.api.settings.dev").unwrap();
        assert_eq!(settings.level, "integration");
        assert_eq!(settings.targets, vec!["daemon"]);
        assert_eq!(settings.runners, vec!["runner.cargo.test.dev"]);
        assert!(registry.suites().contains_key("suite.dev.smoke.local"));
    }

    #[test]
    fn real_suite_registry_paths_exist_and_smoke_stays_non_graphical() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let registry = SuiteRegistry::load(repo_root.join("tools/lab/suites.toml")).unwrap();
        let smoke_suite_ids = registry
            .suites()
            .iter()
            .filter_map(|(id, entry)| entry.tags.iter().any(|tag| tag == "lab-smoke").then_some(id))
            .collect::<Vec<_>>();

        assert_eq!(
            smoke_suite_ids,
            vec![
                "suite.api.health.dev",
                "suite.ui.readiness.web.test",
                "suite.ui.surfaces.web.test",
            ]
        );

        for (id, entry) in registry.suites() {
            assert!(!entry.command.trim().is_empty(), "{id} command is empty");
            assert!(
                entry.timeout_seconds.unwrap_or(DEFAULT_SUITE_TIMEOUT_SECONDS) > 0,
                "{id} timeout must be positive"
            );
            for path in &entry.paths {
                assert!(
                    Path::new(path).is_relative() && !path.contains(".."),
                    "{id} path {path:?} must stay relative to the repository root"
                );
                assert!(
                    repo_root.join(path).exists(),
                    "{id} path {path:?} does not exist"
                );
            }
            if entry.tags.iter().any(|tag| tag == "lab-smoke") {
                assert!(
                    !entry.targets.iter().any(|target| target == "desktop")
                        && !entry.tags.iter().any(|tag| tag == "tauri"),
                    "{id} lab-smoke suite must remain non-graphical"
                );
            }
        }
    }

    #[test]
    fn selects_suites_by_explicit_id_tag_and_target() {
        let registry = SuiteRegistry::from_toml_str(sample_registry_toml()).unwrap();

        let explicit = registry
            .select(&SelectionCriteria {
                suite_id: Some("suite.api.settings.dev".to_string()),
                tags: vec!["settings".to_string()],
                targets: vec!["daemon".to_string()],
                changed: false,
            })
            .unwrap();
        assert_eq!(
            explicit
                .iter()
                .map(|suite| suite.id.as_str())
                .collect::<Vec<_>>(),
            vec!["suite.api.settings.dev"]
        );

        let filtered = registry
            .select(&SelectionCriteria {
                tags: vec!["fast".to_string()],
                targets: vec!["dev".to_string()],
                ..SelectionCriteria::default()
            })
            .unwrap();
        assert_eq!(
            filtered
                .iter()
                .map(|suite| suite.id.as_str())
                .collect::<Vec<_>>(),
            vec!["suite.dev.smoke.local"]
        );
    }

    #[test]
    fn rejects_invalid_lab_ids() {
        assert!(validate_lab_id("runner:web.main.local").is_ok());
        assert!(validate_lab_id("profile.lab.upgrade.dev.from:v0.1.0-dogfood.17").is_ok());
        assert!(validate_lab_id("suite.api settings.dev").is_err());
        assert!(validate_lab_id("unknown.foo").is_err());
        assert!(validate_lab_id("suite").is_err());
    }

    #[test]
    fn changed_selection_is_unsupported() {
        let registry = SuiteRegistry::from_toml_str(sample_registry_toml()).unwrap();

        let err = registry
            .select(&SelectionCriteria {
                changed: true,
                ..SelectionCriteria::default()
            })
            .unwrap_err();
        assert!(matches!(err, LabError::ChangedSelectionUnsupported));
    }

    #[test]
    fn routes_command_specific_help() {
        let args = |parts: &[&str]| {
            parts
                .iter()
                .map(|part| (*part).to_string())
                .collect::<Vec<_>>()
        };

        assert_eq!(usage_kind_for_args(&args(&[])), Some(UsageKind::TopLevel));
        assert_eq!(
            usage_kind_for_args(&args(&["--help"])),
            Some(UsageKind::TopLevel)
        );
        assert_eq!(
            usage_kind_for_args(&args(&["suite", "list", "--help"])),
            Some(UsageKind::SuiteList)
        );
        assert_eq!(
            usage_kind_for_args(&args(&["verify", "--help"])),
            Some(UsageKind::Verify)
        );
        assert_eq!(usage_kind_for_args(&args(&["suite", "list"])), None);
    }

    #[test]
    fn writes_manifest_and_summary_under_disposable_run_root() {
        // spec: docs/L1-lab#disposable-run-roots
        // spec: docs/L1-lab#artifact-manifest
        let registry = SuiteRegistry::from_toml_str(sample_registry_toml()).unwrap();
        let temp_root =
            std::env::temp_dir().join(format!("posthaste-lab-test-{}", Uuid::new_v4().simple()));
        let options = VerifyOptions {
            run_root: temp_root.clone(),
            registry_path: PathBuf::from("tools/lab/suites.toml"),
            argv: vec![
                "posthaste-lab".to_string(),
                "verify".to_string(),
                "suite.api.settings.dev".to_string(),
            ],
            criteria: SelectionCriteria {
                suite_id: Some("suite.api.settings.dev".to_string()),
                ..SelectionCriteria::default()
            },
        };

        let output = write_verify_run_with_env(
            &registry,
            options,
            [
                ("POSTHASTE_CONFIG_ROOT", "/tmp/posthaste/config"),
                ("POSTHASTE_TEST_SECRET_TOKEN", "super-secret-token"),
                ("PATH", "/private/local/bin:/other/local/bin"),
                ("CARGO_MANIFEST_PATH", "/private/local/Cargo.toml"),
                ("SSH_AUTH_SOCK", "/private/local/agent.sock"),
                ("UNRELATED_SECRET", "should-not-be-recorded"),
            ],
        )
        .unwrap();

        assert!(output.run_dir.join("state.config").is_dir());
        assert!(output.run_dir.join("state.data").is_dir());
        assert!(output.run_dir.join("state.secrets").is_dir());

        let manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&output.manifest_path).expect("manifest should be readable"),
        )
        .unwrap();
        assert_eq!(manifest["schemaVersion"], 1);
        assert_eq!(
            manifest["selectedSuites"][0]["id"],
            "suite.api.settings.dev"
        );
        assert_eq!(
            manifest["selection"]["rationale"],
            "explicit suite suite.api.settings.dev"
        );
        assert_eq!(manifest["profiles"][0], "profile.lab.empty.dev");
        assert_eq!(manifest["fixtures"][0], "fixture.mail.basic.test");
        assert_eq!(
            manifest["env"]["POSTHASTE_CONFIG_ROOT"],
            "/tmp/posthaste/config"
        );
        assert_eq!(manifest["env"]["POSTHASTE_TEST_SECRET_TOKEN"], REDACTED);
        assert!(manifest["env"].get("PATH").is_none());
        assert!(manifest["env"].get("CARGO_MANIFEST_PATH").is_none());
        assert!(manifest["env"].get("SSH_AUTH_SOCK").is_none());
        assert!(manifest["env"].get("UNRELATED_SECRET").is_none());

        let summary: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&output.summary_path).expect("summary should be readable"),
        )
        .unwrap();
        assert_eq!(summary["status"], "passed");
        assert_eq!(summary["reason"], "all selected suites passed");
        assert_eq!(summary["selectedSuiteCount"], 1);
        assert_eq!(summary["suiteResults"][0]["exitCode"], 0);
        assert_eq!(summary["suiteResults"][0]["timedOut"], false);
        let stdout_path = summary["suiteResults"][0]["stdoutPath"].as_str().unwrap();
        let stderr_path = summary["suiteResults"][0]["stderrPath"].as_str().unwrap();
        assert_eq!(
            fs::read_to_string(stdout_path).unwrap(),
            "settings stdout\n"
        );
        assert_eq!(
            fs::read_to_string(stderr_path).unwrap(),
            "settings stderr\n"
        );

        fs::remove_dir_all(temp_root).ok();
    }

    #[test]
    fn propagates_marked_nested_artifacts_and_excerpts() {
        let registry = SuiteRegistry::from_toml_str(
            r#"
[suite.nested.artifacts.demo]
level = "smoke"
targets = ["desktop"]
runners = ["runner.shell.test"]
tags = ["nested"]
paths = []
command = '''
mkdir -p "$POSTHASTE_LAB_RUN_DIR/nested/artifacts"
printf '{"status":"failed","reason":"nested smoke failed","exitCode":1,"runDir":"%s"}\n' "$POSTHASTE_LAB_RUN_DIR/nested" > "$POSTHASTE_LAB_RUN_DIR/nested/summary.json"
printf 'nested playwright failure detail\n' > "$POSTHASTE_LAB_RUN_DIR/nested/artifacts/playwright.log"
printf 'TOKEN=should-not-be-in-summary\n' > "$POSTHASTE_SECRETS_ROOT/leak.log"
printf '%s%s\n' 'POSTHASTE_LAB_ARTIFACT_PATH=' "$POSTHASTE_LAB_RUN_DIR/nested/../nested/summary.json"
printf '%s%s\n' 'POSTHASTE_LAB_ARTIFACT_PATH=' "$POSTHASTE_LAB_RUN_DIR/nested/artifacts/playwright.log"
printf '%s%s\n' 'POSTHASTE_LAB_ARTIFACT_PATH=' "$POSTHASTE_LAB_RUN_DIR/nested/missing.json"
printf '%s%s\n' 'POSTHASTE_LAB_ARTIFACT_PATH=' "$POSTHASTE_SECRETS_ROOT/leak.log"
exit 1
'''
timeout_seconds = 5
"#,
        )
        .unwrap();
        let temp_root =
            std::env::temp_dir().join(format!("posthaste-lab-test-{}", Uuid::new_v4().simple()));

        let output = write_verify_run_with_env(
            &registry,
            VerifyOptions {
                run_root: temp_root.clone(),
                registry_path: PathBuf::from("tools/lab/suites.toml"),
                argv: vec!["posthaste-lab".to_string(), "verify".to_string()],
                criteria: SelectionCriteria::default(),
            },
            std::iter::empty::<(String, String)>(),
        )
        .unwrap();

        let summary: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&output.summary_path).expect("summary should be readable"),
        )
        .unwrap();
        let summary_path = output
            .run_dir
            .join("nested")
            .join("summary.json")
            .display()
            .to_string();
        let log_path = output
            .run_dir
            .join("nested")
            .join("artifacts")
            .join("playwright.log")
            .display()
            .to_string();
        let missing_path = output
            .run_dir
            .join("nested")
            .join("missing.json")
            .display()
            .to_string();
        let secret_path = output
            .run_dir
            .join("state.secrets")
            .join("leak.log")
            .display()
            .to_string();

        assert_eq!(summary["status"], "failed");
        let artifact_paths = summary["suiteResults"][0]["artifactPaths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(artifact_paths.contains(&summary_path.as_str()));
        assert!(artifact_paths.contains(&log_path.as_str()));
        assert!(!artifact_paths.contains(&missing_path.as_str()));
        assert!(!artifact_paths.contains(&secret_path.as_str()));
        let artifacts = summary["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(artifacts.contains(&summary_path.as_str()));
        assert!(artifacts.contains(&log_path.as_str()));
        assert!(summary["importantLogExcerpts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str().unwrap().contains("nested smoke failed")));
        assert!(summary["importantLogExcerpts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value
                .as_str()
                .unwrap()
                .contains("nested playwright failure detail")));
        assert!(!summary["importantLogExcerpts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str().unwrap().contains("should-not-be-in-summary")));

        let manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&output.manifest_path).expect("manifest should be readable"),
        )
        .unwrap();
        let manifest_artifacts = manifest["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(manifest_artifacts.contains(&summary_path.as_str()));
        assert!(manifest_artifacts.contains(&log_path.as_str()));

        fs::remove_dir_all(temp_root).ok();
    }

    #[test]
    fn maps_blocked_exit_code_to_blocked_summary() {
        let registry = SuiteRegistry::from_toml_str(
            r#"
[suite.blocked.demo]
level = "smoke"
targets = ["daemon"]
runners = ["runner.shell.test"]
tags = ["blocked"]
paths = []
command = "printf 'display unavailable\\n' >&2; exit 78"
timeout_seconds = 5
"#,
        )
        .unwrap();
        let temp_root =
            std::env::temp_dir().join(format!("posthaste-lab-test-{}", Uuid::new_v4().simple()));

        let output = write_verify_run_with_env(
            &registry,
            VerifyOptions {
                run_root: temp_root.clone(),
                registry_path: PathBuf::from("tools/lab/suites.toml"),
                argv: vec!["posthaste-lab".to_string(), "verify".to_string()],
                criteria: SelectionCriteria::default(),
            },
            std::iter::empty::<(String, String)>(),
        )
        .unwrap();

        let summary: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&output.summary_path).expect("summary should be readable"),
        )
        .unwrap();
        assert_eq!(output.status, LabStatus::Blocked);
        assert_eq!(summary["status"], "blocked");
        assert_eq!(summary["suiteResults"][0]["exitCode"], 78);
        assert_eq!(summary["firstFailure"], "suite.blocked.demo");
        assert!(summary["importantLogExcerpts"][0]
            .as_str()
            .unwrap()
            .contains("display unavailable"));

        fs::remove_dir_all(temp_root).ok();
    }

    #[test]
    fn maps_skipped_exit_code_to_skipped_summary() {
        let registry = SuiteRegistry::from_toml_str(
            r#"
[suite.skipped.demo]
level = "smoke"
targets = ["daemon"]
runners = ["runner.shell.test"]
tags = ["skipped"]
paths = []
command = "printf 'unsupported platform\\n'; exit 77"
timeout_seconds = 5
"#,
        )
        .unwrap();
        let temp_root =
            std::env::temp_dir().join(format!("posthaste-lab-test-{}", Uuid::new_v4().simple()));

        let output = write_verify_run_with_env(
            &registry,
            VerifyOptions {
                run_root: temp_root.clone(),
                registry_path: PathBuf::from("tools/lab/suites.toml"),
                argv: vec!["posthaste-lab".to_string(), "verify".to_string()],
                criteria: SelectionCriteria::default(),
            },
            std::iter::empty::<(String, String)>(),
        )
        .unwrap();

        let summary: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&output.summary_path).expect("summary should be readable"),
        )
        .unwrap();
        assert_eq!(output.status, LabStatus::Skipped);
        assert_eq!(summary["status"], "skipped");
        assert_eq!(summary["reason"], "all selected suites were skipped");
        assert_eq!(summary["suiteResults"][0]["exitCode"], 77);
        assert_eq!(
            summary["suiteResults"][0]["reason"],
            "suite command reported skipped status 77"
        );

        let err = LabError::VerificationSkipped {
            summary_path: output.summary_path.display().to_string(),
        };
        assert_eq!(err.exit_code(), 77);

        fs::remove_dir_all(temp_root).ok();
    }

    #[test]
    fn terminates_timed_out_suite_and_records_stdout_stderr() {
        let registry = SuiteRegistry::from_toml_str(
            r#"
[suite.timeout.demo]
level = "smoke"
targets = ["daemon"]
runners = ["runner.shell.test"]
tags = ["timeout"]
paths = []
command = "printf 'before sleep\\n'; printf 'still waiting\\n' >&2; sleep 2"
timeout_seconds = 1
"#,
        )
        .unwrap();
        let temp_root =
            std::env::temp_dir().join(format!("posthaste-lab-test-{}", Uuid::new_v4().simple()));

        let output = write_verify_run_with_env(
            &registry,
            VerifyOptions {
                run_root: temp_root.clone(),
                registry_path: PathBuf::from("tools/lab/suites.toml"),
                argv: vec!["posthaste-lab".to_string(), "verify".to_string()],
                criteria: SelectionCriteria::default(),
            },
            std::iter::empty::<(String, String)>(),
        )
        .unwrap();

        let summary: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&output.summary_path).expect("summary should be readable"),
        )
        .unwrap();
        assert_eq!(output.status, LabStatus::Failed);
        assert_eq!(summary["status"], "failed");
        assert_eq!(summary["suiteResults"][0]["timedOut"], true);
        assert_eq!(
            summary["suiteResults"][0]["reason"],
            "suite command timed out"
        );
        let stdout_path = summary["suiteResults"][0]["stdoutPath"].as_str().unwrap();
        let stderr_path = summary["suiteResults"][0]["stderrPath"].as_str().unwrap();
        assert_eq!(fs::read_to_string(stdout_path).unwrap(), "before sleep\n");
        assert_eq!(fs::read_to_string(stderr_path).unwrap(), "still waiting\n");

        fs::remove_dir_all(temp_root).ok();
    }
}
