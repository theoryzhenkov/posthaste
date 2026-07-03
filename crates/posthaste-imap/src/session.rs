//! Per-account IMAP session manager: one long-lived authenticated session,
//! reused across every operation (D92/O3).
//!
//! Before this module, every sync / mutation / body fetch / draft / IDLE call
//! opened its own TCP+TLS+AUTH connection (audit C4). Gmail enforces a hard
//! 15-simultaneous-connection limit per account plus connection-establishment
//! rate limits, so a burst (sync while tagging, IDLE plus a fetch) stormed past
//! the limit and the account was throttled or locked out. The ratified fix
//! (RFC-L2-provider-reliability D92c, ruling O3) is a **single reused
//! authenticated session per account** — not a pool — with reconnect-on-drop
//! under jittered backoff.
//!
//! Shape: one slot behind a fair async mutex. Operations `acquire()` a
//! [`SessionLease`], run their protocol commands (each individually
//! deadline-bounded via [`crate::timeout`]), and `finish()` the lease so
//! connection-fatal errors poison the slot (next acquire reconnects). IDLE
//! holds the same slot through [`ImapSessionManager::idle_wait`] and is
//! *recallable*: an operation that needs the session fires the `recall`
//! notify, the IDLE holder sends `DONE`, releases the slot, and re-issues IDLE
//! once the fair mutex hands the slot back. Because IDLE runs in its own
//! spawned task (see [`crate::idle`]), a recalled hold is always polled — the
//! account runtime's select loop can never deadlock against it.
//!
//! Cancellation safety: an aborted holder (a select-arm budget timeout
//! dropping a sync future, or the IDLE task being aborted at teardown) may
//! leave the wire mid-command. Every hold sets `dirty` on the slot and clears
//! it only on an orderly finish; the next acquire sees a still-dirty slot,
//! discards the client, and reconnects instead of talking over a desynced
//! protocol stream.
//!
//! @spec docs/L0-providers#imap-smtp-sync-strategy

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use imap_client::client::tokio::Client as ImapClient;
use posthaste_call_policy::BackoffSchedule;
use posthaste_domain_model::GatewayError;
use posthaste_domain_service::SecretResolver;
use posthaste_observability::{events, ph_debug, ph_info, ph_warn};

use crate::discovery::connect_authenticated_client;
use crate::timeout::with_deadline;
use crate::{ImapAdapterError, ImapConnectionConfig};

/// Upper bound on simultaneously-open IMAP connections per account.
///
/// **Flagged constant (D92/O3).** The ratified default is a single reused
/// session; the slot design *is* the enforcement (there is exactly one slot,
/// so the manager can never hold more than this many connections). Gmail's
/// hard server-side cap is 15 simultaneous IMAP connections per account —
/// this stays far under it on purpose. Revisit only on mutation-burst latency
/// evidence (O3), and never raise past ~10 for Gmail.
pub const IMAP_MAX_SESSIONS_PER_ACCOUNT: usize = 1;

/// Re-issue IDLE (send `DONE`, then a fresh `IDLE`) after this long with no
/// server activity. RFC 2177 recommends re-issuing at least every 29 minutes
/// and Gmail drops idle connections around that mark; 24 minutes leaves a
/// full command round-trip of margin (audit C2).
///
/// Held as milliseconds behind an atomic so a test can shrink the re-issue
/// cycle to real-time-testable lengths (same seam pattern as
/// [`crate::timeout::IMAP_OP_TIMEOUT_MS`]).
pub(crate) static IMAP_IDLE_REISSUE_MS: AtomicU64 = AtomicU64::new(24 * 60 * 1000);

pub(crate) fn idle_reissue_interval() -> Duration {
    Duration::from_millis(IMAP_IDLE_REISSUE_MS.load(Ordering::Relaxed))
}

/// Test-only: shrink the IDLE re-issue interval. Returns a guard closure that
/// restores the previous value.
#[cfg(test)]
pub(crate) fn set_idle_reissue_ms_for_testing(ms: u64) -> impl FnOnce() {
    let previous = IMAP_IDLE_REISSUE_MS.swap(ms, Ordering::Relaxed);
    move || {
        IMAP_IDLE_REISSUE_MS.store(previous, Ordering::Relaxed);
    }
}

