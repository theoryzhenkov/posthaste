use super::*;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LabManifest {
    pub(crate) schema_version: u32,
    pub(crate) run_id: String,
    pub(crate) created_at_unix: i64,
    pub(crate) command_id: String,
    pub(crate) argv: Vec<String>,
    pub(crate) reproduction_command: String,
    pub(crate) registry_path: String,
    pub(crate) selected_suites: Vec<SelectedSuite>,
    pub(crate) suite_results: Vec<SuiteExecutionRecord>,
    pub(crate) selection: SelectionRecord,
    pub(crate) commit_id: Option<String>,
    pub(crate) platform: PlatformInfo,
    pub(crate) tool_versions: BTreeMap<String, String>,
    pub(crate) profiles: Vec<String>,
    pub(crate) fixtures: Vec<String>,
    pub(crate) env_redaction_policy: String,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) process_tree: Vec<String>,
    pub(crate) ports: Vec<String>,
    pub(crate) sockets: Vec<String>,
    pub(crate) artifacts: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LabSummary {
    pub(crate) schema_version: u32,
    pub(crate) run_id: String,
    pub(crate) status: LabStatus,
    pub(crate) reason: String,
    pub(crate) selected_suite_count: usize,
    pub(crate) selected_suites: Vec<String>,
    pub(crate) selection: SelectionRecord,
    pub(crate) suite_results: Vec<SuiteExecutionRecord>,
    pub(crate) first_failure: Option<String>,
    pub(crate) reproduction_command: String,
    pub(crate) important_log_excerpts: Vec<String>,
    pub(crate) artifacts: Vec<String>,
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
pub(crate) struct SuiteExecutionRecord {
    pub(crate) suite_id: String,
    pub(crate) command: String,
    pub(crate) status: LabStatus,
    pub(crate) reason: String,
    pub(crate) exit_code: Option<i32>,
    pub(crate) signal: Option<i32>,
    pub(crate) timed_out: bool,
    pub(crate) duration_ms: u128,
    pub(crate) timeout_seconds: u64,
    pub(crate) stdout_path: String,
    pub(crate) stderr_path: String,
    pub(crate) artifact_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SuiteListOutput {
    pub(crate) schema_version: u32,
    pub(crate) selection: SelectionRecord,
    pub(crate) suites: Vec<SelectedSuite>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionRecord {
    pub(crate) requested_suite_id: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) targets: Vec<String>,
    pub(crate) changed: bool,
    pub(crate) changed_paths: Vec<String>,
    pub(crate) rationale: String,
}

impl SelectionRecord {
    pub(crate) fn from_criteria(criteria: &SelectionCriteria) -> Self {
        Self {
            requested_suite_id: criteria.suite_id.clone(),
            tags: criteria.tags.clone(),
            targets: criteria.targets.clone(),
            changed: criteria.changed,
            changed_paths: criteria.changed_paths.clone(),
            rationale: criteria.rationale(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlatformInfo {
    pub(crate) os: String,
    pub(crate) arch: String,
    pub(crate) family: String,
}

impl PlatformInfo {
    pub(crate) fn current() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            family: std::env::consts::FAMILY.to_string(),
        }
    }
}

pub(crate) fn best_effort_commit_id() -> Option<String> {
    command_output("jj", &["log", "-r", "@", "--no-graph", "-T", "commit_id"])
        .or_else(|| command_output("git", &["rev-parse", "HEAD"]))
}

pub(crate) fn collect_tool_versions() -> BTreeMap<String, String> {
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

pub(crate) fn command_output(command: &str, args: &[&str]) -> Option<String> {
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

pub(crate) fn should_record_env_name(name: &str) -> bool {
    name == "CI"
        || name == "USER"
        || name == "SHELL"
        || name == "RUST_LOG"
        || name.starts_with("POSTHASTE_")
}

pub(crate) fn is_secret_env_name(name: &str) -> bool {
    let uppercase = name.to_ascii_uppercase();
    SECRET_MARKERS
        .iter()
        .any(|marker| uppercase.contains(marker))
}

pub(crate) fn reproduction_command(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn shell_quote(arg: &str) -> String {
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
