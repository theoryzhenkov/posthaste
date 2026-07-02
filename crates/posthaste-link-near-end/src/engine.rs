//! The near-end engine: one place for the link's consuming half + its
//! resilience policy (D40/D44).
//!
//! Instantiated per seam (browser via wasm, native in-process later), the engine
//! owns:
//!
//! * **session lifecycle** — opens a session (POST) and holds its id;
//! * **the frame subscription** — a reconnect loop that re-subscribes with the
//!   engine-owned resume **cursor** (`after_seq`) on *every* reconnect (fixing
//!   the TS closure that dropped it), parses each payload into a typed
//!   [`RuntimeFrame`] at the boundary (no unchecked cast), and classifies stream
//!   errors permanent-vs-transient;
//! * **`forward`** — a mutation POST with a request **deadline** and jittered
//!   backoff retry of transient failures;
//! * **the level-triggered reconciler** — runs on *every* connect (first
//!   included) and replays never-dispatched forwards via the outbox hooks.
//!
//! No timers, no HTTP client, no persistence live here — those are the injected
//! [`Transport`]/[`Scheduler`]/[`FrameSink`]/[`OutboxHooks`]. That is what keeps
//! the crate wasm-pure.

use std::cell::RefCell;
use std::rc::Rc;

use futures_util::future::{select, Either};
use futures_util::StreamExt;

use posthaste_contract_core::{
    MutationReceipt, MutationRequest, RuntimeAdapterError, RuntimeFrame, RuntimeSession,
    RuntimeSessionId, RuntimeSessionSeq,
};

use crate::config::NearEndConfig;
use crate::error::{classify_status, Disposition};
use crate::outbox::OutboxHooks;
use crate::scheduler::Scheduler;
use crate::sink::{ConnectionStatus, FrameSink};
use crate::transport::{PostRequest, PostResponse, StreamRequest, Transport, TransportError};

/// A failure surfaced by an engine call, tagged with its [`Disposition`] so the
/// caller (and the wasm boundary) can react. Mirrors the runtime error envelope
/// when the failure carried one (a 4xx body).
#[derive(Clone, Debug)]
pub struct EngineError {
    pub disposition: Disposition,
    pub message: String,
    pub status: Option<u16>,
    pub error: Option<RuntimeAdapterError>,
}

impl EngineError {
    pub fn transient(message: impl Into<String>) -> Self {
        Self {
            disposition: Disposition::Transient,
            message: message.into(),
            status: None,
            error: None,
        }
    }

    pub fn permanent(message: impl Into<String>) -> Self {
        Self {
            disposition: Disposition::Permanent,
            message: message.into(),
            status: None,
            error: None,
        }
    }

    /// Build from a non-2xx response body, trying to recover the runtime error
    /// envelope for a precise message.
    fn from_response(status: u16, body: &str) -> Self {
        let error = serde_json::from_str::<RuntimeAdapterError>(body).ok();
        let message = error
            .as_ref()
            .map(|e| e.message.clone())
            .unwrap_or_else(|| format!("runtime responded with status {status}"));
        Self {
            disposition: classify_status(status),
            message,
            status: Some(status),
            error,
        }
    }

    pub fn is_permanent(&self) -> bool {
        self.disposition.is_permanent()
    }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for EngineError {}

#[derive(Default)]
struct State {
    session_id: Option<RuntimeSessionId>,
    cursor: Option<RuntimeSessionSeq>,
    reconnect_attempt: u32,
    shutdown: bool,
}

/// The near-end engine. Held behind `Rc` so its long-lived frame loop
/// ([`Self::run`]) and its short one-shot calls ([`Self::forward`]) share one
/// state without a lifetime.
pub struct NearEnd {
    transport: Rc<dyn Transport>,
    scheduler: Rc<dyn Scheduler>,
    sink: Rc<dyn FrameSink>,
    outbox: Rc<dyn OutboxHooks>,
    config: NearEndConfig,
    state: RefCell<State>,
}

impl NearEnd {
    pub fn new(
        transport: Rc<dyn Transport>,
        scheduler: Rc<dyn Scheduler>,
        sink: Rc<dyn FrameSink>,
        outbox: Rc<dyn OutboxHooks>,
        config: NearEndConfig,
    ) -> Rc<Self> {
        let cursor = config.initial_cursor.map(RuntimeSessionSeq::new);
        Rc::new(Self {
            transport,
            scheduler,
            sink,
            outbox,
            config,
            state: RefCell::new(State {
                cursor,
                ..State::default()
            }),
        })
    }

    /// The current session id, once [`Self::open`] has run.
    pub fn session_id(&self) -> Option<RuntimeSessionId> {
        self.state.borrow().session_id.clone()
    }