/// Hard ceiling on a single IDLE hold: the re-issue interval plus one per-op
/// deadline. `imap-client` sends `DONE` at the re-issue mark and then awaits
/// the tagged reply; on a half-open socket that reply never comes, and without
/// this outer bound the hold would hang forever (the C2 dead-socket case).
pub(crate) fn idle_max_wait() -> Duration {
    idle_reissue_interval() + crate::timeout::op_timeout()
}

/// An IDLE hold that returns quicker than this is treated as a suspected
/// IDLE-reject (the client library reports a rejected IDLE and instant
/// activity identically), so the caller applies jittered backoff instead of
/// hammering the server with immediate re-IDLEs (audit C3).
pub(crate) const IMAP_IDLE_SHORT_CYCLE: Duration = Duration::from_secs(2);

/// Jittered backoff between reconnect attempts after consecutive connect
/// failures (reconnect-on-drop must not become a connect storm). Uses the
/// M31 call-policy schedule: full jitter, capped.
fn reconnect_backoff() -> BackoffSchedule {
    BackoffSchedule {
        base: Duration::from_millis(500),
        factor: 2.0,
        cap: Duration::from_secs(30),
        // The manager never gives up on its own: each acquire makes one
        // attempt and surfaces the error; `max_attempts` only shapes
        // `delay_for`, which ignores it.
        max_attempts: u32::MAX,
    }
}

/// The outcome of one recallable IDLE hold (see [`ImapSessionManager::idle_wait`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IdleWaitOutcome {
    /// The server reported mailbox activity — the caller should surface a
    /// push hint. `suspect_reject` is set when the hold ended suspiciously
    /// fast (indistinguishable from an IDLE-reject at this API), so the
    /// caller should back off before re-issuing.
    Activity { suspect_reject: bool },
    /// An operation recalled the session; nothing happened server-side. The
    /// caller just re-issues (the fair mutex queues it behind the operation).
    Recalled,
    /// The re-issue interval elapsed with no activity; `DONE` was sent and
    /// acknowledged. The caller re-issues without surfacing an event.
    ReissueTick,
}

struct SessionSlot {
    client: Option<ImapClient>,
    /// True while a lease/hold is in flight. Still true at the next acquire
    /// means the previous holder was cancelled mid-protocol: reconnect.
    dirty: bool,
    /// Bumped on every operation acquire and every reconnect, so the IDLE
    /// holder knows when its EXAMINE'd mailbox selection may have been
    /// changed underneath it.
    epoch: u64,
    /// The epoch at which IDLE last EXAMINE'd its watch mailbox.
    idle_examined_at_epoch: Option<u64>,
    consecutive_connect_failures: u32,
    next_connect_at: Option<tokio::time::Instant>,
}

/// Per-account owner of the single reused authenticated IMAP session (D92/O3).
///
/// Cheap to share: the gateway holds one in an [`Arc`], and the IDLE stream
/// task holds a clone.
pub struct ImapSessionManager {
    /// Connection coordinates. The `secret` field is a placeholder — the
    /// current secret is resolved through `secret_resolver` at every
    /// (re)connect, which is what makes the manager OAuth-rotation-aware: a
    /// rotated XOAUTH2 token is picked up on the next connect, while the
    /// already-authenticated live session keeps working untouched (IMAP
    /// authentication happens once per connection; token expiry does not
    /// invalidate an established session).
    base_config: ImapConnectionConfig,
    secret_resolver: Arc<dyn SecretResolver>,
    slot: tokio::sync::Mutex<SessionSlot>,
    /// Fired by `acquire()` to evict an idling holder: the IDLE hold selects
    /// on this, sends `DONE`, and releases the slot. `notify_one` (permit
    /// semantics) closes the wake-up race where the notification fires before
    /// the IDLE hold reaches its select; a stale permit at worst costs one
    /// spurious DONE/re-IDLE cycle.
    recall: tokio::sync::Notify,
    /// Successful connect count — the observability seam the connection-reuse
    /// and reconnect tests assert on.
    connects: AtomicU64,
}

