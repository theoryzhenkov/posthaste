use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use posthaste_call_policy::BackoffSchedule;
use posthaste_domain_model::{GatewayError, PushNotification, PushStream};

use super::*;

/// A configurable mock transport. `open_fn` decides each `open()` outcome;
/// `read_deadline` and `on_dead` are wired so the read-deadline / teardown paths
/// (PP1) can be exercised without a real socket.
struct MockTransport {
    name: &'static str,
    open_fn: Box<dyn Fn() -> Result<Option<PushStream>, GatewayError> + Send + Sync>,
    read_deadline: Duration,
    on_dead_calls: Arc<AtomicU32>,
}

impl MockTransport {
    fn new(
        name: &'static str,
        open_fn: impl Fn() -> Result<Option<PushStream>, GatewayError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name,
            open_fn: Box::new(open_fn),
            read_deadline: Duration::from_secs(90),
            on_dead_calls: Arc::new(AtomicU32::new(0)),
        }
    }

    fn with_read_deadline(mut self, deadline: Duration) -> Self {
        self.read_deadline = deadline;
        self
    }
}

#[async_trait::async_trait]
impl PushTransport for MockTransport {
    fn name(&self) -> &'static str {
        self.name
    }
    fn read_deadline(&self) -> Duration {
        self.read_deadline
    }
    async fn on_dead(&self) {
        self.on_dead_calls.fetch_add(1, Ordering::SeqCst);
    }
    async fn open(
        &self,
        _account_id: &AccountId,
        _checkpoint: Option<&str>,
    ) -> Result<Option<PushStream>, GatewayError> {
        (self.open_fn)()
    }
}

fn notification(id: &str) -> PushNotification {
    PushNotification {
        account_id: AccountId::from("test"),
        changed: vec!["Email".to_string()],
        received_at: "2026-01-01T00:00:00Z".to_string(),
        checkpoint: Some(id.to_string()),
    }
}

/// A config with no jitter and effectively no backoff sleep, for tests that only
/// care about the state machine (not the timing).
fn immediate_config(fallback_threshold: u32) -> ResilientPushConfig {
    ResilientPushConfig {
        backoff: BackoffSchedule {
            base: Duration::from_millis(1),
            factor: 2.0,
            cap: Duration::from_millis(10),
            max_attempts: u32::MAX,
        },
        healthy_reset_after: Duration::from_secs(3600),
        fallback_threshold,
        terminal_permanent_attempts: 3,
        jitter: Arc::new(|| 0.0),
    }
}

#[tokio::test]
async fn yields_notifications_from_primary() {
    let primary = Box::new(MockTransport::new("primary", || {
        let stream: PushStream = Box::pin(futures_util::stream::iter(vec![Ok(notification("1"))]));
        Ok(Some(stream))
    }));

    let mut stream =
        resilient_push_stream(AccountId::from("test"), primary, None, immediate_config(3));

    let event = stream.next().await.unwrap();
    assert!(matches!(
        event,
        PushStreamEvent::Connected {
            transport: "primary"
        }
    ));

    let event = stream.next().await.unwrap();
    assert!(matches!(event, PushStreamEvent::Notification(_)));

    let event = stream.next().await.unwrap();
    assert!(matches!(event, PushStreamEvent::Disconnected { .. }));
}

#[tokio::test]
async fn passes_checkpoint_on_reconnect() {
    let call_count = Arc::new(AtomicU32::new(0));
    let call_count_ = call_count.clone();

    let primary = Box::new(MockTransport::new("primary", move || {
        let n = call_count_.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            let stream: PushStream =
                Box::pin(futures_util::stream::iter(vec![Ok(notification("cp42"))]));
            Ok(Some(stream))
        } else {
            Ok(None)
        }
    }));

    let mut stream =
        resilient_push_stream(AccountId::from("test"), primary, None, immediate_config(3));

    stream.next().await; // Connected
    let event = stream.next().await.unwrap();
    if let PushStreamEvent::Notification(n) = event {
        assert_eq!(n.checkpoint, Some("cp42".to_string()));
    } else {
        panic!("expected notification");
    }
    stream.next().await; // Disconnected
}

