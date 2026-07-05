//! Engine behavior, driven through the injected seams with in-memory fakes.
//!
//! Everything the fakes return is synchronously ready (instant sleeps, scripted
//! responses/streams), so a no-op-waker `block_on` busy-poll drives the async
//! engine without a real executor — keeping the crate dependency-free while
//! still exercising the full reconnect/forward/reconcile paths. The harness
//! instantiates the engine over the client seam's [`RuntimeLinkWire`]; the
//! authority-server wire's profile is exercised natively in `posthaste-runtime`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::Future;
use std::rc::Rc;
use std::time::Duration;

use futures_util::future::{pending, ready, LocalBoxFuture};
use futures_util::stream::{self, LocalBoxStream};
use futures_util::{FutureExt, StreamExt};

use posthaste_contract_core::{MutationReceipt, MutationRequest, RuntimeFrame};

use crate::config::{NearEndConfig, STREAM_LIVENESS_DEADLINE};
use crate::pending_set::{PendingSetHooks, SentUnsettled};
use crate::scheduler::Scheduler;
use crate::sink::{ConnectionStatus, FrameSink};
use crate::transport::{
    GetRequest, PostRequest, PostResponse, StreamEvent, StreamRequest, Transport, TransportError,
};
use crate::wire::RuntimeLinkWire;

use super::NearEnd;

// ---- a no-executor block_on -----------------------------------------------

fn block_on<F: Future>(fut: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    let mut fut = std::pin::pin!(fut);
    loop {
        if let Poll::Ready(value) = fut.as_mut().poll(&mut cx) {
            return value;
        }
    }
}

// ---- fakes -----------------------------------------------------------------

struct FakeTransport {
    link_response: PostResponse,
    /// Scripted link-open responses (in order), falling back to `link_response`
    /// when exhausted — lets a re-prepare test hand out a DIFFERENT link id.
    link_responses: RefCell<VecDeque<PostResponse>>,
    mutation_responses: RefCell<VecDeque<Result<PostResponse, TransportError>>>,
    /// If set, mutation POSTs never resolve (to exercise the deadline).
    hang_mutations: bool,
    /// Scripted responses for settlement GETs, in order.
    settlement_responses: RefCell<VecDeque<Result<PostResponse, TransportError>>>,
    /// Scripted streams: the events to yield, plus whether the stream then goes
    /// **silent** (blocks forever with no further event) — a half-open socket
    /// that exercises the read-liveness watchdog.
    stream_scripts: RefCell<VecDeque<(Vec<StreamEvent>, bool)>>,
    posts: RefCell<Vec<(String, String)>>,
    gets: RefCell<Vec<String>>,
    stream_urls: RefCell<Vec<String>>,
}

impl FakeTransport {
    fn new() -> Self {
        Self {
            link_response: PostResponse {
                status: 200,
                body: r#"{"linkId":"link-test"}"#.to_string(),
            },
            link_responses: RefCell::new(VecDeque::new()),
            mutation_responses: RefCell::new(VecDeque::new()),
            hang_mutations: false,
            settlement_responses: RefCell::new(VecDeque::new()),
            stream_scripts: RefCell::new(VecDeque::new()),
            posts: RefCell::new(Vec::new()),
            gets: RefCell::new(Vec::new()),
            stream_urls: RefCell::new(Vec::new()),
        }
    }

    fn with_mutation(mut self, response: Result<PostResponse, TransportError>) -> Self {
        self.mutation_responses.get_mut().push_back(response);
        self
    }

    /// Script the next link-open POST to return this body (a fresh link id).
    fn with_link(mut self, body: &str) -> Self {
        self.link_responses.get_mut().push_back(PostResponse {
            status: 200,
            body: body.to_string(),
        });
        self
    }

    fn with_settlement(mut self, response: Result<PostResponse, TransportError>) -> Self {
        self.settlement_responses.get_mut().push_back(response);
        self
    }

    fn with_stream(mut self, events: Vec<StreamEvent>) -> Self {
        self.stream_scripts.get_mut().push_back((events, false));
        self
    }

    /// Script a stream that yields `events`, then goes SILENT forever — no more
    /// frames, no keep-alive, no `Closed`/`Error`. Models the half-open socket
    /// the read-liveness watchdog exists to catch.
    fn with_silent_stream(mut self, events: Vec<StreamEvent>) -> Self {
        self.stream_scripts.get_mut().push_back((events, true));
        self
    }
}

fn ok_response(status: u16, body: &str) -> Result<PostResponse, TransportError> {
    Ok(PostResponse {
        status,
        body: body.to_string(),
    })
}

