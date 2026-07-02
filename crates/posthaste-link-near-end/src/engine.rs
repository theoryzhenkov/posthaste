//! The near-end engine: one place for the link's consuming half + its
//! resilience policy (D40/D44).
//!
//! Instantiated per seam over a [`Wire`] profile (the browser's client↔runtime
//! wire via wasm, the runtime's authority-server wire natively), the engine
//! owns:
//!
//! * **connection lifecycle** — runs the wire's prepare step (a link open,
//!   when the wire has one) and holds its token;
//! * **the frame subscription** — a reconnect loop that re-subscribes with the
//!   engine-owned resume **cursor** (`after_seq`) on *every* reconnect (fixing
//!   the TS closure that dropped it), parses each payload into the wire's typed
//!   frame at the boundary (no unchecked cast), and classifies stream errors
//!   permanent-vs-transient;
//! * **`forward`** — a mutation POST with a request **deadline** and jittered
//!   backoff retry of transient failures;
//! * **the level-triggered reconciler** — runs on *every* connect (first
//!   included) and drives both halves of D44: never-dispatched replay and the
//!   sent-but-unsettled settlement query (when the wire has one).
//!
//! No timers, no HTTP client, no persistence live here — those are the injected
//! [`Transport`]/[`Scheduler`]/[`FrameSink`]/[`PendingSetHooks`]. That is what keeps
//! the crate wasm-pure.

use std::cell::RefCell;
use std::rc::Rc;

use futures_util::future::{select, Either, LocalBoxFuture};
use futures_util::StreamExt;

use posthaste_contract_core::{
    MutationReceipt, MutationRequest, RuntimeAdapterError, RuntimeMutationSettlement,
};

use crate::config::NearEndConfig;
use crate::error::{classify_status, Disposition};
use crate::pending_set::PendingSetHooks;
use crate::scheduler::Scheduler;
use crate::sink::{ConnectionStatus, FrameSink};
use crate::transport::{
    GetRequest, PostRequest, PostResponse, StreamEvent, Transport, TransportError,
};
use crate::wire::{ParsedFrame, Wire};

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
    /// The wire's prepare step ran (a no-prepare wire is prepared immediately).
    prepared: bool,
    /// The prepare result the wire's requests carry (e.g. the link id).
    token: Option<String>,
    cursor: Option<u64>,
    reconnect_attempt: u32,
    /// Consecutive malformed-frame count within the live stream ([3]); reset by
    /// any successfully parsed frame.
    malformed_streak: u32,
    shutdown: bool,
}

/// What the frame loop does after handling one stream message.
enum MessageOutcome {
    /// Keep reading the current stream.
    Continue,
    /// A seq gap the near node cannot bridge from this stream: reseed and
    /// **immediately** resubscribe from the resume cursor (no backoff) — the
    /// far-end replays the gap (or sends a `Reset` if it cannot).
    Resubscribe,
    /// The wire is permanently broken (N consecutive malformed frames, [3]) —
    /// stop the loop.
    Fatal,
}

/// The near-end engine, generic over the seam's [`Wire`] profile. Held behind
/// `Rc` so its long-lived frame loop ([`Self::run`]) and its short one-shot
/// calls ([`Self::forward`]) share one state without a lifetime.
pub struct NearEnd<W: Wire> {
    wire: W,
    transport: Rc<dyn Transport>,
    scheduler: Rc<dyn Scheduler>,
    sink: Rc<dyn FrameSink<W::Frame>>,
    pending_set: Rc<dyn PendingSetHooks>,
    config: NearEndConfig,
    state: RefCell<State>,
}

impl<W: Wire> NearEnd<W> {
    pub fn new(
        wire: W,
        transport: Rc<dyn Transport>,
        scheduler: Rc<dyn Scheduler>,
        sink: Rc<dyn FrameSink<W::Frame>>,
        pending_set: Rc<dyn PendingSetHooks>,
        config: NearEndConfig,
    ) -> Rc<Self> {
        let cursor = config.initial_cursor;
        Rc::new(Self {
            wire,
            transport,
            scheduler,
            sink,
            pending_set,
            config,
            state: RefCell::new(State {
                cursor,
                ..State::default()
            }),
        })
    }

    /// The wire's connection token (the link id at the client seam), once
    /// [`Self::open`] has run. `None` for a no-prepare wire.
    pub fn token(&self) -> Option<String> {
        self.state.borrow().token.clone()
    }