impl ImapSessionManager {
    pub fn new(
        base_config: ImapConnectionConfig,
        secret_resolver: Arc<dyn SecretResolver>,
    ) -> Arc<Self> {
        Arc::new(Self {
            base_config,
            secret_resolver,
            slot: tokio::sync::Mutex::new(SessionSlot {
                client: None,
                dirty: false,
                epoch: 0,
                idle_examined_at_epoch: None,
                consecutive_connect_failures: 0,
                next_connect_at: None,
            }),
            recall: tokio::sync::Notify::new(),
            connects: AtomicU64::new(0),
        })
    }

    /// Total successful connects since construction. One long-lived session
    /// means this stays at 1 until something drops the connection.
    pub fn connect_count(&self) -> u64 {
        self.connects.load(Ordering::SeqCst)
    }

    /// Check out the account's session for one operation, connecting (with
    /// fresh-secret resolution, per-op deadlines, and reconnect backoff) if
    /// the slot is empty or poisoned. Recalls an in-flight IDLE hold first.
    pub(crate) async fn acquire(
        &self,
        operation: &'static str,
    ) -> Result<SessionLease<'_>, ImapAdapterError> {
        loop {
            // Evict an idling holder; permit semantics cover the race where
            // IDLE has the lock but has not reached its select yet.
            self.recall.notify_one();
            let mut slot = self.slot.lock().await;
            self.sweep_dirty(&mut slot);
            if slot.client.is_none() {
                if let Some(next_connect_at) = slot.next_connect_at {
                    let now = tokio::time::Instant::now();
                    if now < next_connect_at {
                        // Sleep outside the lock so IDLE (or a competing op
                        // whose backoff already elapsed) is not blocked by
                        // our wait.
                        drop(slot);
                        tokio::time::sleep_until(next_connect_at).await;
                        continue;
                    }
                }
                self.connect_into(&mut slot, operation).await?;
            }
            slot.dirty = true;
            slot.epoch = slot.epoch.wrapping_add(1);
            return Ok(SessionLease { slot });
        }
    }

    /// One recallable IDLE hold on the shared session: EXAMINE the watch
    /// mailbox if the selection may have moved, issue IDLE, and wait for
    /// activity / a recall / the re-issue tick — every wait bounded.
    ///
    /// The caller (the spawned IDLE task in [`crate::idle`]) loops around
    /// this; connection loss surfaces as an error here and reconnection is
    /// handled by the next call through the ordinary backoff-gated connect.
    pub(crate) async fn idle_wait(
        &self,
        mailbox_name: &str,
    ) -> Result<IdleWaitOutcome, ImapAdapterError> {
        let mut slot = self.slot.lock().await;
        self.sweep_dirty(&mut slot);
        if slot.client.is_none() {
            if let Some(next_connect_at) = slot.next_connect_at {
                let now = tokio::time::Instant::now();
                if now < next_connect_at {
                    drop(slot);
                    tokio::time::sleep_until(next_connect_at).await;
                    slot = self.slot.lock().await;
                    self.sweep_dirty(&mut slot);
                }
            }
            if slot.client.is_none() {
                self.connect_into(&mut slot, "idle_connect").await?;
            }
        }

        slot.dirty = true;
        let result = self.idle_hold(&mut slot, mailbox_name).await;
        match &result {
            Ok(_) => {
                slot.dirty = false;
            }
            Err(_) => {
                // The wire state is unknowable after a failed hold; force a
                // reconnect on next use rather than resuming a desynced
                // session.
                slot.client = None;
                slot.dirty = false;
                slot.idle_examined_at_epoch = None;
            }
        }
        result
    }

    async fn idle_hold(
        &self,
        slot: &mut SessionSlot,
        mailbox_name: &str,
    ) -> Result<IdleWaitOutcome, ImapAdapterError> {
        // Re-EXAMINE only when an operation (or a reconnect) has touched the
        // session since our last hold — operations SELECT their own
        // mailboxes, which clobbers the IDLE watch selection.
        if slot.idle_examined_at_epoch != Some(slot.epoch) {
            let client = slot.client.as_mut().expect("connected above");
            crate::mailbox::examine_selected_mailbox(client, mailbox_name).await?;
            slot.idle_examined_at_epoch = Some(slot.epoch);
        }

        let client = slot.client.as_mut().expect("connected above");
        let started = tokio::time::Instant::now();
        let tag = client.enqueue_idle();
        tokio::select! {
            // `idle()` internally sends DONE at the client's idle timeout
            // (set to the re-issue interval at connect) and returns Ok
            // once the tagged reply lands — that is the periodic re-issue.
            // The outer timeout is the dead-socket bound: DONE written into a
            // half-open socket never gets its tagged reply (C2).
            result = tokio::time::timeout(idle_max_wait(), client.idle(tag.clone())) => {
                match result {
                    Ok(Ok(())) => {
                        let elapsed = started.elapsed();
                        if elapsed >= idle_reissue_tick_floor() {
                            Ok(IdleWaitOutcome::ReissueTick)
                        } else {
                            Ok(IdleWaitOutcome::Activity {
                                suspect_reject: elapsed < IMAP_IDLE_SHORT_CYCLE,
                            })
                        }
                    }
                    Ok(Err(error)) => Err(ImapAdapterError::Client(format!("{error:?}"))),
                    Err(_elapsed) => Err(ImapAdapterError::Timeout { operation: "idle" }),
                }
            }
            () = self.recall.notified() => {
                // An operation needs the session: wind the IDLE round down
                // (DONE) and hand the slot over. NOT `client.idle_done()` —
                // that helper silently no-ops when the recall lands before
                // the server's IDLE continuation has been processed
                // (`set_idle_done` returns `None` pre-acceptance and is never
                // retried), wedging the handshake. Re-driving `idle()` with a
                // near-zero idle timeout re-attempts DONE on every loop tick,
                // which handles the pre-acceptance window correctly. Bounded
                // — a hung DONE must not wedge the recalling op forever.
                client.state.set_idle_timeout(Duration::from_millis(10));
                let done = tokio::time::timeout(
                    crate::timeout::op_timeout(),
                    client.idle(tag),
                ).await;
                client.state.set_idle_timeout(idle_reissue_interval());
                match done {
                    Ok(Ok(())) => Ok(IdleWaitOutcome::Recalled),
                    Ok(Err(error)) => Err(ImapAdapterError::Client(format!("{error:?}"))),
                    Err(_elapsed) => Err(ImapAdapterError::Timeout { operation: "idle_done" }),
                }
            }
        }
    }

    /// A poisoned slot (holder cancelled mid-protocol) must reconnect.
    fn sweep_dirty(&self, slot: &mut SessionSlot) {
        if slot.dirty {
            ph_warn!(
                events::IMAP_SESSION_POISONED,
                host = %self.base_config.host,
                "previous IMAP session holder was cancelled mid-command; discarding session"
            );
            slot.client = None;
            slot.dirty = false;
            slot.idle_examined_at_epoch = None;
        }
    }

    async fn connect_into(
        &self,
        slot: &mut SessionSlot,
        operation: &'static str,
    ) -> Result<(), ImapAdapterError> {
        // Resolve the secret at connect time — for OAuth this refreshes the
        // short-lived access token, so a token rotated mid-session is used on
        // the very next reconnect without any session-manager bookkeeping.
        let connect_result = async {
            let secret = self
                .secret_resolver
                .resolve_secret()
                .await
                .map_err(|error| ImapAdapterError::Auth(error.to_string()))?;
            let mut config = self.base_config.clone();
            config.secret = secret;
            let mut client = connect_authenticated_client(&config).await?;
            // The IDLE re-issue interval: imap-client sends DONE and returns
            // from `idle()` after this long, which is our re-IDLE cycle.
            client.state.set_idle_timeout(idle_reissue_interval());
            // Post-auth capability refresh, once per connection (capabilities
            // often change after AUTHENTICATE; every former call site did
            // this per-operation).
            with_deadline("refresh_capabilities", client.refresh_capabilities()).await?;
            Ok::<ImapClient, ImapAdapterError>(client)
        }
        .await;

        match connect_result {
            Ok(client) => {
                slot.client = Some(client);
                slot.dirty = false;
                slot.epoch = slot.epoch.wrapping_add(1);
                slot.idle_examined_at_epoch = None;
                slot.consecutive_connect_failures = 0;
                slot.next_connect_at = None;
                let connects = self.connects.fetch_add(1, Ordering::SeqCst) + 1;
                ph_info!(
                    events::IMAP_SESSION_CONNECTED,
                    host = %self.base_config.host,
                    operation,
                    total_connects = connects,
                    "IMAP session (re)connected"
                );
                Ok(())
            }
            Err(error) => {
                let attempt = slot.consecutive_connect_failures;
                slot.consecutive_connect_failures = slot.consecutive_connect_failures.saturating_add(1);
                let delay = reconnect_backoff().delay_for(attempt, jitter_unit());
                slot.next_connect_at = Some(tokio::time::Instant::now() + delay);
                ph_warn!(
                    events::IMAP_SESSION_CONNECT_FAILED,
                    host = %self.base_config.host,
                    operation,
                    consecutive_failures = slot.consecutive_connect_failures,
                    retry_delay_ms = delay.as_millis() as u64,
                    error = %error,
                    "IMAP session connect failed; backing off"
                );
                Err(error)
            }
        }
    }
}