impl Transport for FakeTransport {
    fn post_json(
        &self,
        request: PostRequest,
    ) -> LocalBoxFuture<'static, Result<PostResponse, TransportError>> {
        self.posts
            .borrow_mut()
            .push((request.url.clone(), request.body.clone()));
        if request.url.contains("/mutations") {
            if self.hang_mutations {
                return pending().boxed_local();
            }
            let next = self
                .mutation_responses
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| ok_response(200, EMPTY_RECEIPT));
            ready(next).boxed_local()
        } else {
            let next = self
                .link_responses
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| self.link_response.clone());
            ready(Ok(next)).boxed_local()
        }
    }

    fn get_json(
        &self,
        request: GetRequest,
    ) -> LocalBoxFuture<'static, Result<PostResponse, TransportError>> {
        self.gets.borrow_mut().push(request.url);
        let next = self
            .settlement_responses
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| ok_response(200, r#"{"receipt":null}"#));
        ready(next).boxed_local()
    }

    fn open_stream(&self, request: StreamRequest) -> LocalBoxStream<'static, StreamEvent> {
        self.stream_urls.borrow_mut().push(request.url);
        let (events, silent) = self
            .stream_scripts
            .borrow_mut()
            .pop_front()
            // Exhausted script → a permanent error stops the reconnect loop so
            // the test's `run()` returns instead of spinning forever.
            .unwrap_or_else(|| {
                (
                    vec![StreamEvent::Error {
                        status: Some(403),
                        message: "no more script".to_string(),
                    }],
                    false,
                )
            });
        let base = stream::iter(events);
        if silent {
            // Yield the events, then never again (and never end): the read loop
            // must fall through to its liveness deadline, not block forever.
            base.chain(stream::pending()).boxed_local()
        } else {
            base.boxed_local()
        }
    }
}

const EMPTY_RECEIPT: &str = r#"{"runtimeMutationId":null,"clientMutationId":"x","name":"x","state":"accepted","error":null,"output":null}"#;

struct FakeScheduler {
    sleeps: RefCell<Vec<Duration>>,
    /// Virtual-time control for the read-liveness deadline: a sleep of exactly
    /// [`STREAM_LIVENESS_DEADLINE`] resolves (the window elapsed) only when this
    /// is set. A test flips it to choose between a silently-dead stream
    /// (expired = the deadline fires) and a live-but-idle one (never fires, so
    /// arriving keep-alives always win the race). All other sleeps (backoff,
    /// request deadline) resolve instantly as before.
    liveness_deadline_expired: Cell<bool>,
}

impl FakeScheduler {
    fn new() -> Self {
        Self {
            sleeps: RefCell::new(Vec::new()),
            liveness_deadline_expired: Cell::new(false),
        }
    }

    /// Advance virtual time past the read-liveness deadline (or not), so the
    /// next armed deadline fires (or stays pending).
    fn set_liveness_expired(&self, expired: bool) {
        self.liveness_deadline_expired.set(expired);
    }
}

impl Scheduler for FakeScheduler {
    fn sleep(&self, duration: Duration) -> LocalBoxFuture<'static, ()> {
        self.sleeps.borrow_mut().push(duration);
        if duration == STREAM_LIVENESS_DEADLINE && !self.liveness_deadline_expired.get() {
            // The liveness window has NOT elapsed: this deadline must lose every
            // race to an arriving frame/keep-alive, so it never resolves.
            return pending().boxed_local();
        }
        ready(()).boxed_local()
    }
    fn jitter(&self) -> f64 {
        0.5
    }
}

#[derive(Default)]
struct RecordingSink {
    frames: RefCell<Vec<RuntimeFrame>>,
    malformed: RefCell<Vec<(String, String)>>,
    statuses: RefCell<Vec<String>>,
    resets: RefCell<u32>,
    /// New link ids surfaced via `on_link_reestablished` (the M44 recovery edge).
    reestablished: RefCell<Vec<String>>,
}

impl FrameSink<RuntimeFrame> for RecordingSink {
    fn on_frame(&self, frame: RuntimeFrame) {
        self.frames.borrow_mut().push(frame);
    }
    fn on_malformed(&self, raw: String, error: String) {
        self.malformed.borrow_mut().push((raw, error));
    }
    fn on_reset(&self) {
        *self.resets.borrow_mut() += 1;
    }
    fn on_status(&self, status: ConnectionStatus) {
        let label = match status {
            ConnectionStatus::Connecting => "connecting",
            ConnectionStatus::Connected => "connected",
            ConnectionStatus::Reconnecting => "reconnecting",
            ConnectionStatus::TransientError(_) => "transient",
            ConnectionStatus::Degraded(_) => "degraded",
            ConnectionStatus::PermanentError(_) => "permanent",
        };
        self.statuses.borrow_mut().push(label.to_string());
    }
    fn on_link_reestablished(&self, link_id: String) {
        self.reestablished.borrow_mut().push(link_id);
    }
}

#[derive(Default)]
struct FakePendingSet {
    never: RefCell<Vec<MutationRequest>>,
    unsettled: RefCell<Vec<SentUnsettled>>,
    reconciled: RefCell<Vec<MutationReceipt>>,
    settled: RefCell<Vec<MutationReceipt>>,
}

