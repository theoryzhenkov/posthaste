//! The resilient push stream: reconnects a provider push transport with
//! jittered backoff, enforces a keepalive read-deadline, falls back from the
//! primary transport (WS) to the fallback (SSE) after repeated failures, and
//! goes terminal (poll-only) when the whole ladder fails permanently.

use futures_util::StreamExt;
use posthaste_domain_model::{AccountId, GatewayError};
use posthaste_domain_service::{
    PushEventStream, PushStreamEvent, PushTransport, ResilientPushConfig,
};
use posthaste_observability::{events, ph_debug, ph_warn};

/// Which transport is currently being used by the resilient stream.
enum ActiveTransport {
    Primary,
    Fallback,
}

/// Whether a push-transport open failure is permanent (a structural/config
/// fault or a rejection a reconnect cannot change) rather than transient
/// (the network/credentials may recover, so keep reconnecting under
/// backoff). Exhaustive with no wildcard so a new [`GatewayError`] variant
/// fails to compile here until its push retryability is decided.
fn open_failure_is_permanent(error: &GatewayError) -> bool {
    match error {
        GatewayError::Rejected(_)
        | GatewayError::StateMismatch
        | GatewayError::CannotCalculateChanges
        | GatewayError::Corruption(_)
        | GatewayError::Internal(_)
        | GatewayError::MutationRejected { .. }
        | GatewayError::MailboxNotEmpty { .. } => true,
        GatewayError::Network(_)
        | GatewayError::Unavailable(_)
        | GatewayError::Auth
        | GatewayError::DispatchUncertain(_) => false,
    }
}

