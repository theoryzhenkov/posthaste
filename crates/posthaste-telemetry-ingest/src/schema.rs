use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TelemetryBatch {
    pub schema_version: u32,
    pub app_version: String,
    pub app_channel: AppChannel,
    pub os_family: OsFamily,
    pub arch: Arch,
    pub telemetry_mode: TelemetryMode,
    pub client_day: String,
    #[serde(default)]
    pub subject_id: Option<String>,
    pub events: Vec<TelemetryEvent>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TelemetryEvent {
    pub name: String,
    pub version: u32,
    pub event_id: String,
    #[serde(default)]
    pub fields: BTreeMap<String, Value>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryMode {
    Aggregate,
    Product,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppChannel {
    Dev,
    Beta,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OsFamily {
    Linux,
    Macos,
    Windows,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Arch {
    X86_64,
    Aarch64,
    Unknown,
}

impl TelemetryMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Aggregate => "aggregate",
            Self::Product => "product",
        }
    }
}

impl AppChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Beta => "beta",
        }
    }
}

impl OsFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Unknown => "unknown",
        }
    }
}

impl Arch {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
            Self::Unknown => "unknown",
        }
    }
}