impl PendingSetHooks for FakePendingSet {
    fn never_dispatched(&self) -> LocalBoxFuture<'static, Vec<MutationRequest>> {
        // Drain: a replayed record is no longer never-dispatched.
        let taken = std::mem::take(&mut *self.never.borrow_mut());
        ready(taken).boxed_local()
    }
    fn on_reconciled(&self, receipt: MutationReceipt) -> LocalBoxFuture<'static, ()> {
        self.reconciled.borrow_mut().push(receipt);
        ready(()).boxed_local()
    }
    fn sent_unsettled(&self) -> LocalBoxFuture<'static, Vec<SentUnsettled>> {
        let taken = std::mem::take(&mut *self.unsettled.borrow_mut());
        ready(taken).boxed_local()
    }
    fn on_settlement(&self, receipt: MutationReceipt) -> LocalBoxFuture<'static, ()> {
        self.settled.borrow_mut().push(receipt);
        ready(()).boxed_local()
    }
}

fn sample_request(client_mutation_id: &str) -> MutationRequest {
    let json = format!(
        r#"{{"name":"message.setReadState","args":{{"sourceId":"acct-1","messageId":"m1","read":true}},"clientMutationId":"{client_mutation_id}"}}"#
    );
    serde_json::from_str(&json).expect("valid mutation request")
}

fn confirmed_receipt(client_mutation_id: &str) -> String {
    format!(
        r#"{{"runtimeMutationId":"rm-1","clientMutationId":"{client_mutation_id}","name":"message.setReadState","state":"confirmed","error":null,"output":null}}"#
    )
}

struct Harness {
    engine: Rc<NearEnd<RuntimeLinkWire>>,
    transport: Rc<FakeTransport>,
    scheduler: Rc<FakeScheduler>,
    sink: Rc<RecordingSink>,
    pending_set: Rc<FakePendingSet>,
}

fn harness(transport: FakeTransport, pending_set: FakePendingSet, config: NearEndConfig) -> Harness {
    let transport = Rc::new(transport);
    let scheduler = Rc::new(FakeScheduler::new());
    let sink = Rc::new(RecordingSink::default());
    let pending_set = Rc::new(pending_set);
    let engine = NearEnd::new(
        RuntimeLinkWire {
            view_delta: true,
            ..RuntimeLinkWire::default()
        },
        transport.clone(),
        scheduler.clone(),
        sink.clone(),
        pending_set.clone(),
        config,
    );
    Harness {
        engine,
        transport,
        scheduler,
        sink,
        pending_set,
    }
}

// ---- tests -----------------------------------------------------------------

#[test]
fn forward_returns_receipt_and_stamps_session() {
    let transport =
        FakeTransport::new().with_mutation(ok_response(200, &confirmed_receipt("op-1")));
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());

    let receipt = block_on(async {
        h.engine.open().await.unwrap();
        h.engine.forward(sample_request("op-1")).await
    })
    .expect("forward ok");

    assert_eq!(receipt.client_mutation_id.as_str(), "op-1");
    assert_eq!(receipt.name, "message.setReadState");
    // The forward body stamped the opened link id and round-tripped the op.
    let posts = h.transport.posts.borrow();
    let (url, body) = posts.iter().find(|(u, _)| u.contains("/mutations")).unwrap();
    assert!(url.contains("/runtime/sessions/link-test/mutations"), "{url}");
    assert!(body.contains("\"linkId\":\"link-test\""), "{body}");
    assert!(body.contains("\"name\":\"message.setReadState\""), "{body}");
}

#[test]
fn forward_retries_transient_then_succeeds() {
    let transport = FakeTransport::new()
        .with_mutation(ok_response(503, "unavailable"))
        .with_mutation(ok_response(200, &confirmed_receipt("op-2")));
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());

    let receipt = block_on(async {
        h.engine.open().await.unwrap();
        h.engine.forward(sample_request("op-2")).await
    })
    .expect("forward ok after retry");

    assert_eq!(receipt.state, posthaste_contract_core::MutationSettlementState::Confirmed);
    // Two mutation POSTs (503 then 200) and one backoff sleep between them.
    let mutation_posts = h
        .transport
        .posts
        .borrow()
        .iter()
        .filter(|(u, _)| u.contains("/mutations"))
        .count();
    assert_eq!(mutation_posts, 2);
    assert!(h.scheduler.sleeps.borrow().iter().any(|d| *d > Duration::ZERO));
}

#[test]
fn forward_4xx_is_permanent_with_envelope() {
    let body = r#"{"code":"invalid_mutation","message":"nope","terminality":"permanent","correlationId":null,"details":null}"#;
    let transport = FakeTransport::new().with_mutation(ok_response(422, body));
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());

    let err = block_on(async {
        h.engine.open().await.unwrap();
        h.engine.forward(sample_request("op-3")).await
    })
    .expect_err("permanent");

    assert!(err.is_permanent());
    assert_eq!(err.status, Some(422));
    assert_eq!(err.message, "nope");
    assert!(err.error.is_some());
    // No retry on a permanent verdict: exactly one mutation POST.
    let mutation_posts = h
        .transport
        .posts
        .borrow()
        .iter()
        .filter(|(u, _)| u.contains("/mutations"))
        .count();
    assert_eq!(mutation_posts, 1);
}