/// Build a resilient push notification stream over a primary transport and
/// an optional fallback. The stream yields [`PushStreamEvent`]s forever; a
/// permanent failure of the exhausted ladder yields `Terminal` once and then
/// parks (the poll interval keeps the account fresh from there).
pub(crate) fn resilient_push_stream(
    account_id: AccountId,
    primary: Box<dyn PushTransport>,
    fallback: Option<Box<dyn PushTransport>>,
    config: ResilientPushConfig,
) -> PushEventStream {
    Box::pin(async_stream::stream! {
        let mut active = ActiveTransport::Primary;
        // Not reset on a successful `open()`: only a connection held healthy
        // past `healthy_reset_after` earns the reset, so an accept-then-drop
        // server escalates backoff and reaches the fallback threshold
        // instead of pinning at the floor.
        let mut consecutive_failures: u32 = 0;
        // Consecutive permanent-class failures on the exhausted ladder — the
        // terminal (poll-only) trip counter.
        let mut permanent_streak: u32 = 0;
        let mut checkpoint: Option<String> = None;

        loop {
            let transport: &dyn PushTransport = match active {
                ActiveTransport::Primary => &*primary,
                ActiveTransport::Fallback => match &fallback {
                    Some(fb) => &**fb,
                    None => &*primary,
                },
            };
            let read_deadline = transport.read_deadline();

            match transport.open(&account_id, checkpoint.as_deref()).await {
                Ok(Some(mut stream)) => {
                    let opened_at = tokio::time::Instant::now();
                    yield PushStreamEvent::Connected {
                        transport: transport.name(),
                    };

                    // Consume the stream under a per-item keepalive
                    // read-deadline: a half-open socket delivers no traffic
                    // and no error, so the timeout is what turns silent death
                    // into a detectable disconnect.
                    let reason = loop {
                        match tokio::time::timeout(read_deadline, stream.next()).await {
                            Err(_elapsed) => {
                                transport.on_dead().await;
                                break format!(
                                    "read deadline exceeded ({}s; no traffic or keepalive)",
                                    read_deadline.as_secs()
                                );
                            }
                            Ok(None) => break "stream ended".to_string(),
                            Ok(Some(Ok(notification))) => {
                                if notification.checkpoint.is_some() {
                                    checkpoint.clone_from(&notification.checkpoint);
                                }
                                yield PushStreamEvent::Notification(notification);
                            }
                            Ok(Some(Err(error))) => {
                                transport.on_dead().await;
                                break error.to_string();
                            }
                        }
                    };

                    yield PushStreamEvent::Disconnected {
                        transport: transport.name(),
                        reason,
                    };

                    // Health gate: a connection held past the healthy window
                    // earns a full reset — a drop after a long-lived stream is
                    // a fresh incident, not part of a reconnect storm.
                    if opened_at.elapsed() >= config.healthy_reset_after {
                        consecutive_failures = 0;
                    }
                    consecutive_failures += 1;
                    // A stream that opened is not a structural fault,
                    // regardless of how briefly it lived.
                    permanent_streak = 0;
                }
                Ok(None) => {
                    ph_debug!(
                        events::PUSH_TRANSPORT_UNSUPPORTED,
                        account_id = %account_id,
                        transport = transport.name(),
                        "transport unsupported by server"
                    );
                    consecutive_failures += 1;
                    // "Unsupported" is a structural signal for this transport.
                    permanent_streak += 1;
                }
                Err(error) => {
                    let permanent = open_failure_is_permanent(&error);
                    ph_warn!(
                        events::PUSH_TRANSPORT_OPEN_FAILED,
                        account_id = %account_id,
                        transport = transport.name(),
                        error = %error,
                        permanent,
                        attempt = consecutive_failures + 1,
                        fallback_threshold = config.fallback_threshold,
                        "push transport open failed"
                    );
                    yield PushStreamEvent::Disconnected {
                        transport: transport.name(),
                        reason: error.to_string(),
                    };
                    consecutive_failures += 1;
                    if permanent {
                        permanent_streak += 1;
                    } else {
                        permanent_streak = 0;
                    }
                }
            }

            // Terminal check: a permanent fault that persisted past the
            // bounded attempts, with no healthy transport left to try. Stop
            // cycling; park (the stream never yields again — the poll keeps
            // the account fresh from here).
            let ladder_exhausted =
                fallback.is_none() || matches!(active, ActiveTransport::Fallback);
            if ladder_exhausted && permanent_streak >= config.terminal_permanent_attempts {
                ph_warn!(
                    events::PUSH_TERMINAL,
                    account_id = %account_id,
                    transport = transport.name(),
                    permanent_streak,
                    "push transport permanently failed; falling back to poll-only"
                );
                yield PushStreamEvent::Terminal {
                    transport: transport.name(),
                    reason: format!(
                        "{} permanently unavailable after {permanent_streak} attempts",
                        transport.name()
                    ),
                };
                std::future::pending::<()>().await;
            }

            // Fallback check: failures accumulate across accept-then-drop
            // flaps, so the threshold is reachable.
            if consecutive_failures >= config.fallback_threshold {
                if let Some(ref fb) = fallback {
                    match active {
                        ActiveTransport::Primary => {
                            ph_warn!(
                                events::PUSH_FALLBACK_TRIGGERED,
                                account_id = %account_id,
                                from = primary.name(),
                                to = fb.name(),
                                consecutive_failures,
                                "push transport fallback triggered"
                            );
                            active = ActiveTransport::Fallback;
                            consecutive_failures = 0;
                            // Give the fallback its own bounded permanent budget.
                            permanent_streak = 0;
                            yield PushStreamEvent::Fallback {
                                from: primary.name(),
                                to: fb.name(),
                            };
                            continue; // try fallback immediately
                        }
                        ActiveTransport::Fallback => {
                            ph_warn!(
                                events::PUSH_FALLBACK_CYCLED_TO_PRIMARY,
                                account_id = %account_id,
                                from = fb.name(),
                                to = primary.name(),
                                consecutive_failures,
                                "push fallback exhausted, cycling back to primary"
                            );
                            active = ActiveTransport::Primary;
                            consecutive_failures = 0;
                            permanent_streak = 0;
                            continue;
                        }
                    }
                }
            }

            // Jittered backoff, keyed on the accumulated failure count.
            let delay = config
                .backoff
                .delay_for(consecutive_failures, (config.jitter)());
            ph_debug!(
                events::PUSH_RECONNECT_BACKOFF,
                account_id = %account_id,
                delay_ms = delay.as_millis(),
                attempt = consecutive_failures,
                fallback_threshold = config.fallback_threshold,
                "push reconnect backoff"
            );
            tokio::time::sleep(delay).await;
        }
    })
}