    /// The engine-owned resume cursor (last seen `sessionSeq`). The host persists
    /// this so a reload resumes where it left off — callers no longer thread
    /// `afterSeq` themselves.
    pub fn cursor(&self) -> Option<u64> {
        self.state.borrow().cursor.map(|c| c.get())
    }

    /// Ask the frame loop to stop (no further reconnects). Idempotent. Aborting
    /// an *idle* open stream promptly is the host's concern (it drops the task).
    pub fn request_shutdown(&self) {
        self.state.borrow_mut().shutdown = true;
    }

    fn is_shutdown(&self) -> bool {
        self.state.borrow().shutdown
    }

    // ---- session -----------------------------------------------------------

    /// Open a session (idempotent). POSTs `/runtime/sessions` and stores the id.
    pub async fn open(&self) -> Result<RuntimeSessionId, EngineError> {
        if let Some(id) = self.session_id() {
            return Ok(id);
        }
        let url = self.session_open_url();
        let resp = self
            .post_with_deadline(&url, "")
            .await
            .map_err(|e| EngineError::transient(e.message))?;
        if !(200..300).contains(&resp.status) {
            return Err(EngineError::from_response(resp.status, &resp.body));
        }
        let session: RuntimeSession = serde_json::from_str(&resp.body)
            .map_err(|e| EngineError::permanent(format!("parse session: {e}")))?;
        self.state.borrow_mut().session_id = Some(session.session_id.clone());
        Ok(session.session_id)
    }

    // ---- forward -----------------------------------------------------------

    /// Forward a mutation with the request deadline + transient-retry policy.
    /// Stamps the current session id onto the request (typed round-trip: parse
    /// in, serialize out — the mutation crosses the wire as a validated
    /// `MailOperation`, never a raw cast). Returns the receipt on 2xx (including
    /// an authority `Failed` verdict), a permanent error on 4xx, or a transient
    /// error once retries are exhausted.
    pub async fn forward(&self, mut request: MutationRequest) -> Result<MutationReceipt, EngineError> {
        let session_id = self
            .session_id()
            .ok_or_else(|| EngineError::transient("forward before session open"))?;
        request.session_id = Some(session_id.clone());
        let url = self.mutation_url(&session_id);
        let body = serde_json::to_string(&request)
            .map_err(|e| EngineError::permanent(format!("serialize mutation: {e}")))?;

        let mut attempt = 0u32;
        loop {
            match self.post_with_deadline(&url, &body).await {
                Ok(resp) if (200..300).contains(&resp.status) => {
                    return serde_json::from_str::<MutationReceipt>(&resp.body)
                        .map_err(|e| EngineError::permanent(format!("parse receipt: {e}")));
                }
                Ok(resp) => {
                    if classify_status(resp.status).is_permanent() {
                        return Err(EngineError::from_response(resp.status, &resp.body));
                    }
                    // transient status (5xx): fall through to retry
                }
                Err(_transient) => {
                    // network/deadline: fall through to retry
                }
            }
            attempt += 1;
            if attempt >= self.config.forward_max_attempts {
                return Err(EngineError::transient(format!(
                    "forward exhausted after {attempt} attempts"
                )));
            }
            let dur = self
                .config
                .backoff
                .sleep_for(attempt, self.scheduler.jitter());
            self.scheduler.sleep(dur).await;
        }
    }

    async fn post_with_deadline(
        &self,
        url: &str,
        body: &str,
    ) -> Result<PostResponse, TransportError> {
        let post = self.transport.post_json(PostRequest {
            url: url.to_string(),
            headers: json_headers(),
            body: body.to_string(),
        });
        let deadline = self.scheduler.sleep(self.config.request_deadline);
        match select(post, deadline).await {
            Either::Left((result, _)) => result,
            // Deadline won: dropping `post` cancels the in-flight request.
            Either::Right(((), _)) => Err(TransportError::new("request deadline exceeded")),
        }
    }

    // ---- frame loop --------------------------------------------------------