#[test]
fn forward_respects_envelope_terminality_over_status_band() {
    // M29/D70: when the response envelope carries a typed terminality, it is
    // authoritative — a 4xx that the far end marked transient must be retried,
    // not fatally stopped by the status band. Here a 422 stamped
    // `terminality: transient` is retried and then succeeds.
    let body = r#"{"code":"invalid_mutation","message":"transient-4xx","terminality":"transient","correlationId":null,"details":null}"#;
    let transport = FakeTransport::new()
        .with_mutation(ok_response(422, body))
        .with_mutation(ok_response(200, &confirmed_receipt("op-tt")));
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());

    let receipt = block_on(async {
        h.engine.open().await.unwrap();
        h.engine.forward(sample_request("op-tt")).await
    })
    .expect("transient 4xx is retried to success");

    assert_eq!(
        receipt.state,
        posthaste_contract_core::MutationSettlementState::Confirmed
    );
    let mutation_posts = h
        .transport
        .posts
        .borrow()
        .iter()
        .filter(|(u, _)| u.contains("/mutations"))
        .count();
    assert_eq!(mutation_posts, 2, "the transient-stamped 4xx was retried");
}

#[test]
fn forward_honors_request_deadline() {
    let mut transport = FakeTransport::new();
    transport.hang_mutations = true;
    let config = NearEndConfig {
        forward_max_attempts: 1,
        ..NearEndConfig::default()
    };
    let h = harness(transport, FakePendingSet::default(), config);

    let err = block_on(async {
        h.engine.open().await.unwrap();
        h.engine.forward(sample_request("op-4")).await
    })
    .expect_err("deadline");

    // A hung POST that never resolves is abandoned at the deadline, not awaited
    // forever (lifecycle-debt row 1). With max_attempts=1 it surfaces at once.
    assert!(!err.is_permanent());
}

#[test]
fn stream_reconnects_and_carries_the_resume_cursor() {
    // First connect delivers a frame at seq 5 then closes; second connect stops
    // the loop. The reconnect must resubscribe with afterSeq=5.
    let transport = FakeTransport::new()
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Message(r#"{"type":"heartbeat","linkSeq":5}"#.to_string()),
            StreamEvent::Closed,
        ])
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(403),
                message: "stop".to_string(),
            },
        ]);
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());

    block_on(h.engine.clone().run());

    // The engine parsed the heartbeat and advanced its cursor.
    assert_eq!(h.engine.cursor(), Some(5));
    assert_eq!(h.sink.frames.borrow().len(), 1);
    // Two subscribe URLs; the second resumes from the cursor.
    let urls = h.transport.stream_urls.borrow();
    assert_eq!(urls.len(), 2, "expected a reconnect");
    assert!(!urls[0].contains("afterSeq"), "first has no cursor: {}", urls[0]);
    assert!(urls[1].contains("afterSeq=5"), "reconnect resumes: {}", urls[1]);
    assert!(h.sink.statuses.borrow().iter().any(|s| s == "reconnecting"));
}

// Count POSTs to the link-open route (`/runtime/sessions`, NOT the per-link
// `/mutations` sub-route) — one per prepare handshake, so a re-prepare shows as 2.
fn prepare_post_count(h: &Harness) -> usize {
    h.transport
        .posts
        .borrow()
        .iter()
        .filter(|(u, _)| u.contains("/runtime/sessions") && !u.contains("/mutations"))
        .count()
}

// D110a / M40 — the CONFIRMED F1 hotfix. A stream-open failure indicating a
// stale/absent link (404 — the runtime's `not_found` for a link idle-reaped at
// SESSION_IDLE_TTL, i.e. every laptop sleep >5min, or dropped by a daemon
// restart) must NOT classify Permanent. The engine clears the dead link's
// prepared state + resume cursor and re-runs the prepare handshake against a
// FRESH link, so `run()` keeps going instead of halting live updates until reload.
#[test]
fn stale_link_404_stream_error_re_prepares_a_fresh_link() {
    let transport = FakeTransport::new()
        .with_stream(vec![
            StreamEvent::Open,
            // A frame advances the resume cursor to 7 before the link dies...
            StreamEvent::Message(r#"{"type":"heartbeat","linkSeq":7}"#.to_string()),
            // ...then the subscribe GET is rejected 404 (the reaped/dead link).
            StreamEvent::Error {
                status: Some(404),
                message: "runtime stream rejected with 404".to_string(),
            },
        ])
        // The re-prepared fresh link opens, then a 403 stops the loop so the test
        // terminates deterministically.
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(403),
                message: "stop".to_string(),
            },
        ]);
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());

    block_on(h.engine.clone().run());

    // The prepare handshake ran TWICE — the 404 forced a fresh link open, not a
    // permanent halt.
    assert_eq!(prepare_post_count(&h), 2, "the stale link was re-prepared");
    // Two subscribes; the resume cursor was CLEARED with the dead link, so the
    // fresh subscribe carries no `afterSeq` (a seq 7 from the defunct link's seq
    // space must never leak into the new link).
    let urls = h.transport.stream_urls.borrow();
    assert_eq!(urls.len(), 2, "re-subscribed on the fresh link");
    assert!(
        !urls[1].contains("afterSeq"),
        "the fresh link subscribe drops the dead link's cursor: {}",
        urls[1]
    );
    // The 404 was surfaced as transient + drove the reconnect/backoff tail, never
    // a permanent stop.
    let statuses = h.sink.statuses.borrow();
    assert!(statuses.iter().any(|s| s == "transient"), "{statuses:?}");
    assert!(statuses.iter().any(|s| s == "reconnecting"), "{statuses:?}");
    // A jittered backoff sleep ran between the re-prepare attempts (no tight storm).
    assert!(h.scheduler.sleeps.borrow().iter().any(|d| *d > Duration::ZERO));
}

