//! Engine behavior, driven through the injected seams with in-memory fakes.
//!
//! Everything the fakes return is synchronously ready (instant sleeps, scripted
//! responses/streams), so a no-op-waker `block_on` busy-poll drives the async
//! engine without a real executor — keeping the crate dependency-free while
//! still exercising the full reconnect/forward/reconcile paths.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::rc::Rc;
use std::time::Duration;

use futures_util::future::{pending, ready, LocalBoxFuture};
use futures_util::stream::{self, LocalBoxStream};
use futures_util::{FutureExt, StreamExt};

use posthaste_contract_core::{MutationReceipt, MutationRequest};

use crate::config::NearEndConfig;
use crate::outbox::OutboxHooks;
use crate::scheduler::Scheduler;
use crate::sink::{ConnectionStatus, FrameSink};
use crate::transport::{
    PostRequest, PostResponse, StreamEvent, StreamRequest, Transport, TransportError,
};

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
    session_response: PostResponse,
    mutation_responses: RefCell<VecDeque<Result<PostResponse, TransportError>>>,
    /// If set, mutation POSTs never resolve (to exercise the deadline).
    hang_mutations: bool,
    stream_scripts: RefCell<VecDeque<Vec<StreamEvent>>>,
    posts: RefCell<Vec<(String, String)>>,
    stream_urls: RefCell<Vec<String>>,
}

impl FakeTransport {
    fn new() -> Self {
        Self {
            session_response: PostResponse {
                status: 200,
                body: r#"{"sessionId":"session-test"}"#.to_string(),
            },
            mutation_responses: RefCell::new(VecDeque::new()),
            hang_mutations: false,
            stream_scripts: RefCell::new(VecDeque::new()),
            posts: RefCell::new(Vec::new()),
            stream_urls: RefCell::new(Vec::new()),
        }
    }

    fn with_mutation(mut self, response: Result<PostResponse, TransportError>) -> Self {
        self.mutation_responses.get_mut().push_back(response);
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
            ready(Ok(self.session_response.clone())).boxed_local()
        }
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
    frames: RefCell<Vec<super::RuntimeFrame>>,
    malformed: RefCell<Vec<(String, String)>>,
    statuses: RefCell<Vec<String>>,
}

impl FrameSink for RecordingSink {
    fn on_frame(&self, frame: super::RuntimeFrame) {
        self.frames.borrow_mut().push(frame);
    }
    fn on_malformed(&self, raw: String, error: String) {
        self.malformed.borrow_mut().push((raw, error));
    }
    fn on_status(&self, status: ConnectionStatus) {
        let label = match status {
            ConnectionStatus::Connecting => "connecting",
            ConnectionStatus::Connected => "connected",
            ConnectionStatus::Reconnecting => "reconnecting",
            ConnectionStatus::TransientError(_) => "transient",
            ConnectionStatus::PermanentError(_) => "permanent",
        };
        self.statuses.borrow_mut().push(label.to_string());
    }
}

#[derive(Default)]
struct FakeOutbox {
    never: RefCell<Vec<MutationRequest>>,
    reconciled: RefCell<Vec<MutationReceipt>>,
}

impl OutboxHooks for FakeOutbox {
    fn never_dispatched(&self) -> LocalBoxFuture<'static, Vec<MutationRequest>> {
        // Drain: a replayed record is no longer never-dispatched.
        let taken = std::mem::take(&mut *self.never.borrow_mut());
        ready(taken).boxed_local()
    }
    fn on_reconciled(&self, receipt: MutationReceipt) -> LocalBoxFuture<'static, ()> {
        self.reconciled.borrow_mut().push(receipt);
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
    engine: Rc<NearEnd>,
    transport: Rc<FakeTransport>,
    scheduler: Rc<FakeScheduler>,
    sink: Rc<RecordingSink>,
    outbox: Rc<FakeOutbox>,
}

fn harness(transport: FakeTransport, outbox: FakeOutbox, config: NearEndConfig) -> Harness {
    let transport = Rc::new(transport);
    let scheduler = Rc::new(FakeScheduler::new());
    let sink = Rc::new(RecordingSink::default());
    let outbox = Rc::new(outbox);
    let engine = NearEnd::new(
        transport.clone(),
        scheduler.clone(),
        sink.clone(),
        outbox.clone(),
        config,
    );
    Harness {
        engine,
        transport,
        scheduler,
        sink,
        outbox,
    }
}

