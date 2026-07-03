//! The native outbound-call executor (D80/D83, M31) — the *execution* half over
//! the wasm-pure [`posthaste_call_policy`] core.
//!
//! One executor owns one shared `reqwest::Client` (the F4 fix: a single reused
//! connection pool instead of a fresh client per mutation), the ratified
//! [`BackoffSchedule`], and the per-account circuit-breaker table. Every call
//! flows through [`ProviderCallExecutor::execute`], which:
//!
//! 1. consults the breaker (fast-fail with a distinct reason if open, D83);
//! 2. selects the per-class deadline from [`CallClass::deadline_policy`] — a
//!    *total* for metadata/send, a between-chunks *stall* for blobs (D81/F2);
//! 3. runs the attempt, retrying only [`Terminality::Transient`] outcomes on the
//!    `Retry-After`-aware jittered schedule (F1); and
//! 4. records the terminal outcome against the breaker.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use posthaste_call_policy::{
    resolve_terminality, BackoffSchedule, CallClass, RetryDecision, Terminality, METADATA_TOTAL,
};
use reqwest::header::HeaderMap;
use reqwest::{redirect, Client, Method};
use time::OffsetDateTime;

use crate::breaker::{BreakerConfig, BreakerPhaseView, BreakerRegistry};
use crate::error::{CallErrorReason, ProviderCallError};
use crate::retry_after::parse_retry_after;
use crate::stall::drain_with_stall;

/// Construction parameters for a [`ProviderCallExecutor`].
#[derive(Clone, Debug)]
pub struct ExecutorConfig {
    /// The retry schedule (jittered backoff + `Retry-After` arithmetic + give-up).
    pub schedule: BackoffSchedule,
    /// The circuit-breaker parameters (D83/O4).
    pub breaker: BreakerConfig,
    /// Hosts a redirect may target. Empty ⇒ redirects are refused (the safe
    /// default the raw JMAP POST used); populated ⇒ redirects to those hosts are
    /// followed (blob downloads that bounce to a CDN on the same host).
    pub trusted_hosts: Vec<String>,
    /// Connection-establishment ceiling (not a total request timeout — that is
    /// per-class). `None` leaves reqwest's default.
    pub connect_timeout: Option<Duration>,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            schedule: BackoffSchedule::default(),
            breaker: BreakerConfig::default(),
            trusted_hosts: Vec::new(),
            connect_timeout: Some(Duration::from_secs(30)),
        }
    }
}

/// A fully-specified outbound HTTP request the executor will drive. Deliberately
/// transport-level: the executor owns the wire, the caller owns the protocol
/// (it builds this spec and later decodes [`ProviderResponse::body`]).
pub struct HttpRequestSpec {
    /// HTTP method.
    pub method: Method,
    /// Absolute request URL.
    pub url: String,
    /// Request headers (auth, content-type, …).
    pub headers: HeaderMap,
    /// Optional request body.
    pub body: Option<Vec<u8>>,
    /// An optional typed terminality from a posthaste wire envelope: when
    /// present it *wins* over the HTTP status band (D82 precedence via
    /// [`resolve_terminality`]). JMAP callers leave this `None` and let the band
    /// decide.
    pub envelope_terminality: Option<Terminality>,
}

impl HttpRequestSpec {
    /// A `POST` with a body (the JMAP raw-request path).
    pub fn post(url: impl Into<String>, headers: HeaderMap, body: Vec<u8>) -> Self {
        Self {
            method: Method::POST,
            url: url.into(),
            headers,
            body: Some(body),
            envelope_terminality: None,
        }
    }

    /// A `GET` with no body (the blob-download path).
    pub fn get(url: impl Into<String>, headers: HeaderMap) -> Self {
        Self {
            method: Method::GET,
            url: url.into(),
            headers,
            body: None,
            envelope_terminality: None,
        }
    }
}