// D110a — 410 Gone is treated identically to 404 (forward-compatibility for a
// server that ever distinguishes a reaped link): re-prepare, not Permanent.
#[test]
fn stale_link_410_stream_error_re_prepares_a_fresh_link() {
    let transport = FakeTransport::new()
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(410),
                message: "gone".to_string(),
            },
        ])
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(403),
                message: "stop".to_string(),
            },
        ]);
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());

    block_on(h.engine.clone().run());

    assert_eq!(prepare_post_count(&h), 2, "410 also re-prepares");
    assert_eq!(h.transport.stream_urls.borrow().len(), 2);
}

// M44/D112 — the recovery-edge signal. A stale-link 404 re-prepare opens a
// FRESH link; the engine must surface `on_link_reestablished` exactly once,
// carrying the NEW link id, so the host adopts it + reconciles server-served
// views/caches (the fix for "open views stop updating" + "empty mailbox stuck
// on Syncing"). It must NOT fire on the first connect.
#[test]
fn re_prepare_surfaces_the_recovery_edge_with_the_new_link_id() {
    let transport = FakeTransport::new()
        // First link is "link-A"; then it's reaped (404).
        .with_link(r#"{"linkId":"link-A"}"#)
        // The re-prepared FRESH link is a DIFFERENT id.
        .with_link(r#"{"linkId":"link-B"}"#)
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(404),
                message: "runtime stream rejected with 404".to_string(),
            },
        ])
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(403),
                message: "stop".to_string(),
            },
        ]);
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());

    block_on(h.engine.clone().run());

    // Exactly one recovery edge, carrying the fresh link id — never the old one,
    // never the first connect.
    let reestablished = h.sink.reestablished.borrow();
    assert_eq!(reestablished.len(), 1, "one recovery edge: {reestablished:?}");
    assert_eq!(reestablished[0], "link-B", "carries the FRESH link id");
}

// M44 — the discrimination the reconcile depends on: a SAME-link reconnect (a
// 5xx / statusless blip that keeps the link valid) resumes the stream without
// re-preparing, so NO recovery edge fires — the host must not needlessly
// re-serve every view on an ordinary reconnect.
#[test]
fn same_link_reconnect_does_not_surface_the_recovery_edge() {
    let transport = FakeTransport::new()
        .with_stream(vec![
            StreamEvent::Open,
            // A statusless mid-stream drop: reconnect the SAME link.
            StreamEvent::Error {
                status: None,
                message: "network dropped".to_string(),
            },
        ])
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(403),
                message: "stop".to_string(),
            },
        ]);
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());

    block_on(h.engine.clone().run());

    // One prepare (never re-prepared), two subscribes (reconnected), and NO edge.
    assert_eq!(prepare_post_count(&h), 1, "same link, no re-prepare");
    assert_eq!(h.transport.stream_urls.borrow().len(), 2, "reconnected");
    assert!(
        h.sink.reestablished.borrow().is_empty(),
        "a same-link reconnect must not fire the recovery edge: {:?}",
        h.sink.reestablished.borrow()
    );
}