// ---- tests -----------------------------------------------------------------

#[test]
fn forward_returns_receipt_and_stamps_session() {
    let transport =
        FakeTransport::new().with_mutation(ok_response(200, &confirmed_receipt("op-1")));
    let h = harness(transport, FakeOutbox::default(), NearEndConfig::default());

    let receipt = block_on(async {
        h.engine.open().await.unwrap();
        h.engine.forward(sample_request("op-1")).await
    })
    .expect("forward ok");

    assert_eq!(receipt.client_mutation_id.as_str(), "op-1");
    assert_eq!(receipt.name, "message.setReadState");
    // The forward body stamped the opened session id and round-tripped the op.
    let posts = h.transport.posts.borrow();
    let (url, body) = posts.iter().find(|(u, _)| u.contains("/mutations")).unwrap();
    assert!(url.contains("/runtime/sessions/session-test/mutations"), "{url}");
    assert!(body.contains("\"sessionId\":\"session-test\""), "{body}");
    assert!(body.contains("\"name\":\"message.setReadState\""), "{body}");
}

#[test]
fn forward_retries_transient_then_succeeds() {
    let transport = FakeTransport::new()
        .with_mutation(ok_response(503, "unavailable"))
        .with_mutation(ok_response(200, &confirmed_receipt("op-2")));
    let h = harness(transport, FakeOutbox::default(), NearEndConfig::default());

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
    let body = r#"{"code":"invalid_mutation","message":"nope","retryable":false,"correlationId":null,"details":null}"#;
    let transport = FakeTransport::new().with_mutation(ok_response(422, body));
    let h = harness(transport, FakeOutbox::default(), NearEndConfig::default());

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
fn forward_honors_request_deadline() {
    let mut transport = FakeTransport::new();
    transport.hang_mutations = true;
    let config = NearEndConfig {
        forward_max_attempts: 1,
        ..NearEndConfig::default()
    };
    let h = harness(transport, FakeOutbox::default(), config);

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
            StreamEvent::Message(r#"{"type":"heartbeat","sessionSeq":5}"#.to_string()),
            StreamEvent::Closed,
        ])
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(403),
                message: "stop".to_string(),
            },
        ]);
    let h = harness(transport, FakeOutbox::default(), NearEndConfig::default());

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
    let h = harness(transport, FakeOutbox::default(), NearEndConfig::default());

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
    let h = harness(transport, FakeOutbox::default(), NearEndConfig::default());

    block_on(h.engine.clone().run());

    assert_eq!(h.sink.frames.borrow().len(), 0);
    assert_eq!(h.sink.malformed.borrow().len(), 1);
    assert_eq!(h.engine.cursor(), None);
}

#[test]
fn reconciler_replays_never_dispatched_on_connect() {
    let mut outbox = FakeOutbox::default();
    outbox.never.get_mut().push(sample_request("replay-1"));
    let transport = FakeTransport::new()
        .with_mutation(ok_response(200, &confirmed_receipt("replay-1")))
        .with_stream(vec![
            StreamEvent::Open,
            StreamEvent::Error {
                status: Some(403),
                message: "stop".to_string(),
            },
        ]);
    let h = harness(transport, outbox, NearEndConfig::default());

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
    let reconciled = h.outbox.reconciled.borrow();
    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0].client_mutation_id.as_str(), "replay-1");
}

#[test]
fn shutdown_prevents_reconnect() {
    let transport = FakeTransport::new().with_stream(vec![StreamEvent::Open, StreamEvent::Closed]);
    let h = harness(transport, FakeOutbox::default(), NearEndConfig::default());
    // Request shutdown before running; the loop opens once then exits at the
    // clean close without a reconnect.
    block_on(async {
        h.engine.open().await.unwrap();
        h.engine.request_shutdown();
        h.engine.clone().run().await;
    });
    assert!(h.transport.stream_urls.borrow().len() <= 1);
}