    /// The engine-owned resume cursor (last seen frame seq). The host persists
    /// this so a reload resumes where it left off — callers no longer thread
    /// `afterSeq` themselves.
    pub fn cursor(&self) -> Option<u64> {
        self.state.borrow().cursor
    }

    /// Ask the frame loop to stop (no further reconnects). Idempotent. Aborting
    /// an *idle* open stream promptly is the host's concern (it drops the task).
    pub fn request_shutdown(&self) {
        self.state.borrow_mut().shutdown = true;
    }

    fn is_shutdown(&self) -> bool {
        self.state.borrow().shutdown
    }

    fn is_prepared(&self) -> bool {
        self.state.borrow().prepared
    }

    // ---- connection prepare --------------------------------------------------

    /// Run the wire's prepare step (idempotent) — the client seam POSTs
    /// `/runtime/sessions` and stores the link id; a no-prepare wire is
    /// prepared immediately. Returns the connection token, if the wire has one.
    pub async fn open(&self) -> Result<Option<String>, EngineError> {
        if self.is_prepared() {
            return Ok(self.token());
        }
        let Some(request) = self.wire.prepare_request() else {
            self.state.borrow_mut().prepared = true;
            return Ok(None);
        };
        let resp = self
            .post_with_deadline(request)
            .await
            .map_err(|e| EngineError::transient(e.message))?;
        if !(200..300).contains(&resp.status) {
            return Err(EngineError::from_response(resp.status, &resp.body));
        }
        let token = self
            .wire
            .parse_prepared(&resp.body)
            .map_err(EngineError::permanent)?;
        let mut state = self.state.borrow_mut();
        state.token = Some(token.clone());
        state.prepared = true;
        Ok(Some(token))
    }

    // ---- forward -----------------------------------------------------------