/// PP2/D89 regression: an accept-then-drop server (opens, then drops within
/// seconds) must **escalate** backoff (delays grow, the counter is never reset
/// per-open) and thereby **reach** the WS→SSE fallback threshold. Under the old
/// per-open reset the counter oscillated at 1 and the threshold was unreachable.
#[tokio::test(start_paused = true)]
async fn accept_then_drop_escalates_backoff_and_falls_back() {
    let start = tokio::time::Instant::now();
    let primary_opens: Arc<Mutex<Vec<Duration>>> = Arc::new(Mutex::new(Vec::new()));
    let primary_opens_ = primary_opens.clone();

    // Primary: opens, then the stream immediately ends (accept-then-drop).
    let primary = Box::new(MockTransport::new("primary", move || {
        primary_opens_.lock().unwrap().push(start.elapsed());
        let stream: PushStream = Box::pin(futures_util::stream::empty());
        Ok(Some(stream))
    }));
    // Fallback: connects and stays (one notification keeps it alive).
    let fallback = Box::new(MockTransport::new("fallback", || {
        let stream: PushStream = Box::pin(futures_util::stream::iter(vec![Ok(notification("f1"))]));
        Ok(Some(stream))
    }));

    let config = ResilientPushConfig {
        backoff: BackoffSchedule {
            base: Duration::from_millis(100),
            factor: 2.0,
            cap: Duration::from_secs(60),
            max_attempts: u32::MAX,
        },
        healthy_reset_after: Duration::from_secs(60),
        fallback_threshold: 3,
        terminal_permanent_attempts: 5,
        // Full ceiling (deterministic), so each backoff is exactly `ceiling(n)`.
        jitter: Arc::new(|| 1.0),
    };

    let mut stream =
        resilient_push_stream(AccountId::from("test"), primary, Some(fallback), config);

    // Drive until the fallback transport connects.
    let mut saw_fallback_event = false;
    loop {
        match stream.next().await.unwrap() {
            PushStreamEvent::Fallback { from, to } => {
                assert_eq!((from, to), ("primary", "fallback"));
                saw_fallback_event = true;
            }
            PushStreamEvent::Connected {
                transport: "fallback",
            } => break,
            _ => {}
        }
    }
    assert!(saw_fallback_event, "WS→SSE fallback must fire");

    // The primary was retried exactly `fallback_threshold` times (the counter
    // accumulated across opens — no per-open reset) ...
    let opens = primary_opens.lock().unwrap().clone();
    assert_eq!(
        opens.len(),
        3,
        "primary opened once per accumulated failure"
    );
    // ... and the gaps between opens strictly grow (backoff escalates).
    assert!(
        opens[1] - opens[0] < opens[2] - opens[1],
        "reconnect delay must escalate, got opens at {opens:?}"
    );
}

/// PP1/D88 regression: a NAT half-open stream delivers no traffic and no error;
/// the client read-deadline is what flips it to `Reconnecting`, and `on_dead`
/// fires so interactive mutations stop routing to the corpse (dead-WS teardown).
#[tokio::test(start_paused = true)]
async fn nat_half_open_flips_reconnecting_within_read_deadline_and_tears_down() {
    let read_deadline = Duration::from_secs(90);
    let primary = MockTransport::new("primary", || {
        // Opens, then never yields (half-open socket).
        let stream: PushStream = Box::pin(futures_util::stream::pending());
        Ok(Some(stream))
    })
    .with_read_deadline(read_deadline);
    let on_dead_calls = primary.on_dead_calls.clone();

    let mut stream = resilient_push_stream(
        AccountId::from("test"),
        Box::new(primary),
        None,
        immediate_config(3),
    );

    let connected_at = tokio::time::Instant::now();
    assert!(matches!(
        stream.next().await.unwrap(),
        PushStreamEvent::Connected { .. }
    ));

    // The next event is a Disconnected produced by the read-deadline, not by any
    // stream error — and it arrives within the deadline window.
    match stream.next().await.unwrap() {
        PushStreamEvent::Disconnected { reason, .. } => {
            assert!(
                reason.contains("read deadline"),
                "expected a read-deadline disconnect, got: {reason}"
            );
        }
        other => panic!("expected Disconnected, got {other:?}"),
    }
    assert!(connected_at.elapsed() <= read_deadline);
    assert_eq!(
        on_dead_calls.load(Ordering::SeqCst),
        1,
        "the dead connection must be torn down so mutations stop routing to it"
    );
}

/// PP6/D91 regression: a permanent-class open failure (e.g. a 404 eventsource
/// URL → `Rejected`) must not cycle `Reconnecting` forever — after the bounded
/// attempts the stream emits `Terminal` and parks (poll-only), rather than
/// reopening endlessly.
#[tokio::test(start_paused = true)]
async fn permanent_open_failure_goes_terminal_without_infinite_cycle() {
    let opens = Arc::new(AtomicU32::new(0));
    let opens_ = opens.clone();
    let primary = Box::new(MockTransport::new("primary", move || {
        opens_.fetch_add(1, Ordering::SeqCst);
        Err(GatewayError::Rejected(
            "404 eventsource not found".to_string(),
        ))
    }));

    let config = ResilientPushConfig {
        terminal_permanent_attempts: 3,
        fallback_threshold: 5,
        ..immediate_config(5)
    };

    let mut stream = resilient_push_stream(AccountId::from("test"), primary, None, config);

    // Consume until the Terminal event.
    loop {
        match stream.next().await.unwrap() {
            PushStreamEvent::Terminal { reason, .. } => {
                assert!(reason.contains("permanently unavailable"));
                break;
            }
            PushStreamEvent::Disconnected { .. } => {}
            other => panic!("unexpected event before terminal: {other:?}"),
        }
    }
    assert_eq!(
        opens.load(Ordering::SeqCst),
        3,
        "must stop after the bounded permanent attempts"
    );

    // After Terminal the stream parks: no further events, and the open count
    // stays put even after a long wait (no infinite cycle).
    let next = tokio::time::timeout(Duration::from_secs(3600), stream.next()).await;
    assert!(next.is_err(), "stream must park after going terminal");
    assert_eq!(opens.load(Ordering::SeqCst), 3);
}
