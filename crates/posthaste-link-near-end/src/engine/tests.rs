//! Engine behavior, driven through the injected seams with in-memory fakes.
//!
//! Everything the fakes return is synchronously ready (instant sleeps, scripted
//! responses/streams), so a no-op-waker `block_on` busy-poll drives the async
//! engine without a real executor — keeping the crate dependency-free while
//! still exercising the full reconnect/forward/reconcile paths. The harness
//! instantiates the engine over the client seam's [`RuntimeLinkWire`]; the
//! authority-server wire's profile is exercised natively in `posthaste-runtime`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::rc::Rc;
use std::time::Duration;

use futures_util::future::{pending, ready, LocalBoxFuture};
use futures_util::stream::{self, LocalBoxStream};
use futures_util::{FutureExt, StreamExt};

use posthaste_contract_core::{MutationReceipt, MutationRequest, RuntimeFrame};

use crate::config::NearEndConfig;
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
    mutation_responses: RefCell<VecDeque<Result<PostResponse, TransportError>>>,
    /// If set, mutation POSTs never resolve (to exercise the deadline).
    hang_mutations: bool,
    /// Scripted responses for settlement GETs, in order.
    settlement_responses: RefCell<VecDeque<Result<PostResponse, TransportError>>>,
    stream_scripts: RefCell<VecDeque<Vec<StreamEvent>>>,
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

    fn with_settlement(mut self, response: Result<PostResponse, TransportError>) -> Self {
        self.settlement_responses.get_mut().push_back(response);
        self
    }

    fn with_stream(mut self, events: Vec<StreamEvent>) -> Self {
        self.stream_scripts.get_mut().push_back(events);
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
            ready(Ok(self.link_response.clone())).boxed_local()
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
        let events = self
            .stream_scripts
            .borrow_mut()
            .pop_front()
            // Exhausted script → a permanent error stops the reconnect loop so
            // the test's `run()` returns instead of spinning forever.
            .unwrap_or_else(|| {
                vec![StreamEvent::Error {
                    status: Some(403),
                    message: "no more script".to_string(),
                }]
            });
        stream::iter(events).boxed_local()
    }
}

const EMPTY_RECEIPT: &str = r#"{"runtimeMutationId":null,"clientMutationId":"x","name":"x","state":"accepted","error":null,"output":null}"#;

struct FakeScheduler {
    sleeps: RefCell<Vec<Duration>>,
}

impl FakeScheduler {
    fn new() -> Self {
        Self {
            sleeps: RefCell::new(Vec::new()),
        }
    }
}

impl Scheduler for FakeScheduler {
    fn sleep(&self, duration: Duration) -> LocalBoxFuture<'static, ()> {
        self.sleeps.borrow_mut().push(duration);
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