/// A successful (2xx) provider response, body fully buffered.
#[derive(Clone, Debug)]
pub struct ProviderResponse {
    /// The HTTP status (always 2xx here).
    pub status: u16,
    /// The response body bytes (JSON to decode, or blob bytes).
    pub body: Vec<u8>,
}

/// The outbound-call envelope: shared client + policy + per-account breaker.
pub struct ProviderCallExecutor {
    http: Client,
    schedule: BackoffSchedule,
    breakers: BreakerRegistry,
    jitter: AtomicU64,
}

impl ProviderCallExecutor {
    /// Build an executor (and its single shared connection pool) from `config`.
    pub fn new(config: ExecutorConfig) -> Result<Self, reqwest::Error> {
        let trusted = config.trusted_hosts.clone();
        let redirect_policy = redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() > 5 {
                attempt.error("too many redirects")
            } else if attempt
                .url()
                .host_str()
                .is_some_and(|host| trusted.iter().any(|t| t == host))
            {
                attempt.follow()
            } else {
                attempt.stop()
            }
        });
        let mut builder = Client::builder().redirect(redirect_policy);
        if let Some(connect) = config.connect_timeout {
            builder = builder.connect_timeout(connect);
        }
        Ok(Self {
            http: builder.build()?,
            schedule: config.schedule,
            breakers: BreakerRegistry::new(config.breaker),
            // Seed the jitter PRNG off the wall clock; quality here only needs to
            // decorrelate retry storms, not be cryptographic.
            jitter: AtomicU64::new(seed()),
        })
    }

    /// Execute one logical provider call for `account`: breaker gate → per-class
    /// deadline → `Retry-After`-aware retry loop → breaker record.
    pub async fn execute(
        &self,
        account: &str,
        class: CallClass,
        spec: HttpRequestSpec,
    ) -> Result<ProviderResponse, ProviderCallError> {
        let policy = class.deadline_policy();
        if let Some(stall) = policy.stall {
            // Blob: no total; a between-chunks stall read-deadline (F2).
            self.run(account, || self.attempt_streamed(&spec, stall))
                .await
        } else {
            // Metadata/Send/Subscribe: a total wall-clock deadline.
            let total = policy.total.unwrap_or(METADATA_TOTAL);
            self.run(account, || self.attempt_buffered(&spec, total))
                .await
        }
    }

    /// A read-only view of an account's breaker phase (status surfacing/tests).
    pub fn breaker_phase(&self, account: &str) -> BreakerPhaseView {
        self.breakers.phase(account)
    }

    /// The breaker + retry loop, generic over the per-attempt future so the pure
    /// control flow is exercised in tests with a scripted attempt (no socket).
    pub(crate) async fn run<T, F, Fut>(
        &self,
        account: &str,
        mut attempt: F,
    ) -> Result<T, ProviderCallError>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, ProviderCallError>>,
    {
        if !self.breakers.admit(account) {
            return Err(ProviderCallError::circuit_open(account));
        }
        let mut n = 0u32;
        let outcome = loop {
            match attempt().await {
                Ok(value) => break Ok(value),
                Err(error) => {
                    if error.terminality.is_permanent() {
                        break Err(error);
                    }
                    match self
                        .schedule
                        .retry_delay(n, self.next_rand_unit(), error.retry_after)
                    {
                        RetryDecision::GiveUp => break Err(error),
                        RetryDecision::Retry(delay) => {
                            tokio::time::sleep(delay).await;
                            n += 1;
                        }
                    }
                }
            }
        };
        self.breakers.record(account, outcome.is_ok());
        outcome
    }

    /// One buffered attempt under a single `total` wall-clock deadline covering
    /// send + status classification + body read.
    async fn attempt_buffered(
        &self,
        spec: &HttpRequestSpec,
        total: Duration,
    ) -> Result<ProviderResponse, ProviderCallError> {
        let call = async {
            let response = self.send(spec).await.map_err(transport)?;
            let status = response.status().as_u16();
            if (200..300).contains(&status) {
                let body = response.bytes().await.map_err(transport)?;
                Ok(ProviderResponse {
                    status,
                    body: body.to_vec(),
                })
            } else {
                Err(http_error(status, response, spec).await)
            }
        };
        match tokio::time::timeout(total, call).await {
            Ok(result) => result,
            Err(_elapsed) => Err(ProviderCallError::timeout(format!(
                "call exceeded total deadline {total:?}"
            ))),
        }
    }

    /// One streamed attempt: the connect/headers wait and every subsequent chunk
    /// are each bounded by `stall` — no total, so a large-but-progressing body
    /// completes (F2), while a genuinely dead read fails fast.
    async fn attempt_streamed(
        &self,
        spec: &HttpRequestSpec,
        stall: Duration,
    ) -> Result<ProviderResponse, ProviderCallError> {
        let response = match tokio::time::timeout(stall, self.send(spec)).await {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => return Err(transport(error)),
            Err(_elapsed) => {
                return Err(ProviderCallError::stall(format!(
                    "no response headers within stall deadline {stall:?}"
                )))
            }
        };
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(http_error(status, response, spec).await);
        }
        match drain_with_stall(response.bytes_stream(), stall).await {
            Ok(body) => Ok(ProviderResponse { status, body }),
            Err(None) => Err(ProviderCallError::stall(format!(
                "blob stalled: no bytes within stall deadline {stall:?}"
            ))),
            Err(Some(error)) => Err(transport(error)),
        }
    }

    /// Build and dispatch the request on the shared client.
    async fn send(&self, spec: &HttpRequestSpec) -> Result<reqwest::Response, reqwest::Error> {
        let mut request = self
            .http
            .request(spec.method.clone(), &spec.url)
            .headers(spec.headers.clone());
        if let Some(body) = &spec.body {
            request = request.body(body.clone());
        }
        request.send().await
    }

    /// Draw a jitter value in `[0.0, 1.0)` (xorshift64; see [`Self::new`]).
    fn next_rand_unit(&self) -> f64 {
        let mut x = self.jitter.load(Ordering::Relaxed);
        if x == 0 {
            x = 0x9E37_79B9_7F4A_7C15;
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.jitter.store(x, Ordering::Relaxed);
        // Top 53 bits → a double in [0, 1).
        (x >> 11) as f64 / ((1u64 << 53) as f64)
    }
}

