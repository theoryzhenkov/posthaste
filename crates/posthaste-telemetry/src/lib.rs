use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use reqwest::StatusCode;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use posthaste_domain::{TelemetryMode, TelemetrySettings};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::Sha256;
use time::OffsetDateTime;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

const NOTICE_VERSION: &str = "2026-05-beta-1";
const DEFAULT_CATEGORIES: &[&str] = &["health", "performance", "cache", "ui", "profile"];
const MAX_SPOOL_BYTES: u64 = 1_048_576;
const MAX_SPOOL_AGE: Duration = Duration::from_secs(72 * 60 * 60);

#[derive(Clone, Debug)]
pub struct TelemetrySpool {
    root: PathBuf,
    settings: TelemetrySettings,
    app_version: String,
    app_channel: String,
}

impl TelemetrySpool {
    pub fn new(
        state_root: impl AsRef<Path>,
        settings: TelemetrySettings,
        app_version: String,
    ) -> Self {
        Self {
            root: state_root.as_ref().join("telemetry"),
            settings,
            app_version,
            app_channel: "beta".to_string(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        !matches!(self.settings.mode, TelemetryMode::Off)
    }

    pub fn emit(&self, event: TelemetryEvent) -> Result<EmitOutcome, TelemetryError> {
        if !self.is_enabled() {
            return Ok(EmitOutcome::DroppedConsentOff);
        }
        if self.settings.notice_version.as_deref() != Some(NOTICE_VERSION)
            || self.settings.enabled_at.is_none()
            || self.settings.categories.is_empty()
        {
            return Ok(EmitOutcome::DroppedInvalidConsent);
        }

        let pending_dir = self.root.join("pending");
        fs::create_dir_all(&pending_dir)?;
        set_owner_only_dir(&self.root)?;
        set_owner_only_dir(&pending_dir)?;
        cleanup_pending(&pending_dir)?;

        let now = OffsetDateTime::now_utc();
        let event_id = Uuid::new_v4().to_string();
        let batch = self.batch(now, event_id, event)?;
        let bytes = serde_json::to_vec_pretty(&batch)?;
        if pending_size(&pending_dir)? + bytes.len() as u64 > MAX_SPOOL_BYTES {
            return Ok(EmitOutcome::DroppedQuota);
        }

        let file_id = Uuid::new_v4();
        let tmp_path = pending_dir.join(format!("batch-{file_id}.json.tmp"));
        let final_path = pending_dir.join(format!("batch-{file_id}.json"));
        write_atomic(&tmp_path, &final_path, &bytes)?;
        Ok(EmitOutcome::Spooled(final_path))
    }

    fn batch(
        &self,
        now: OffsetDateTime,
        event_id: String,
        event: TelemetryEvent,
    ) -> Result<TelemetryBatch, TelemetryError> {
        let mut batch = TelemetryBatch {
            schema_version: 1,
            app_version: self.app_version.clone(),
            app_channel: self.app_channel.clone(),
            os_family: os_family().to_string(),
            arch: arch().to_string(),
            telemetry_mode: telemetry_mode_value(self.settings.mode).to_string(),
            client_day: client_day(now),
            subject_id: None,
            events: vec![SpooledEvent {
                name: event.name,
                version: 1,
                event_id,
                fields: event.fields,
            }],
        };
        if self.settings.mode == TelemetryMode::Product {
            batch.subject_id = Some(product_subject_id(&self.root, now)?);
        }
        Ok(batch)
    }

    pub fn purge(state_root: impl AsRef<Path>) -> Result<(), TelemetryError> {
        Self::purge_root(state_root.as_ref().join("telemetry"))
    }

    pub fn purge_root(root: impl AsRef<Path>) -> Result<(), TelemetryError> {
        match fs::remove_dir_all(root) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(TelemetryError::Io(error)),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum EmitOutcome {
    Spooled(PathBuf),
    DroppedConsentOff,
    DroppedInvalidConsent,
    DroppedQuota,
}

#[derive(Clone, Debug)]
pub struct TelemetryEvent {
    name: String,
    fields: BTreeMap<String, Value>,
}

impl TelemetryEvent {
    pub fn app_startup_completed(duration: Duration, result: TelemetryResult) -> Self {
        Self {
            name: "app.startup.completed".to_string(),
            fields: BTreeMap::from([
                (
                    "duration_bucket".to_string(),
                    json!(duration_bucket(duration)),
                ),
                ("result".to_string(), json!(result.as_str())),
                ("reason_bucket".to_string(), json!("none")),
            ]),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum TelemetryResult {
    Ok,
    Failed,
    Cancelled,
}

impl TelemetryResult {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TelemetryBatch {
    schema_version: u32,
    app_version: String,
    app_channel: String,
    os_family: String,
    arch: String,
    telemetry_mode: String,
    client_day: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    subject_id: Option<String>,
    events: Vec<SpooledEvent>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SpooledEvent {
    name: String,
    version: u32,
    event_id: String,
    fields: BTreeMap<String, Value>,
}

fn product_subject_id(root: &Path, now: OffsetDateTime) -> Result<String, TelemetryError> {
    fs::create_dir_all(root)?;
    let secret_path = root.join("product-secret");
    let secret = match fs::read_to_string(&secret_path) {
        Ok(secret) => secret,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let secret = Uuid::new_v4().to_string();
            write_secret(&secret_path, secret.as_bytes())?;
            secret
        }
        Err(error) => return Err(TelemetryError::Io(error)),
    };
    let mut mac = HmacSha256::new_from_slice(secret.trim().as_bytes())
        .map_err(|_| TelemetryError::InvalidSecret)?;
    mac.update(
        format!(
            "posthaste-telemetry:v1:{:04}-{:02}",
            now.year(),
            u8::from(now.month())
        )
        .as_bytes(),
    );
    Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

fn write_atomic(tmp_path: &Path, final_path: &Path, bytes: &[u8]) -> Result<(), TelemetryError> {
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        set_owner_only_file(tmp_path)?;
    }
    fs::rename(tmp_path, final_path)?;
    sync_parent_dir(final_path)?;
    Ok(())
}

fn write_secret(path: &Path, bytes: &[u8]) -> Result<(), TelemetryError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    set_owner_only_file(path)?;
    sync_parent_dir(path)?;
    Ok(())
}

fn cleanup_pending(pending_dir: &Path) -> Result<(), TelemetryError> {
    let cutoff = SystemTime::now() - MAX_SPOOL_AGE;
    for entry in fs::read_dir(pending_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.modified().is_ok_and(|modified| modified < cutoff) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn pending_size(pending_dir: &Path) -> Result<u64, TelemetryError> {
    let mut total = 0;
    for entry in fs::read_dir(pending_dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            total += metadata.len();
        }
    }
    Ok(total)
}

#[cfg(unix)]
fn set_owner_only_dir(path: &Path) -> Result<(), TelemetryError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_dir(_path: &Path) -> Result<(), TelemetryError> {
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_file(path: &Path) -> Result<(), TelemetryError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_file(_path: &Path) -> Result<(), TelemetryError> {
    Ok(())
}

fn sync_parent_dir(path: &Path) -> Result<(), TelemetryError> {
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn telemetry_mode_value(mode: TelemetryMode) -> &'static str {
    match mode {
        TelemetryMode::Off => "off",
        TelemetryMode::Aggregate => "aggregate",
        TelemetryMode::Product => "product",
    }
}

fn os_family() -> &'static str {
    match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "macos",
        "windows" => "windows",
        _ => "unknown",
    }
}

fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        _ => "unknown",
    }
}

fn client_day(now: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        now.year(),
        u8::from(now.month()),
        now.day()
    )
}

fn duration_bucket(duration: Duration) -> &'static str {
    match duration.as_secs() {
        0 => "lt_1s",
        1..=4 => "s1_5",
        5..=14 => "s5_15",
        15..=59 => "s15_60",
        60..=299 => "m1_5",
        _ => "gt_5m",
    }
}

#[derive(Clone, Debug)]
pub struct UploadConfig {
    pub endpoint: String,
    pub ingest_token: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UploadOutcome {
    pub uploaded: usize,
    pub retained: usize,
    pub discarded: usize,
}

pub async fn upload_pending(
    telemetry_root: impl AsRef<Path>,
    config: &UploadConfig,
    client: &reqwest::Client,
) -> Result<UploadOutcome, TelemetryError> {
    let pending_dir = telemetry_root.as_ref().join("pending");
    if !pending_dir.exists() {
        return Ok(UploadOutcome::default());
    }
    cleanup_pending(&pending_dir)?;

    let mut outcome = UploadOutcome::default();
    for entry in fs::read_dir(&pending_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let bytes = fs::read(&path)?;
        let mut request = client
            .post(&config.endpoint)
            .header("content-type", "application/json")
            .body(bytes);
        if let Some(token) = &config.ingest_token {
            request = request.bearer_auth(token);
        }
        match request.send().await {
            Ok(response) if response.status().is_success() => {
                fs::remove_file(path)?;
                outcome.uploaded += 1;
            }
            Ok(response) if discard_after_status(response.status()) => {
                fs::remove_file(path)?;
                outcome.discarded += 1;
            }
            Ok(_) | Err(_) => {
                outcome.retained += 1;
            }
        }
    }
    Ok(outcome)
}

fn discard_after_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::BAD_REQUEST
            | StatusCode::PAYLOAD_TOO_LARGE
            | StatusCode::UNSUPPORTED_MEDIA_TYPE
            | StatusCode::UNPROCESSABLE_ENTITY
    )
}

pub fn default_telemetry_settings(mode: TelemetryMode) -> TelemetrySettings {
    match mode {
        TelemetryMode::Off => TelemetrySettings::default(),
        TelemetryMode::Aggregate | TelemetryMode::Product => TelemetrySettings {
            mode,
            notice_version: Some(NOTICE_VERSION.to_string()),
            enabled_at: Some(OffsetDateTime::now_utc().to_string()),
            categories: DEFAULT_CATEGORIES
                .iter()
                .map(|category| (*category).to_string())
                .collect(),
        },
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("telemetry filesystem error")]
    Io(#[from] std::io::Error),
    #[error("telemetry serialization error")]
    Serde(#[from] serde_json::Error),
    #[error("invalid telemetry product secret")]
    InvalidSecret,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_mode_does_not_create_spool_directory() {
        let root = temp_root();
        let spool = TelemetrySpool::new(
            &root,
            TelemetrySettings::default(),
            "0.1.0-beta.1".to_string(),
        );

        let outcome = spool
            .emit(TelemetryEvent::app_startup_completed(
                Duration::from_millis(250),
                TelemetryResult::Ok,
            ))
            .expect("emit");

        assert_eq!(outcome, EmitOutcome::DroppedConsentOff);
        assert!(!root.join("telemetry").exists());
    }

    #[test]
    fn aggregate_mode_writes_ingest_compatible_batch() {
        let root = temp_root();
        let spool = TelemetrySpool::new(
            &root,
            default_telemetry_settings(TelemetryMode::Aggregate),
            "0.1.0-beta.1".to_string(),
        );

        let outcome = spool
            .emit(TelemetryEvent::app_startup_completed(
                Duration::from_secs(3),
                TelemetryResult::Ok,
            ))
            .expect("emit");

        let EmitOutcome::Spooled(path) = outcome else {
            panic!("expected spooled batch");
        };
        let payload: Value = serde_json::from_slice(&fs::read(path).expect("batch")).expect("json");
        assert_eq!(payload["telemetryMode"], "aggregate");
        assert!(payload.get("subjectId").is_none());
        assert_eq!(payload["events"][0]["name"], "app.startup.completed");
        assert_eq!(payload["events"][0]["fields"]["duration_bucket"], "s1_5");
    }

    #[tokio::test]
    async fn upload_pending_posts_batch_and_deletes_on_success() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let root = temp_root();
        let spool = TelemetrySpool::new(
            &root,
            default_telemetry_settings(TelemetryMode::Aggregate),
            "0.1.0-beta.1".to_string(),
        );
        let _ = spool
            .emit(TelemetryEvent::app_startup_completed(
                Duration::from_secs(3),
                TelemetryResult::Ok,
            ))
            .expect("emit");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let endpoint = format!(
            "http://{}/telemetry/v1/batches",
            listener.local_addr().expect("addr")
        );
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut buffer = vec![0; 4096];
            let read = stream.read(&mut buffer).await.expect("read");
            let request = String::from_utf8_lossy(&buffer[..read]);
            assert!(request.starts_with("POST /telemetry/v1/batches HTTP/1.1"));
            assert!(request
                .to_ascii_lowercase()
                .contains("content-type: application/json"));
            assert!(request
                .to_ascii_lowercase()
                .contains("authorization: bearer shared-token"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}")
                .await
                .expect("write");
        });

        let outcome = upload_pending(
            root.join("telemetry"),
            &UploadConfig {
                endpoint,
                ingest_token: Some("shared-token".to_string()),
            },
            &reqwest::Client::new(),
        )
        .await
        .expect("upload");
        server.await.expect("server");

        assert_eq!(outcome.uploaded, 1);
        assert!(fs::read_dir(root.join("telemetry/pending"))
            .expect("pending")
            .next()
            .is_none());
    }

    #[test]
    fn aggregate_mode_drops_when_spool_quota_is_full() {
        let root = temp_root();
        let pending = root.join("telemetry/pending");
        fs::create_dir_all(&pending).expect("pending");
        fs::write(
            pending.join("existing.json"),
            vec![b'x'; MAX_SPOOL_BYTES as usize],
        )
        .expect("existing batch");
        let spool = TelemetrySpool::new(
            &root,
            default_telemetry_settings(TelemetryMode::Aggregate),
            "0.1.0-beta.1".to_string(),
        );

        let outcome = spool
            .emit(TelemetryEvent::app_startup_completed(
                Duration::from_secs(3),
                TelemetryResult::Ok,
            ))
            .expect("emit");

        assert_eq!(outcome, EmitOutcome::DroppedQuota);
    }

    #[test]
    fn purge_deletes_pending_batches_and_product_secret() {
        let root = temp_root();
        let spool = TelemetrySpool::new(
            &root,
            default_telemetry_settings(TelemetryMode::Product),
            "0.1.0-beta.1".to_string(),
        );
        let _ = spool
            .emit(TelemetryEvent::app_startup_completed(
                Duration::from_secs(3),
                TelemetryResult::Ok,
            ))
            .expect("emit");
        assert!(root.join("telemetry").exists());

        TelemetrySpool::purge(&root).expect("purge");

        assert!(!root.join("telemetry").exists());
    }

    #[cfg(unix)]
    #[test]
    fn spooled_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_root();
        let spool = TelemetrySpool::new(
            &root,
            default_telemetry_settings(TelemetryMode::Aggregate),
            "0.1.0-beta.1".to_string(),
        );

        let outcome = spool
            .emit(TelemetryEvent::app_startup_completed(
                Duration::from_secs(3),
                TelemetryResult::Ok,
            ))
            .expect("emit");
        let EmitOutcome::Spooled(path) = outcome else {
            panic!("expected spooled batch");
        };

        assert_eq!(
            fs::metadata(root.join("telemetry"))
                .expect("root metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(path)
                .expect("file metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("posthaste-telemetry-test-{}", Uuid::new_v4()))
    }
}
