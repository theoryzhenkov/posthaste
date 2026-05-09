use std::{env, net::SocketAddr, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_path: PathBuf,
    pub max_body_bytes: usize,
    pub max_events_per_batch: usize,
    pub raw_retention_days: i64,
    pub dedupe_retention_days: i64,
    pub disabled: bool,
    pub ingest_token: Option<String>,
    pub rate_limit_per_minute: usize,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            bind: env_or("POSTHASTE_TELEMETRY_BIND", "127.0.0.1:8080").parse()?,
            database_path: PathBuf::from(env_or(
                "POSTHASTE_TELEMETRY_DATABASE",
                "/data/telemetry.sqlite3",
            )),
            max_body_bytes: parse_env_usize("POSTHASTE_TELEMETRY_MAX_BODY_BYTES", 262_144)?,
            max_events_per_batch: parse_env_usize("POSTHASTE_TELEMETRY_MAX_EVENTS_PER_BATCH", 100)?,
            raw_retention_days: parse_env_i64("POSTHASTE_TELEMETRY_RAW_RETENTION_DAYS", 30)?,
            dedupe_retention_days: parse_env_i64("POSTHASTE_TELEMETRY_DEDUPE_RETENTION_DAYS", 7)?,
            disabled: parse_env_bool("POSTHASTE_TELEMETRY_DISABLED"),
            ingest_token: required_ingest_token()?,
            rate_limit_per_minute: parse_env_usize(
                "POSTHASTE_TELEMETRY_RATE_LIMIT_PER_MINUTE",
                60,
            )?,
        })
    }
}

fn env_or(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_string())
}

fn parse_env_usize(name: &str, default: usize) -> Result<usize, ConfigError> {
    match env::var(name) {
        Ok(value) => Ok(value.parse()?),
        Err(_) => Ok(default),
    }
}

fn parse_env_i64(name: &str, default: i64) -> Result<i64, ConfigError> {
    match env::var(name) {
        Ok(value) => Ok(value.parse()?),
        Err(_) => Ok(default),
    }
}

fn required_ingest_token() -> Result<Option<String>, ConfigError> {
    let token = env::var("POSTHASTE_TELEMETRY_INGEST_TOKEN")
        .ok()
        .filter(|value| !value.is_empty());
    if token.is_some() || parse_env_bool("POSTHASTE_TELEMETRY_ALLOW_UNAUTHENTICATED") {
        Ok(token)
    } else {
        Err(ConfigError::MissingIngestToken)
    }
}

fn parse_env_bool(name: &str) -> bool {
    matches!(
        env::var(name).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid bind address")]
    Bind(#[from] std::net::AddrParseError),
    #[error("POSTHASTE_TELEMETRY_INGEST_TOKEN is required unless POSTHASTE_TELEMETRY_ALLOW_UNAUTHENTICATED=true")]
    MissingIngestToken,
    #[error("invalid integer environment value")]
    Integer(#[from] std::num::ParseIntError),
}
