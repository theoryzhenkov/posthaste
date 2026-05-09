use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;

use crate::{
    config::Config,
    schema::TelemetryBatch,
    storage::{IngestOutcome, Store},
    validation::{validate_batch, ValidationError},
};

#[derive(Clone)]
pub struct AppState {
    config: Config,
    store: Arc<Store>,
    rate_limit: Arc<Mutex<VecDeque<Instant>>>,
}

pub fn router(config: Config, store: Arc<Store>) -> Router {
    let max_body_bytes = config.max_body_bytes;
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/telemetry/v1/batches", post(ingest_batch))
        .layer(DefaultBodyLimit::max(max_body_bytes))
        .with_state(AppState {
            config,
            store,
            rate_limit: Arc::new(Mutex::new(VecDeque::new())),
        })
}

async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn readyz(State(state): State<AppState>) -> Result<Json<HealthResponse>, ApiError> {
    if state.store.ready() {
        Ok(Json(HealthResponse { status: "ok" }))
    } else {
        Err(ApiError::unavailable())
    }
}

async fn ingest_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    if state.config.disabled {
        return Err(ApiError::unavailable());
    }
    require_json_content_type(&headers)?;
    require_ingest_token(&state.config, &headers)?;
    enforce_rate_limit(&state)?;

    let batch: TelemetryBatch = serde_json::from_slice(&body).map_err(|_| ApiError::invalid())?;
    validate_batch(&batch, state.config.max_events_per_batch).map_err(ApiError::from)?;
    let outcome = state
        .store
        .ingest(&batch)
        .map_err(|_| ApiError::unavailable())?;
    Ok((
        [("cache-control", "no-store")],
        Json(IngestResponse {
            status: "ok",
            outcome,
        }),
    )
        .into_response())
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct IngestResponse {
    status: &'static str,
    #[serde(flatten)]
    outcome: IngestOutcome,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

fn require_json_content_type(headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(content_type) = headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
    else {
        return Err(ApiError::unsupported_media_type());
    };
    if content_type
        .split(';')
        .next()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
    {
        Ok(())
    } else {
        Err(ApiError::unsupported_media_type())
    }
}

fn require_ingest_token(config: &Config, headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(expected) = &config.ingest_token else {
        return Ok(());
    };
    let supplied = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if supplied == Some(expected.as_str()) {
        Ok(())
    } else {
        Err(ApiError::unauthorized())
    }
}

fn enforce_rate_limit(state: &AppState) -> Result<(), ApiError> {
    let limit = state.config.rate_limit_per_minute;
    if limit == 0 {
        return Err(ApiError::rate_limited());
    }
    let now = Instant::now();
    let window_start = now - Duration::from_secs(60);
    let mut requests = state
        .rate_limit
        .lock()
        .map_err(|_| ApiError::unavailable())?;
    while requests
        .front()
        .is_some_and(|instant| *instant < window_start)
    {
        requests.pop_front();
    }
    if requests.len() >= limit {
        return Err(ApiError::rate_limited());
    }
    requests.push_back(now);
    Ok(())
}

impl ApiError {
    fn invalid() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "INVALID_TELEMETRY_PAYLOAD",
            message: "Invalid telemetry payload",
        }
    }

    fn unsupported_media_type() -> Self {
        Self {
            status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
            code: "UNSUPPORTED_MEDIA_TYPE",
            message: "Telemetry batches must be sent as JSON",
        }
    }

    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "UNAUTHORIZED",
            message: "Telemetry ingestion is unauthorized",
        }
    }

    fn rate_limited() -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "RATE_LIMITED",
            message: "Telemetry ingestion is rate limited",
        }
    }

    fn unavailable() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "TELEMETRY_UNAVAILABLE",
            message: "Telemetry ingestion is unavailable",
        }
    }
}

impl From<ValidationError> for ApiError {
    fn from(_: ValidationError) -> Self {
        Self::invalid()
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            [
                ("cache-control", "no-store"),
                ("content-type", "application/json"),
            ],
            Json(ErrorBody {
                error: ErrorDetails {
                    code: self.code,
                    message: self.message,
                },
            }),
        )
            .into_response()
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorDetails,
}

