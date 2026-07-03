use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::Stream;

use posthaste_call_policy::BackoffSchedule;
use posthaste_domain_model::{AccountId, GatewayError, PushNotification, PushStream};

/// A raw push transport that opens a single connection and returns a stream.
/// Stateless, does not reconnect. Implementations: SSE, WebSocket.
///
/// @spec docs/L2-transport#push-transport
#[async_trait]
pub trait PushTransport: Send + Sync {
    /// Human-readable name for logging (e.g. "ws", "sse").
    fn name(&self) -> &'static str;

    /// Open a push stream. Returns `None` if the server does not support
    /// this transport (e.g. no WebSocket capability advertised).
    async fn open(
        &self,
        account_id: &AccountId,
        checkpoint: Option<&str>,
    ) -> Result<Option<PushStream>, GatewayError>;

    /// The client-enforced read deadline for this transport's open stream
    /// (PP1/D88): if no item — a real notification, or a keepalive the transport
    /// surfaces — arrives within this window, the connection is declared dead and
    /// the resilient wrapper tears it down and reconnects. WS drives it with an
    /// active `ws_ping`; SSE relies on the server's periodic keepalive ping. The
    /// deadline is set above the keepalive interval so a healthy link never trips
    /// it. **Review** default (a generic value; each transport overrides).
    fn read_deadline(&self) -> Duration {
        PUSH_DEFAULT_READ_DEADLINE
    }

    /// Tear down any shared connection state that backs a now-dead stream, so
    /// interactive traffic stops routing to a corpse (PP1: the audit's worst
    /// finding — mutations eating a 10 s timeout each against a dead WS). Default
    /// no-op: a transport that owns no connection shared with the request path
    /// (SSE) has nothing to tear down; WS disconnects its `SharedWsConnection`.
    async fn on_dead(&self) {}
}

/// Events emitted by a resilient push stream alongside push notifications.
///
/// @spec docs/L2-transport#resilientpushstream
#[derive(Clone, Debug)]
pub enum PushStreamEvent {
    /// A JMAP state-change notification.
    Notification(PushNotification),
    /// Transport connected successfully.
    Connected { transport: &'static str },
    /// Transport disconnected or errored.
    Disconnected {
        transport: &'static str,
        reason: String,
    },
    /// Automatic transport fallback (e.g. WS to SSE).
    Fallback {
        from: &'static str,
        to: &'static str,
    },
    /// The push transport ladder is *structurally* broken (PP6/D91): a
    /// permanent-class open failure (e.g. a 404 eventsource URL) persisted past
    /// the bounded attempts on every available transport. The resilient stream
    /// stops cycling `Reconnecting` forever and parks; the supervisor marks push
    /// terminal and the account falls back to poll-only.
    Terminal {
        transport: &'static str,
        reason: String,
    },
}

/// A single incarnation that stays connected for at least this long resets the
/// push reconnect failure budget — a drop after a sustained-healthy connection is
/// a fresh incident, not part of a reconnect storm (D89, the M21 watchdog's
/// `WATCHDOG_HEALTHY_RESET_AFTER` vocabulary applied to push). **Review**.
pub const PUSH_HEALTHY_RESET_AFTER: Duration = Duration::from_secs(60);
/// Consecutive failures on the active transport before falling back (WS→SSE).
/// Reachable now that the counter survives an accept-then-drop flap (D89/PP2).
/// **Review**.
pub const PUSH_FALLBACK_THRESHOLD: u32 = 3;
/// Consecutive *permanent-class* open failures on the exhausted transport ladder
/// before push is declared terminal and the account falls back to poll-only
/// (PP6/D91) instead of cycling `Reconnecting` forever. **Review**.
pub const PUSH_TERMINAL_PERMANENT_ATTEMPTS: u32 = 3;
/// First-attempt reconnect-backoff ceiling before jitter. **Review**.
pub const PUSH_RECONNECT_BASE: Duration = Duration::from_secs(5);
/// Reconnect-backoff growth factor per accumulated failure. **Review**.
pub const PUSH_RECONNECT_FACTOR: f64 = 2.0;
/// Absolute ceiling on the (pre-jitter) reconnect backoff. **Review**.
pub const PUSH_RECONNECT_CAP: Duration = Duration::from_secs(120);
/// Generic read deadline used when a transport does not override
/// [`PushTransport::read_deadline`]. **Review**.
pub const PUSH_DEFAULT_READ_DEADLINE: Duration = Duration::from_secs(90);

/// Configuration for the resilient push stream's reconnect, fallback, keepalive
/// health-gate, and terminal behaviour (D88/D89/D91). The backoff *schedule* is
/// the shared near-end policy ([`BackoffSchedule`]) — the provider push loop
/// instantiates it rather than re-hand-rolling a jitterless doubling loop
/// (D89/XIV).
///
/// @spec docs/L2-transport#resilientpushstream
pub struct ResilientPushConfig {
    /// The jittered, capped exponential reconnect schedule. Its `max_attempts`
    /// give-up bound is intentionally not used here: push retries *transient*
    /// failures indefinitely (a network outage must not permanently kill push);
    /// giving up is governed instead by `terminal_permanent_attempts` on the
    /// *permanent* class only (PP6).
    pub backoff: BackoffSchedule,
    /// How long a connection must stay held before its success resets the
    /// failure budget (D89 health gate — an accept-then-drop server never earns
    /// the reset, so its backoff escalates instead of pinning at the floor).
    pub healthy_reset_after: Duration,
    /// Consecutive failures on the active transport before falling back.
    pub fallback_threshold: u32,
    /// Consecutive permanent-class failures on the exhausted ladder before push
    /// goes terminal (poll-only).
    pub terminal_permanent_attempts: u32,
    /// Full-jitter source in `[0, 1)`. Injected (not `thread_rng` inside) so the
    /// backoff sequence is deterministic in a virtual-time test — the same reason
    /// the M21 watchdog carries its own jitter source.
    pub jitter: Arc<dyn Fn() -> f64 + Send + Sync>,
}

impl Default for ResilientPushConfig {
    fn default() -> Self {
        Self {
            backoff: BackoffSchedule {
                base: PUSH_RECONNECT_BASE,
                factor: PUSH_RECONNECT_FACTOR,
                cap: PUSH_RECONNECT_CAP,
                // Not a give-up bound for push — see the field doc above.
                max_attempts: u32::MAX,
            },
            healthy_reset_after: PUSH_HEALTHY_RESET_AFTER,
            fallback_threshold: PUSH_FALLBACK_THRESHOLD,
            terminal_permanent_attempts: PUSH_TERMINAL_PERMANENT_ATTEMPTS,
            jitter: Arc::new(jitter_unit),
        }
    }
}

/// A cheap, dependency-free uniform-ish value in `[0, 1)` for full-jitter
/// decorrelation of reconnect backoff (kills the cross-account reconnect herd,
/// D89/Sc1). Not cryptographic; jitter quality is "Review", matching the near-end
/// engine's and the watchdog's own posture.
fn jitter_unit() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos())
        .unwrap_or(0);
    f64::from(nanos % 1_000_000) / 1_000_000.0
}

/// Async stream of [`PushStreamEvent`]s consumed by the supervisor.
///
/// @spec docs/L2-transport#resilientpushstream
pub type PushEventStream = Pin<Box<dyn Stream<Item = PushStreamEvent> + Send>>;