    /// Forward a mutation with the request deadline + transient-retry policy.
    /// Returns the receipt on 2xx (including an authority `Failed` verdict), a
    /// permanent error on 4xx, or a transient error once retries are exhausted.
    pub async fn forward(&self, request: MutationRequest) -> Result<MutationReceipt, EngineError> {
        let mut attempt = 0u32;
        loop {
            let token = self.token();
            let post = self.wire.forward_request(token.as_deref(), &request)?;
            match self.post_with_deadline(post).await {
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
        request: PostRequest,
    ) -> Result<PostResponse, TransportError> {
        self.with_deadline(self.transport.post_json(request)).await
    }

    async fn get_with_deadline(&self, request: GetRequest) -> Result<PostResponse, TransportError> {
        self.with_deadline(self.transport.get_json(request)).await
    }

    async fn with_deadline(
        &self,
        request: LocalBoxFuture<'static, Result<PostResponse, TransportError>>,
    ) -> Result<PostResponse, TransportError> {
        let deadline = self.scheduler.sleep(self.config.request_deadline);
        match select(request, deadline).await {
            Either::Left((result, _)) => result,
            // Deadline won: dropping the request future cancels it in flight.
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

            // Run the wire's prepare step (link open) before subscribing.
            if !self.is_prepared() {
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
            let request = {
                let state = self.state.borrow();
                self.wire.stream_request(state.token.as_deref(), state.cursor)
            };
            let mut stream = self.transport.open_stream(request);

            let mut permanent = false;
            // An immediate (no-backoff) resubscribe to bridge a detected seq gap.
            let mut resubscribe = false;
            while let Some(event) = stream.next().await {
                match event {
                    StreamEvent::Open => {
                        self.state.borrow_mut().reconnect_attempt = 0;
                        self.sink.on_status(ConnectionStatus::Connected);
                        // Level-triggered reconciler: every connect, first included.
                        self.clone().reconcile().await;
                    }
                    StreamEvent::Message(data) => match self.handle_message(data) {
                        MessageOutcome::Continue => {}
                        MessageOutcome::Resubscribe => {
                            resubscribe = true;
                            break;
                        }
                        MessageOutcome::Fatal => {
                            permanent = true;
                            break;
                        }
                    },
                    StreamEvent::Closed => break,
                    StreamEvent::Error { status, message } => {
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
            if resubscribe {
                // Gap recovery (D49): resubscribe at once from the resume cursor,
                // without the reconnect backoff — this is a targeted catch-up, not
                // a failure.
                continue;
            }
            self.sink.on_status(ConnectionStatus::Reconnecting);
            self.backoff_before_reconnect().await;
        }
    }

    fn handle_message(&self, data: String) -> MessageOutcome {
        // Skip SSE keep-alive / empty payloads (parity with the TS `!event.data`).
        // An empty payload is not a malformed frame — it does not touch the streak.
        if data.trim().is_empty() {
            return MessageOutcome::Continue;
        }
        match self.wire.parse_frame(&data) {
            Ok(ParsedFrame::Reset { highest_seq }) => {
                // The far-end could not serve our resume point (backlog overflow):
                // adopt its current cursor so we stop gap-detecting against the
                // lost seqs, and re-seed from current state (D49).
                let mut state = self.state.borrow_mut();
                state.malformed_streak = 0;
                state.cursor = Some(highest_seq);
                drop(state);
                self.sink.on_reset();
                MessageOutcome::Continue
            }
            Ok(ParsedFrame::Frame { seq, frame }) => {
                let gap = {
                    let mut state = self.state.borrow_mut();
                    state.malformed_streak = 0;
                    // A gap is a seq strictly beyond the next expected one.
                    let gap = state.cursor.is_some_and(|c| seq > c + 1);
                    if !gap && state.cursor.is_none_or(|c| seq > c) {
                        state.cursor = Some(seq);
                    }
                    gap
                };
                if gap {
                    // Reseed the near node's incremental view, then resubscribe
                    // from the (unchanged) cursor to replay the missing frames
                    // (D49). The frame that revealed the gap is not delivered — it
                    // will arrive again in order after the resubscribe.
                    self.sink.on_reset();
                    MessageOutcome::Resubscribe
                } else {
                    self.sink.on_frame(frame);
                    MessageOutcome::Continue
                }
            }
            Err(e) => {
                let streak = {
                    let mut state = self.state.borrow_mut();
                    state.malformed_streak += 1;
                    state.malformed_streak
                };
                self.sink.on_malformed(data, e);
                if streak >= self.config.max_consecutive_malformed {
                    // [3]: a run of unparseable frames is a version skew / corrupt
                    // peer, not an ignorable keep-alive — surface Degraded and stop.
                    self.sink.on_status(ConnectionStatus::Degraded(format!(
                        "{streak} consecutive malformed frames; the link peer is incompatible or corrupt"
                    )));
                    MessageOutcome::Fatal
                } else {
                    MessageOutcome::Continue
                }
            }
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

    /// The level-triggered reconciler (D44), run on every connect:
    ///
    /// * **(a) never-dispatched replay** — a request the host optimistically
    ///   accepted with no evidence it reached the runtime is re-forwarded. Safe:
    ///   never-dispatched means no server-side application, and a same-link
    ///   duplicate `clientMutationId` is deduped by the runtime.
    /// * **(b) sent-but-unsettled reconciliation** — a record with a receipt but
    ///   no terminal settlement (link-continuity loss) queries the runtime's
    ///   settlement state by stored ids ([`Wire::settlement_request`]): a
    ///   terminal verdict settles locally, a still-pending record is left to the
    ///   frame stream, and a record the runtime does not know is re-forwarded
    ///   (the far-end dedup ledger guards a raced duplicate).
    async fn reconcile(self: Rc<Self>) {
        let pending = self.pending_set.never_dispatched().await;
        for request in pending {
            // A transient/permanent replay failure surfaces through the normal
            // frame/settlement stream; the record stays for the next connect.
            if let Ok(receipt) = self.forward(request).await {
                self.pending_set.on_reconciled(receipt).await;
            }
        }

        let unsettled = self.pending_set.sent_unsettled().await;
        for record in unsettled {
            let Some(get) = self
                .wire
                .settlement_request(&record.link_id, &record.client_mutation_id)
            else {
                // This seam has no settlement query — nothing to reconcile here.
                return;
            };
            let Ok(resp) = self.get_with_deadline(get).await else {
                // Transport failure: leave the record for the next connect.
                continue;
            };
            if !(200..300).contains(&resp.status) {
                continue;
            }
            let Ok(settlement) = serde_json::from_str::<RuntimeMutationSettlement>(&resp.body)
            else {
                continue;
            };
            match settlement.receipt {
                Some(receipt) if receipt.state.is_terminal() => {
                    // The runtime already settled it — settle locally.
                    self.pending_set.on_settlement(receipt).await;
                }
                Some(_) => {
                    // Still pending server-side; the frame stream will settle it.
                }
                None => {
                    // The runtime has no record (link-continuity loss):
                    // re-forward, if the host still holds the original request.
                    let Some(request) = record.request else { continue };
                    if let Ok(receipt) = self.forward(request).await {
                        self.pending_set.on_reconciled(receipt).await;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