#[derive(Serialize)]
struct ErrorDetails {
    code: &'static str,
    message: &'static str,
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use serde_json::json;
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn ingest_accepts_valid_payload_and_deduplicates_retries() {
        let (router, store) = test_router();
        let payload = valid_payload("9fb18840-1a4b-4f0a-b94d-9c5e4a8c40c2");

        let first = router
            .clone()
            .oneshot(request(payload.clone()))
            .await
            .expect("response");
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(
            first
                .headers()
                .get("cache-control")
                .and_then(|value| value.to_str().ok()),
            Some("no-store")
        );

        let retry = router.oneshot(request(payload)).await.expect("response");
        assert_eq!(retry.status(), StatusCode::OK);
        assert_eq!(store.event_count().expect("count"), 1);
    }

    #[tokio::test]
    async fn ingest_rejects_sensitive_values_without_storing() {
        let (router, store) = test_router();
        let mut payload = valid_payload("9fb18840-1a4b-4f0a-b94d-9c5e4a8c40c2");
        payload["events"][0]["fields"]["result"] = json!("alice@example.com");

        let response = router.oneshot(request(payload)).await.expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(store.event_count().expect("count"), 0);
    }

    #[tokio::test]
    async fn ingest_requires_json_content_type() {
        let (router, store) = test_router();
        let payload = valid_payload("9fb18840-1a4b-4f0a-b94d-9c5e4a8c40c2");
        let request = Request::post("/telemetry/v1/batches")
            .header("content-type", "text/plain")
            .body(Body::from(payload.to_string()))
            .expect("request");

        let response = router.oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(store.event_count().expect("count"), 0);
    }

    #[tokio::test]
    async fn ingest_requires_configured_bearer_token() {
        let (router, store) = test_router_with_token("shared-beta-token");
        let payload = valid_payload("9fb18840-1a4b-4f0a-b94d-9c5e4a8c40c2");

        let response = router.oneshot(request(payload)).await.expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(store.event_count().expect("count"), 0);
    }

    #[tokio::test]
    async fn ingest_rate_limits_when_configured_limit_is_exceeded() {
        let (router, store) = test_router_with_rate_limit(1);

        let first = router
            .clone()
            .oneshot(request(valid_payload(
                "9fb18840-1a4b-4f0a-b94d-9c5e4a8c40c2",
            )))
            .await
            .expect("response");
        let second = router
            .oneshot(request(valid_payload(
                "a72eca64-004f-4109-a316-4203080a9105",
            )))
            .await
            .expect("response");

        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(store.event_count().expect("count"), 1);
    }

    fn test_router() -> (Router, Arc<Store>) {
        test_router_with_optional_token(None)
    }

    fn test_router_with_token(token: &str) -> (Router, Arc<Store>) {
        test_router_with_optional_token(Some(token.to_string()))
    }

    fn test_router_with_rate_limit(rate_limit_per_minute: usize) -> (Router, Arc<Store>) {
        test_router_with_options(None, rate_limit_per_minute)
    }

    fn test_router_with_optional_token(ingest_token: Option<String>) -> (Router, Arc<Store>) {
        test_router_with_options(ingest_token, 60)
    }

    fn test_router_with_options(
        ingest_token: Option<String>,
        rate_limit_per_minute: usize,
    ) -> (Router, Arc<Store>) {
        let dir = std::env::temp_dir().join(format!(
            "posthaste-telemetry-ingest-test-{}",
            uuid::Uuid::new_v4()
        ));
        let store = Arc::new(Store::open(dir.join("telemetry.sqlite3")).expect("store"));
        let config = Config {
            bind: "127.0.0.1:0".parse().expect("bind"),
            database_path: dir.join("telemetry.sqlite3"),
            max_body_bytes: 262_144,
            max_events_per_batch: 100,
            raw_retention_days: 30,
            dedupe_retention_days: 7,
            disabled: false,
            ingest_token,
            rate_limit_per_minute,
        };
        (router(config, Arc::clone(&store)), store)
    }

    fn request(payload: serde_json::Value) -> Request<Body> {
        Request::post("/telemetry/v1/batches")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("request")
    }

    fn valid_payload(event_id: &str) -> serde_json::Value {
        json!({
            "schemaVersion": 1,
            "appVersion": "0.1.0-beta.1",
            "appChannel": "beta",
            "osFamily": "linux",
            "arch": "x86_64",
            "telemetryMode": "aggregate",
            "clientDay": "2026-05-09",
            "events": [{
                "name": "app.startup.completed",
                "version": 1,
                "eventId": event_id,
                "fields": {
                    "duration_bucket": "s1_5",
                    "result": "ok",
                    "reason_bucket": "none"
                }
            }]
        })
    }
}