// W1 — the read-liveness watchdog. A stream that opens then goes SILENT (no
// frame, no keep-alive, no Closed/Error — a half-open socket) must NOT block
// forever: past the liveness deadline the engine re-prepares a FRESH link and
// fires the M44 recovery edge, PURELY from the deadline — no observed error was
// ever delivered. This is the silent-death case M40/M44 alone cannot see.
#[test]
fn silent_dead_stream_re_prepares_from_the_liveness_deadline() {
    let transport = FakeTransport::new()
        // First link is "link-A"; the fresh re-prepared link is "link-B".
        .with_link(r#"{"linkId":"link-A"}"#)
        .with_link(r#"{"linkId":"link-B"}"#)
        // The link opens, reconciles, then the stream goes silent forever.
        .with_silent_stream(vec![StreamEvent::Open])
        // The re-prepared fresh link opens, then a 403 stops the loop so the
        // test terminates deterministically.
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(403),
                message: "stop".to_string(),
            },
        ]);
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());
    // Advance virtual time past the liveness deadline: the armed deadline fires
    // while the stream is silent.
    h.scheduler.set_liveness_expired(true);

    block_on(h.engine.clone().run());

    // The prepare handshake ran TWICE — silence alone forced a fresh link, not a
    // permanent halt and not a blocked-forever read.
    assert_eq!(
        prepare_post_count(&h),
        2,
        "the silently-dead stream was re-prepared"
    );
    assert_eq!(
        h.transport.stream_urls.borrow().len(),
        2,
        "re-subscribed on the fresh link"
    );
    // The M44 recovery edge fired exactly once, carrying the FRESH link id —
    // the SAME outcome as an observed transient death (views/counts reconcile).
    let reestablished = h.sink.reestablished.borrow();
    assert_eq!(reestablished.len(), 1, "one recovery edge: {reestablished:?}");
    assert_eq!(reestablished[0], "link-B", "carries the FRESH link id");
    // Surfaced transient + drove the reconnect/backoff tail off the SILENT
    // stream — never from an observed error (none was delivered on link-A). The
    // transient recovery precedes the reconnect; the only "permanent" is the
    // deliberate 403 that stops the fresh link so the test terminates.
    let statuses = h.sink.statuses.borrow();
    let transient_at = statuses.iter().position(|s| s == "transient");
    let reconnecting_at = statuses.iter().position(|s| s == "reconnecting");
    assert!(transient_at.is_some(), "{statuses:?}");
    assert!(reconnecting_at.is_some(), "{statuses:?}");
    assert!(transient_at < reconnecting_at, "the silence recovered transiently before reconnecting: {statuses:?}");
    // No frame, no malformed report: the re-prepare came from the deadline only.
    assert_eq!(h.sink.frames.borrow().len(), 0);
    assert_eq!(h.sink.malformed.borrow().len(), 0);
}

// W1 — the no-false-trigger proof. While keep-alives (empty messages, the shim's
// surfacing of axum's `:\n\n` every 15s) and real frames keep arriving, the
// liveness deadline NEVER elapses — every arriving event wins the biased race
// and re-arms it. So an idle-but-alive stream must NOT re-prepare, must NOT fire
// a recovery edge, and stays on the SAME link across the whole span.
#[test]
fn keep_alives_and_frames_within_the_deadline_never_trip_the_watchdog() {
    let transport = FakeTransport::new()
        .with_stream(vec![
            StreamEvent::Open,
            // Empty messages ARE the keep-alives (proof of life), interleaved
            // with one real frame.
            StreamEvent::Message(String::new()),
            StreamEvent::Message(String::new()),
            StreamEvent::Message(r#"{"type":"heartbeat","linkSeq":1}"#.to_string()),
            StreamEvent::Message(String::new()),
            // A clean close ends the (still-live) span → a same-link reconnect.
            StreamEvent::Closed,
        ])
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(403),
                message: "stop".to_string(),
            },
        ]);
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());
    // The liveness window never elapses while events flow — the deadline stays
    // pending and must lose every race.
    h.scheduler.set_liveness_expired(false);

    block_on(h.engine.clone().run());

    // Never re-prepared: one prepare handshake; the reconnect after the clean
    // close reused the SAME link (no recovery edge).
    assert_eq!(
        prepare_post_count(&h),
        1,
        "keep-alives/frames must not force a re-prepare"
    );
    assert!(
        h.sink.reestablished.borrow().is_empty(),
        "an idle-but-alive stream must not fire the watchdog recovery edge: {:?}",
        h.sink.reestablished.borrow()
    );
    // The single real frame was delivered; empty keep-alives were skipped, and
    // an empty keep-alive is NOT a malformed frame.
    assert_eq!(h.sink.frames.borrow().len(), 1);
    assert_eq!(h.sink.malformed.borrow().len(), 0);
    assert_eq!(h.engine.cursor(), Some(1));
    // Two subscribes: the reconnect after the clean close, on the same link.
    assert_eq!(h.transport.stream_urls.borrow().len(), 2, "reconnected");
}

// D110a — the reserved case: a genuine auth refusal (401/403) still classifies
// Permanent and stops the loop. Re-prepare is ONLY for a stale/absent link.
#[test]
fn auth_refused_stream_error_stays_permanent() {
    for status in [401u16, 403u16] {
        let transport = FakeTransport::new().with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(status),
                message: "auth refused".to_string(),
            },
        ]);
        let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());

        block_on(h.engine.clone().run());

        // Exactly one prepare + one subscribe: no re-prepare, no reconnect.
        assert_eq!(prepare_post_count(&h), 1, "{status} must not re-prepare");
        assert_eq!(h.transport.stream_urls.borrow().len(), 1, "{status}");
        let statuses = h.sink.statuses.borrow();
        assert!(statuses.iter().any(|s| s == "permanent"), "{status}: {statuses:?}");
        assert!(
            !statuses.iter().any(|s| s == "reconnecting"),
            "{status}: an auth refusal must not reconnect: {statuses:?}"
        );
    }
}