/// The activity/re-issue boundary: a hold that lasted at least this long with
/// `Ok(())` is the quiet DONE/re-IDLE cycle, not server activity. Slightly
/// under the re-issue interval to absorb timer skew.
fn idle_reissue_tick_floor() -> Duration {
    idle_reissue_interval().mul_f64(0.9)
}

/// Full-jitter unit draw for the reconnect schedule. Derived from the
/// system clock's sub-second bits: the call-policy schedule wants an injected
/// `[0, 1)` value, and this avoids pulling an RNG dependency for one number.
fn jitter_unit() -> f64 {
    f64::from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since_epoch| since_epoch.subsec_nanos())
            .unwrap_or(0),
    ) / f64::from(1_000_000_000u32)
}

/// An exclusive checkout of the account's IMAP session.
///
/// Run protocol commands against [`SessionLease::client`], then call
/// [`SessionLease::finish`] (or [`SessionLease::finish_gateway`]) with the
/// result: connection-fatal errors drop the session so the next acquire
/// reconnects, and an orderly finish clears the poison flag. Dropping the
/// lease without finishing (an early return or a cancelled future) leaves the
/// slot dirty on purpose — the wire state is unknown, so the next holder
/// reconnects.
pub(crate) struct SessionLease<'m> {
    slot: tokio::sync::MutexGuard<'m, SessionSlot>,
}

