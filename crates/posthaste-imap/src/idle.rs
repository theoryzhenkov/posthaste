//! IMAP IDLE push hints on the shared per-account session.
//!
//! RFC 2177 IDLE is mailbox-selected and advisory: it wakes the sync loop when
//! the server reports activity, but periodic poll remains the correctness
//! fallback for missed events and unobserved mailboxes.
//!
//! Lifecycle (audit C2/C3, RFC-L2-provider-reliability D92):
//! - IDLE runs on the account's **single shared session** (D92c/O3), holding
//!   it through [`ImapSessionManager::idle_wait`]. Any operation that needs
//!   the session *recalls* the hold (`DONE` + release); IDLE re-issues once
//!   the fair mutex hands the slot back. IDLE never opens its own connection.
//! - The IDLE protocol loop runs in a **spawned task**, not inline in the
//!   returned stream: the account runtime's select loop stops polling the
//!   push stream while a sync arm runs, and an unpolled stream holding the
//!   session would deadlock the recall handshake. A task is always polled;
//!   the stream just drains its channel. Dropping the stream aborts the task,
//!   and the session manager's poison flag cleans up an abort that lands
//!   mid-protocol.
//! - Every wait is bounded: IDLE is re-issued (`DONE` + fresh `IDLE`) every
//!   [`crate::session::idle_reissue_interval`] (24 min, under the ~29 min
//!   server/NAT cutoffs), and a dead socket trips the
//!   [`crate::session::idle_max_wait`] ceiling instead of hanging.
//! - Suspected IDLE-rejects (holds that end implausibly fast) and connection
//!   failures back off with capped full jitter instead of hammering the
//!   server on a fixed 30 s cadence.
//!
//! @spec docs/L0-providers#imap-smtp-sync-strategy
//! @spec docs/L1-sync#sync-loop

use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use posthaste_call_policy::BackoffSchedule;
use posthaste_domain_model::AccountId;
use posthaste_domain_model::{now_iso8601, PushNotification};
use posthaste_domain_service::{PushEventStream, PushStreamEvent};
use posthaste_observability::{events, ph_debug, ph_warn};

use crate::session::{IdleWaitOutcome, ImapSessionManager};

/// Backoff for IDLE trouble: reconnects after a dropped session and suspected
/// IDLE-rejects. Full jitter (C3/Sc1) so many accounts don't re-IDLE in step.
fn idle_backoff() -> BackoffSchedule {
    BackoffSchedule {
        base: Duration::from_secs(1),
        factor: 2.0,
        cap: Duration::from_secs(5 * 60),
        max_attempts: u32::MAX,
    }
}

/// Open an IMAP IDLE watcher as a best-effort push hint stream on the shared
/// account session.
pub(crate) fn imap_idle_event_stream(
    account_id: AccountId,
    manager: Arc<ImapSessionManager>,
    mailbox_name: String,
) -> PushEventStream {
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<PushStreamEvent>(16);
    let task = tokio::spawn(run_idle_task(account_id, manager, mailbox_name, event_tx));
    let abort_on_drop = AbortOnDrop(task);

    Box::pin(stream! {
        // Owned by the stream: dropping the stream aborts the IDLE task. The
        // session manager's poison sweep reconnects if the abort landed
        // mid-protocol.
        let _abort_on_drop = abort_on_drop;
        while let Some(event) = event_rx.recv().await {
            yield event;
        }
    })
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn run_idle_task(
    account_id: AccountId,
    manager: Arc<ImapSessionManager>,
    mailbox_name: String,
    events_tx: tokio::sync::mpsc::Sender<PushStreamEvent>,
) {
    let backoff = idle_backoff();
    let mut trouble_streak: u32 = 0;
    let mut announced_connected = false;

    loop {
        match manager.idle_wait(&mailbox_name).await {
            Ok(outcome) => {
                if !announced_connected {
                    announced_connected = true;
                    if send_event(
                        &events_tx,
                        PushStreamEvent::Connected {
                            transport: "imap-idle",
                        },
                    )
                    .await
                    .is_err()
                    {
                        return;
                    }
                }
                match outcome {
                    IdleWaitOutcome::Activity { suspect_reject } => {
                        ph_debug!(
                            events::IMAP_IDLE_RETURNED,
                            account_id = %account_id,
                            mailbox_name,
                            suspect_reject,
                            "IMAP IDLE returned"
                        );
                        let received_at = match now_iso8601() {
                            Ok(received_at) => received_at,
                            Err(reason) => {
                                let _ = send_event(
                                    &events_tx,
                                    PushStreamEvent::Disconnected {
                                        transport: "imap-idle",
                                        reason,
                                    },
                                )
                                .await;
                                return;
                            }
                        };
                        if send_event(
                            &events_tx,
                            PushStreamEvent::Notification(imap_idle_notification(
                                account_id.clone(),
                                received_at,
                            )),
                        )
                        .await
                        .is_err()
                        {
                            return;
                        }
                        if suspect_reject {
                            // Indistinguishable from an IDLE-reject at the
                            // client API: notify (harmless if genuine) but
                            // back off before re-issuing so a rejecting
                            // server is not hammered (C3).
                            trouble_streak = trouble_streak.saturating_add(1);
                            let delay =
                                backoff.delay_for(trouble_streak.saturating_sub(1), rand_unit());
                            ph_warn!(
                                events::IMAP_IDLE_REJECT_BACKOFF,
                                account_id = %account_id,
                                mailbox_name,
                                streak = trouble_streak,
                                delay_ms = delay.as_millis() as u64,
                                "IMAP IDLE ended suspiciously fast; backing off before re-issue"
                            );
                            tokio::time::sleep(delay).await;
                        } else {
                            trouble_streak = 0;
                        }
                    }
                    IdleWaitOutcome::Recalled | IdleWaitOutcome::ReissueTick => {
                        // Recalled: the fair session mutex queues our next
                        // hold behind the recalling operation — loop straight
                        // back in. ReissueTick: quiet DONE/re-IDLE cycle.
                        trouble_streak = 0;
                    }
                }
            }
            Err(error) => {
                ph_warn!(
                    events::IMAP_IDLE_DISCONNECTED,
                    account_id = %account_id,
                    mailbox_name,
                    error = %error,
                    "IMAP IDLE hold failed"
                );
                announced_connected = false;
                if send_event(
                    &events_tx,
                    PushStreamEvent::Disconnected {
                        transport: "imap-idle",
                        reason: error.to_string(),
                    },
                )
                .await
                .is_err()
                {
                    return;
                }
                trouble_streak = trouble_streak.saturating_add(1);
                let delay = backoff.delay_for(trouble_streak.saturating_sub(1), rand_unit());
                tokio::time::sleep(delay).await;
            }
        }
    }
}

async fn send_event(
    events_tx: &tokio::sync::mpsc::Sender<PushStreamEvent>,
    event: PushStreamEvent,
) -> Result<(), ()> {
    events_tx.send(event).await.map_err(|_closed| ())
}

/// Full-jitter unit draw (see `session::jitter_unit`; duplicated to keep the
/// modules decoupled).
fn rand_unit() -> f64 {
    f64::from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since_epoch| since_epoch.subsec_nanos())
            .unwrap_or(0),
    ) / f64::from(1_000_000_000u32)
}

fn imap_idle_notification(account_id: AccountId, received_at: String) -> PushNotification {
    PushNotification {
        account_id,
        changed: Vec::new(),
        received_at,
        checkpoint: None,
    }
}

#[cfg(test)]
mod tests;