#[test]
fn permanent_stream_error_stops_the_loop() {
    let transport = FakeTransport::new().with_stream(vec![
        StreamEvent::Open,
        StreamEvent::Error {
            status: Some(403),
            message: "forbidden".to_string(),
        },
    ]);
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());

    block_on(h.engine.clone().run());

    // Only one subscribe: a 4xx is permanent, no reconnect attempted.
    assert_eq!(h.transport.stream_urls.borrow().len(), 1);
    assert!(h.sink.statuses.borrow().iter().any(|s| s == "permanent"));
    assert!(!h.sink.statuses.borrow().iter().any(|s| s == "reconnecting"));
}

#[test]
fn malformed_frame_is_reported_not_cast() {
    let transport = FakeTransport::new().with_stream(vec![
        StreamEvent::Open,
        StreamEvent::Message("this is not json".to_string()),
        StreamEvent::Error {
            status: Some(403),
            message: "stop".to_string(),
        },
    ]);
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());

    block_on(h.engine.clone().run());

    assert_eq!(h.sink.frames.borrow().len(), 0);
    assert_eq!(h.sink.malformed.borrow().len(), 1);
    assert_eq!(h.engine.cursor(), None);
}

// D49 gap recovery: a frame whose seq jumps past the next expected one is a
// gap — the engine re-seeds (on_reset), does NOT deliver the gap frame, and
// immediately resubscribes from the resume cursor (no backoff) to replay it.
#[test]
fn a_seq_gap_reseeds_and_resubscribes_from_the_cursor() {
    let transport = FakeTransport::new()
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Message(r#"{"type":"heartbeat","linkSeq":1}"#.to_string()),
            // seq 3 skips 2 → a gap.
            StreamEvent::Message(r#"{"type":"heartbeat","linkSeq":3}"#.to_string()),
        ])
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(403),
                message: "stop".to_string(),
            },
        ]);
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());

    block_on(h.engine.clone().run());

    // Only the in-order seq-1 frame was delivered; the gap frame was not.
    assert_eq!(h.sink.frames.borrow().len(), 1);
    assert_eq!(*h.sink.resets.borrow(), 1, "the gap triggered a reseed");
    // Cursor stayed at 1 (the gap frame did not advance it).
    assert_eq!(h.engine.cursor(), Some(1));
    // Immediate resubscribe from the cursor, and NO reconnect-backoff sleep.
    let urls = h.transport.stream_urls.borrow();
    assert_eq!(urls.len(), 2, "gap forces a resubscribe");
    assert!(urls[1].contains("afterSeq=1"), "resumes from the cursor: {}", urls[1]);
    assert!(
        !h.sink.statuses.borrow().iter().any(|s| s == "reconnecting"),
        "a gap resubscribe is not a reconnect"
    );
}

// [3]: N (=3) consecutive malformed frames is a version skew / corrupt peer —
// the engine surfaces Degraded and stops, rather than swallowing them as
// keep-alives forever.
#[test]
fn consecutive_malformed_frames_degrade_and_stop() {
    let transport = FakeTransport::new().with_stream(vec![
        StreamEvent::Open,
        StreamEvent::Message("garbage-1".to_string()),
        StreamEvent::Message("garbage-2".to_string()),
        StreamEvent::Message("garbage-3".to_string()),
    ]);
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());

    block_on(h.engine.clone().run());

    assert_eq!(h.sink.malformed.borrow().len(), 3);
    assert!(h.sink.statuses.borrow().iter().any(|s| s == "degraded"));
    // Permanent-class: the loop stopped, no reconnect.
    assert_eq!(h.transport.stream_urls.borrow().len(), 1);
    assert!(!h.sink.statuses.borrow().iter().any(|s| s == "reconnecting"));
}

// A good frame between malformed ones resets the streak, so isolated glitches
// never trip the degraded threshold.
#[test]
fn a_good_frame_resets_the_malformed_streak() {
    let transport = FakeTransport::new()
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Message("garbage-1".to_string()),
            StreamEvent::Message("garbage-2".to_string()),
            StreamEvent::Message(r#"{"type":"heartbeat","linkSeq":1}"#.to_string()),
            StreamEvent::Message("garbage-3".to_string()),
            StreamEvent::Closed,
        ])
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(403),
                message: "stop".to_string(),
            },
        ]);
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());

    block_on(h.engine.clone().run());

    assert_eq!(h.sink.malformed.borrow().len(), 3);
    // Never hit 3 *consecutive* → never degraded; the clean close reconnected.
    assert!(!h.sink.statuses.borrow().iter().any(|s| s == "degraded"));
    assert!(h.sink.statuses.borrow().iter().any(|s| s == "reconnecting"));
}