fn transport(error: impl std::fmt::Display) -> ProviderCallError {
    ProviderCallError::transport(error.to_string())
}

/// Classify a non-2xx response (D82), consuming it to read the diagnostic body.
/// 429/503 are the retry-eligible rate-limit band (and carry `Retry-After`);
/// every other status defers to the envelope-over-band precedence rule — the
/// typed envelope, when present, wins over the status band.
async fn http_error(
    status: u16,
    response: reqwest::Response,
    spec: &HttpRequestSpec,
) -> ProviderCallError {
    let retry_after = parse_retry_after(response.headers(), OffsetDateTime::now_utc());
    let body = response.text().await.unwrap_or_default();
    let detail = format!("HTTP {status}: {}", truncate(&body));
    if status == 429 || status == 503 {
        ProviderCallError {
            terminality: spec.envelope_terminality.unwrap_or(Terminality::Transient),
            reason: CallErrorReason::RateLimited(status),
            retry_after,
            detail,
        }
    } else {
        ProviderCallError {
            terminality: resolve_terminality(spec.envelope_terminality, status),
            reason: CallErrorReason::Http(status),
            retry_after,
            detail,
        }
    }
}

fn truncate(body: &str) -> String {
    const MAX: usize = 512;
    if body.len() <= MAX {
        body.to_string()
    } else {
        format!("{}…", &body[..MAX])
    }
}

fn seed() -> u64 {
    // Truncating i128 → u64 is fine: this only seeds a jitter PRNG.
    (OffsetDateTime::now_utc().unix_timestamp_nanos() as u64) | 1
}
