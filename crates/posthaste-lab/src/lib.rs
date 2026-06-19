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

mod cli;
mod cli_support;
mod error;
mod process;
mod records;
mod registry;
mod selection;
mod summary;
mod verify;

pub use cli::{default_config_dir, default_registry_path, default_run_root, run_cli};
pub use error::{LabError, LabResult};
pub use records::{redacted_env_snapshot_from, LabStatus};
pub use registry::{validate_lab_id, SuiteEntry, SuiteRegistry};
pub use selection::{SelectedSuite, SelectionCriteria};
pub use verify::{write_verify_run, write_verify_run_with_env, VerifyOptions, VerifyOutput};

use cli_support::*;
use process::*;
use records::{
    best_effort_commit_id, collect_tool_versions, command_output, is_secret_env_name,
    reproduction_command, LabManifest, LabSummary, PlatformInfo, SelectionRecord,
    SuiteExecutionRecord, SuiteListOutput,
};
use registry::normalize_lab_path;
use summary::*;

#[cfg(test)]
use cli::{parse_config_validate_options, parse_list_options, run_config_validate_command};
#[cfg(test)]
use cli_support::{usage_kind_for_args, UsageKind};

#[cfg(test)]
mod tests;