impl SessionLease<'_> {
    pub(crate) fn client(&mut self) -> &mut ImapClient {
        self.slot
            .client
            .as_mut()
            .expect("a lease always holds a connected session")
    }

    /// Classify an adapter-level result: transport-fatal errors poison the
    /// session (reconnect on next use); logical errors keep it.
    pub(crate) fn finish<T>(
        mut self,
        result: Result<T, ImapAdapterError>,
    ) -> Result<T, ImapAdapterError> {
        self.slot.dirty = false;
        if let Err(error) = &result {
            if imap_error_is_connection_fatal(error) {
                self.drop_session();
            }
        }
        result
    }

    /// [`SessionLease::finish`] for results that were already mapped into
    /// [`GatewayError`] deep inside the sync pipeline.
    pub(crate) fn finish_gateway<T>(
        mut self,
        result: Result<T, GatewayError>,
    ) -> Result<T, GatewayError> {
        self.slot.dirty = false;
        if let Err(error) = &result {
            if gateway_error_is_connection_fatal(error) {
                self.drop_session();
            }
        }
        result
    }

    fn drop_session(&mut self) {
        ph_debug!(
            events::IMAP_SESSION_DROPPED,
            "IMAP session dropped after a connection-fatal error; next use reconnects"
        );
        self.slot.client = None;
        self.slot.idle_examined_at_epoch = None;
    }
}

/// Transport-level failures mean the session's wire state is unusable; logical
/// failures (bad mailbox name, UIDVALIDITY break, parse errors, …) leave a
/// perfectly good session behind.
fn imap_error_is_connection_fatal(error: &ImapAdapterError) -> bool {
    matches!(
        error,
        ImapAdapterError::Timeout { .. } | ImapAdapterError::Client(_) | ImapAdapterError::Auth(_)
    )
}

/// The gateway mapping folds transport errors into `Network` (and auth into
/// `Auth`); everything else is logical.
fn gateway_error_is_connection_fatal(error: &GatewayError) -> bool {
    matches!(error, GatewayError::Network(_) | GatewayError::Auth)
}

#[cfg(test)]
mod tests;