    /// The reconnect loop. Runs until a permanent error or [`Self::request_shutdown`].
    /// The host spawns this once (e.g. `spawn_local`) after constructing the
    /// engine. Re-subscribes with the resume cursor on every reconnect, resets
    /// backoff on a clean open, and runs the reconciler on every open.
    pub async fn run(self: Rc<Self>) {
        loop {
            if self.is_shutdown() {
                return;
            }

            // Ensure a session before subscribing.
            if self.session_id().is_none() {
                self.sink.on_status(ConnectionStatus::Connecting);
                if let Err(e) = self.open().await {
                    if e.is_permanent() {
                        self.sink.on_status(ConnectionStatus::PermanentError(e.message));
                        return;
                    }
                    self.sink.on_status(ConnectionStatus::TransientError(e.message));
                    self.backoff_before_reconnect().await;
                    continue;
                }
            }

            self.sink.on_status(ConnectionStatus::Connecting);
            let request = StreamRequest {
                url: self.stream_url(),
                headers: Vec::new(),
            };
            let mut stream = self.transport.open_stream(request);

            let mut permanent = false;
            while let Some(event) = stream.next().await {
                match event {
                    crate::transport::StreamEvent::Open => {
                        self.state.borrow_mut().reconnect_attempt = 0;
                        self.sink.on_status(ConnectionStatus::Connected);
                        // Level-triggered reconciler: every connect, first included.
                        self.clone().reconcile().await;
                    }
                    crate::transport::StreamEvent::Message(data) => {
                        self.handle_message(data);
                    }
                    crate::transport::StreamEvent::Closed => break,
                    crate::transport::StreamEvent::Error { status, message } => {
                        if status.map(classify_status) == Some(Disposition::Permanent) {
                            self.sink.on_status(ConnectionStatus::PermanentError(message));
                            permanent = true;
                        } else {
                            self.sink.on_status(ConnectionStatus::TransientError(message));
                        }
                        break;
                    }
                }
                if self.is_shutdown() {
                    return;
                }
            }
            // Drop the stream so the host aborts the underlying connection.
            drop(stream);

            if permanent || self.is_shutdown() {
                return;
            }
            self.sink.on_status(ConnectionStatus::Reconnecting);
            self.backoff_before_reconnect().await;
        }
    }

    fn handle_message(&self, data: String) {
        // Skip SSE keep-alive / empty payloads (parity with the TS `!event.data`).
        if data.trim().is_empty() {
            return;
        }
        match serde_json::from_str::<RuntimeFrame>(&data) {
            Ok(frame) => {
                let seq = frame.session_seq();
                {
                    let mut state = self.state.borrow_mut();
                    if state.cursor.is_none_or(|c| seq.get() > c.get()) {
                        state.cursor = Some(seq);
                    }
                }
                self.sink.on_frame(frame);
            }
            Err(e) => self.sink.on_malformed(data, e.to_string()),
        }
    }

    async fn backoff_before_reconnect(&self) {
        let attempt = {
            let mut state = self.state.borrow_mut();
            let a = state.reconnect_attempt;
            state.reconnect_attempt = state.reconnect_attempt.saturating_add(1);
            a
        };
        let dur = self.config.backoff.sleep_for(attempt, self.scheduler.jitter());
        self.scheduler.sleep(dur).await;
    }

    // ---- reconciler --------------------------------------------------------

    /// Replay never-dispatched forwards (D44a). Safe on every connect: a
    /// never-dispatched request never reached the server, so re-forwarding it
    /// cannot double-apply; a same-session duplicate `clientMutationId` is
    /// deduped by the runtime. Sent-but-unsettled reconciliation is handled by
    /// the cursor-owned resubscribe (session-collapse re-delivers the terminal
    /// notification frames) — those records are *not* in `never_dispatched`.
    async fn reconcile(self: Rc<Self>) {
        let pending = self.outbox.never_dispatched().await;
        for request in pending {
            // A transient/permanent replay failure surfaces through the normal
            // frame/settlement stream; the record stays for the next connect.
            if let Ok(receipt) = self.forward(request).await {
                self.outbox.on_reconciled(receipt).await;
            }
        }
    }

    // ---- url building ------------------------------------------------------

    fn session_open_url(&self) -> String {
        let mut query = Vec::new();
        if self.config.view_delta {
            query.push("viewDelta=true".to_string());
        }
        if let Some(source) = &self.config.source_id {
            query.push(format!("sourceId={source}"));
        }
        format!(
            "{}/runtime/sessions{}",
            self.config.base_url,
            query_string(&query)
        )
    }

    fn mutation_url(&self, session_id: &RuntimeSessionId) -> String {
        let mut query = Vec::new();
        if let Some(source) = &self.config.source_id {
            query.push(format!("sourceId={source}"));
        }
        format!(
            "{}/runtime/sessions/{}/mutations{}",
            self.config.base_url,
            session_id.as_str(),
            query_string(&query)
        )
    }

    fn stream_url(&self) -> String {
        let session = self.session_id().map(|s| s.as_str().to_string()).unwrap_or_default();
        let mut query = Vec::new();
        if let Some(cursor) = self.cursor() {
            query.push(format!("afterSeq={cursor}"));
        }
        if let Some(source) = &self.config.source_id {
            query.push(format!("sourceId={source}"));
        }
        format!(
            "{}/runtime/sessions/{}/stream{}",
            self.config.base_url,
            session,
            query_string(&query)
        )
    }
}

fn json_headers() -> Vec<(String, String)> {
    vec![("content-type".to_string(), "application/json".to_string())]
}

fn query_string(parts: &[String]) -> String {
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}

#[cfg(test)]
mod tests;
