use super::*;

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
    #[error("changed-file suite selection found no changed paths")]
    ChangedSelectionNeedsPaths,
    #[error("usage error: {0}")]
    Usage(String),
    #[error("verification failed; summary: {summary_path}")]
    VerificationFailed { summary_path: String },
    #[error("verification blocked; summary: {summary_path}")]
    VerificationBlocked { summary_path: String },
    #[error("verification skipped; summary: {summary_path}")]
    VerificationSkipped { summary_path: String },
    #[error("config validation failed for {config_dir}: {message}")]
    ConfigValidation { config_dir: String, message: String },
}

impl LabError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::VerificationFailed { .. } | Self::ConfigValidation { .. } => 1,
            Self::VerificationSkipped { .. } => 77,
            Self::VerificationBlocked { .. } => 78,
            _ => 2,
        }
    }
}
