use futures_util::StreamExt;
use posthaste_call_policy::Terminality;
use posthaste_domain_model::{AccountId, GatewayError};
use posthaste_domain_service::{PushEventStream, PushStreamEvent, PushTransport, ResilientPushConfig};
use posthaste_observability::{events, ph_debug, ph_warn};

/// Which transport is currently being used by the resilient stream.
enum ActiveTransport {
    Primary,
    Fallback,
}

/// Classify a push-transport open failure by its retryability (D82/PP6). This is
/// the [`Terminality`]-gated terminal path's decision function: a *permanent*
/// failure (a 404 eventsource URL, a server rejection) must not be retried
/// forever like a *transient* network blip. Exhaustive over [`GatewayError`] with
/// **no wildcard** — the M29 gate posture: a new variant fails to compile here
/// until its push terminality is decided, rather than silently defaulting to an
/// infinite reconnect cycle.
fn open_failure_terminality(error: &GatewayError) -> Terminality {
    match error {
        // Terminal as written: a structural/config fault or a rejection that a
        // reconnect cannot change.
        GatewayError::Rejected(_)
        | GatewayError::StateMismatch
        | GatewayError::CannotCalculateChanges
        | GatewayError::Corruption(_)
        | GatewayError::Internal(_)
        | GatewayError::MutationRejected { .. } => Terminality::Permanent,
        // Reachable-again: the network/credentials may recover, so keep
        // reconnecting under backoff.
        GatewayError::Network(_)
        | GatewayError::Unavailable(_)
        | GatewayError::Auth
        | GatewayError::DispatchUncertain(_) => Terminality::Transient,
    }
}

/// Build a resilient push notification stream that reconnects with jittered
/// backoff, enforces a keepalive read-deadline (PP1), falls back from the
/// primary transport (WS) to the fallback (SSE) after repeated failures (PP2),
/// and — when the whole ladder fails *permanently* — stops cycling and goes
/// terminal so the account falls back to poll-only (PP6).
///
/// @spec docs/L2-transport#resilientpushstream
/// @spec docs/L2-transport#http-fallback
pub fn resilient_push_stream(
    account_id: AccountId,
    primary: Box<dyn PushTransport>,
    fallback: Option<Box<dyn PushTransport>>,
    config: ResilientPushConfig,
) -> PushEventStream {
    Box::pin(async_stream::stream! {
        let mut active = ActiveTransport::Primary;
        // The reconnect attempt counter. Critically it is **not** reset on a
        // successful `open()` (the audit's push.rs:44-45 defect); it is reset
        // only when a connection is *held healthy* past `healthy_reset_after`
        // (the D89 health gate), so an accept-then-drop server escalates backoff
        // and reaches the fallback threshold instead of pinning at the floor.
        let mut consecutive_failures: u32 = 0;
        // Consecutive *permanent-class* failures on the exhausted ladder — the
        // terminal (poll-only) trip counter (PP6).
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

                    // Consume the stream under a per-item keepalive read-deadline
                    // (PP1/D88). A NAT half-open socket delivers no traffic and no
                    // error — `stream.next()` would park forever — so the timeout
                    // is what turns silent death into a detectable disconnect.
                    let reason = loop {
                        match tokio::time::timeout(read_deadline, stream.next()).await {
                            Err(_elapsed) => {
                                // No notification and no keepalive within the
                                // deadline: the connection is dead. Tear down the
                                // backing connection so interactive mutations stop
                                // routing to a corpse (PP1 dead-WS teardown).
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

                    // Health gate (D89): a connection held past the healthy
                    // window earns a full reset — a drop after a long-lived stream
                    // is a fresh incident, not part of a reconnect storm.
                    if opened_at.elapsed() >= config.healthy_reset_after {
                        consecutive_failures = 0;
                    }
                    consecutive_failures += 1;
                    // A stream that *opened* is not a structural/permanent fault,
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
                    let terminality = open_failure_terminality(&error);
                    ph_warn!(
                        events::PUSH_TRANSPORT_OPEN_FAILED,
                        account_id = %account_id,
                        transport = transport.name(),
                        error = %error,
                        terminality = ?terminality,
                        attempt = consecutive_failures + 1,
                        fallback_threshold = config.fallback_threshold,
                        "push transport open failed"
                    );
                    yield PushStreamEvent::Disconnected {
                        transport: transport.name(),
                        reason: error.to_string(),
                    };
                    consecutive_failures += 1;
                    match terminality {
                        Terminality::Permanent => permanent_streak += 1,
                        Terminality::Transient => permanent_streak = 0,
                    }
                }
            }

            // Terminal check (PP6/D91): a *permanent* fault that persisted past
            // the bounded attempts, with no healthy transport left to try. Stop
            // cycling `Reconnecting` forever; go terminal and park (the stream
            // never yields again — the 60 s safety-net poll keeps the account
            // fresh from here).
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

            // Fallback check (PP2): reachable now that failures accumulate across
            // an accept-then-drop flap.
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

            // Jittered backoff (D89): the shared near-end schedule, keyed on the
            // accumulated failure count, decorrelated by the injected jitter.
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

#[cfg(test)]
mod tests;