#[test]
fn reconciler_replays_never_dispatched_on_connect() {
    let mut pending_set = FakePendingSet::default();
    pending_set.never.get_mut().push(sample_request("replay-1"));
    let transport = FakeTransport::new()
        .with_mutation(ok_response(200, &confirmed_receipt("replay-1")))
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(403),
                message: "stop".to_string(),
            },
        ]);
    let h = harness(transport, pending_set, NearEndConfig::default());

    block_on(h.engine.clone().run());

    // The reconciler re-forwarded the never-dispatched record on connect and
    // linked its receipt.
    let mutation_posts = h
        .transport
        .posts
        .borrow()
        .iter()
        .filter(|(u, _)| u.contains("/mutations"))
        .count();
    assert_eq!(mutation_posts, 1);
    let reconciled = h.pending_set.reconciled.borrow();
    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0].client_mutation_id.as_str(), "replay-1");
}

// D44b: a sent-but-unsettled record whose settlement query returns a terminal
// receipt is settled locally — no re-forward.
#[test]
fn reconciler_settles_sent_but_unsettled_from_a_terminal_query() {
    let mut pending_set = FakePendingSet::default();
    pending_set.unsettled.get_mut().push(SentUnsettled {
        link_id: "link-old".to_string(),
        client_mutation_id: "sent-1".to_string(),
        request: Some(sample_request("sent-1")),
    });
    let transport = FakeTransport::new()
        .with_settlement(ok_response(
            200,
            &format!(r#"{{"receipt":{}}}"#, confirmed_receipt("sent-1")),
        ))
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(403),
                message: "stop".to_string(),
            },
        ]);
    let h = harness(transport, pending_set, NearEndConfig::default());

    block_on(h.engine.clone().run());

    // The query hit the OLD link's settlement route.
    let gets = h.transport.gets.borrow();
    assert_eq!(gets.len(), 1);
    assert!(
        gets[0].contains("/runtime/sessions/link-old/mutations/sent-1"),
        "{}",
        gets[0]
    );
    // Settled locally; never re-forwarded.
    assert_eq!(h.pending_set.settled.borrow().len(), 1);
    assert_eq!(h.pending_set.settled.borrow()[0].client_mutation_id.as_str(), "sent-1");
    let mutation_posts = h
        .transport
        .posts
        .borrow()
        .iter()
        .filter(|(u, _)| u.contains("/mutations") && !u.contains("/mutations/"))
        .count();
    assert_eq!(mutation_posts, 0, "a terminal verdict must not re-forward");
}

// D44b: when the runtime has NO record of a sent-but-unsettled mutation, the
// reconciler re-forwards the stored request and links the fresh receipt.
#[test]
fn reconciler_reforwards_when_the_runtime_has_no_record() {
    let mut pending_set = FakePendingSet::default();
    pending_set.unsettled.get_mut().push(SentUnsettled {
        link_id: "link-old".to_string(),
        client_mutation_id: "sent-2".to_string(),
        request: Some(sample_request("sent-2")),
    });
    let transport = FakeTransport::new()
        .with_settlement(ok_response(200, r#"{"receipt":null}"#))
        .with_mutation(ok_response(200, &confirmed_receipt("sent-2")))
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(403),
                message: "stop".to_string(),
            },
        ]);
    let h = harness(transport, pending_set, NearEndConfig::default());

    block_on(h.engine.clone().run());

    assert_eq!(h.pending_set.settled.borrow().len(), 0);
    let reconciled = h.pending_set.reconciled.borrow();
    assert_eq!(reconciled.len(), 1, "no-record must re-forward and link");
    assert_eq!(reconciled[0].client_mutation_id.as_str(), "sent-2");
}

// D44b: a still-pending server-side record is left alone — the frame stream
// (link collapse re-delivers terminal notifications) settles it.
#[test]
fn reconciler_leaves_a_still_pending_record_alone() {
    let mut pending_set = FakePendingSet::default();
    pending_set.unsettled.get_mut().push(SentUnsettled {
        link_id: "link-old".to_string(),
        client_mutation_id: "sent-3".to_string(),
        request: Some(sample_request("sent-3")),
    });
    let pending_receipt = r#"{"runtimeMutationId":"rm-1","clientMutationId":"sent-3","name":"message.setReadState","state":"accepted","error":null,"output":null}"#;
    let transport = FakeTransport::new()
        .with_settlement(ok_response(
            200,
            &format!(r#"{{"receipt":{pending_receipt}}}"#),
        ))
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(403),
                message: "stop".to_string(),
            },
        ]);
    let h = harness(transport, pending_set, NearEndConfig::default());

    block_on(h.engine.clone().run());

    assert_eq!(h.pending_set.settled.borrow().len(), 0);
    assert_eq!(h.pending_set.reconciled.borrow().len(), 0);
}

#[test]
fn shutdown_prevents_reconnect() {
    let transport = FakeTransport::new().with_stream(vec![StreamEvent::Open, StreamEvent::Closed]);
    let h = harness(transport, FakePendingSet::default(), NearEndConfig::default());
    // Request shutdown before running; the loop opens once then exits at the
    // clean close without a reconnect.
    block_on(async {
        h.engine.open().await.unwrap();
        h.engine.request_shutdown();
        h.engine.clone().run().await;
    });
    assert!(h.transport.stream_urls.borrow().len() <= 1);
}
