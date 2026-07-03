use super::*;

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

impl SupervisorShared {
    pub(crate) async fn gateway(
        &self,
        account_id: &AccountId,
    ) -> Result<SharedGateway, ServiceError> {
        self.gateways
            .read()
            .await
            .get(account_id.as_str())
            .cloned()
            .ok_or_else(|| GatewayError::Unavailable(account_id.to_string()).into())
    }

    pub(crate) async fn set_gateway(&self, account_id: &AccountId, gateway: SharedGateway) {
        self.gateways
            .write()
            .await
            .insert(account_id.to_string(), gateway);
    }

    pub(crate) async fn remove_gateway(&self, account_id: &AccountId) {
        self.gateways.write().await.remove(account_id.as_str());
    }

    /// A random startup-sync splay in `[0, sync_governor.startup_splay_max)`
    /// (D98(a) / Sc1). Returns `Duration::ZERO` when the splay window is zero.
    /// Uses the same dependency-free "Review"-grade jitter as the watchdog
    /// backoff — each account runtime starts at a slightly different instant, so
    /// the `SystemTime`-nanos draws differ enough to decorrelate the boot herd.
    pub(crate) fn startup_splay_delay(&self) -> Duration {
        let max = self.sync_governor.startup_splay_max;
        if max.is_zero() {
            return Duration::ZERO;
        }
        max.mul_f64(jitter_unit())
    }

    /// Acquire one global concurrent-sync slot (D98(b) / R4 / O7). The returned
    /// permit is held for the whole sync cycle, so at most
    /// [`GLOBAL_CONCURRENT_SYNC_LIMIT`] provider syncs run at once across every
    /// account. This governor is DISTINCT from `cache_resources`
    /// ([`CacheResourceGovernor`](posthaste_domain_service::CacheResourceGovernor)),
    /// which throttles cache fetches only.
    pub(crate) async fn acquire_sync_slot(&self) -> tokio::sync::OwnedSemaphorePermit {
        self.sync_governor
            .slots
            .clone()
            .acquire_owned()
            .await
            .expect("the sync governor semaphore is never closed")
    }

    pub(crate) async fn register_account(&self, account_id: &AccountId) {
        let count = {
            let mut known_accounts = self.known_accounts.write().await;
            known_accounts.insert(account_id.to_string());
            known_accounts.len()
        };
        self.account_count.store(count, Ordering::SeqCst);
    }

    pub(crate) async fn unregister_account(&self, account_id: &AccountId) {
        let count = {
            let mut known_accounts = self.known_accounts.write().await;
            known_accounts.remove(account_id.as_str());
            known_accounts.len()
        };
        self.account_count.store(count, Ordering::SeqCst);
    }

    pub(crate) async fn next_runtime_generation(
        &self,
        account_id: &AccountId,
    ) -> RuntimeGeneration {
        let mut generations = self.runtime_generations.write().await;
        let generation = generations
            .get(account_id.as_str())
            .copied()
            .unwrap_or(RuntimeGeneration::INITIAL)
            .next();
        generations.insert(account_id.to_string(), generation);
        generation
    }

    /// Mints a fresh [`SyncCycleGeneration`] for `account_id` and registers it
    /// as the account's current cycle, invalidating whatever token the
    /// previous cycle (if any) handed to its progress forwarder (N5 + the M26
    /// flag / M27 sub-unit (d)).
    ///
    /// Called twice per relevant occasion: once at the start of every sync
    /// cycle (`sync_flow::process_sync_trigger_inner`), and again — with the
    /// return value discarded — whenever a select!-loop arm abandons a cycle
    /// (`runtime::record_arm_timeout`), so a progress write still in flight
    /// for the abandoned cycle no longer matches and is rejected by
    /// [`Self::set_sync_progress`]'s generation check.
    pub(crate) async fn next_sync_cycle_generation(
        &self,
        account_id: &AccountId,
    ) -> SyncCycleGeneration {
        let mut cycles = self.sync_cycle_generations.write().await;
        let cycle = cycles
            .get(account_id.as_str())
            .copied()
            .unwrap_or(SyncCycleGeneration::INITIAL)
            .next();
        cycles.insert(account_id.to_string(), cycle);
        cycle
    }

    /// Broadcast domain events to all SSE subscribers.
    pub(crate) fn publish_events(&self, events: &[DomainEvent]) {
        for event in events {
            let _ = self.event_sender.send(event.clone());
        }
    }

    /// Read the cached runtime overview for an account, defaulting to empty.
    pub(crate) async fn runtime_overview(&self, account_id: &AccountId) -> AccountRuntimeOverview {
        self.runtime_overviews
            .read()
            .await
            .get(account_id.as_str())
            .cloned()
            .unwrap_or_default()
    }

    /// Update the running sync progress, setting account status to Syncing
    /// while present.
    ///
    /// `cycle` gates this write against [`Self::next_sync_cycle_generation`]'s
    /// per-account current token (N5 + the M26 flag / M27 sub-unit (d)): a
    /// write minted for a cycle that has since been abandoned (a select!-loop
    /// arm timeout) or superseded (a fresh cycle already started) is dropped,
    /// on top of — not instead of — the existing `RuntimeGeneration` guard,
    /// which only catches a whole-incarnation restart, not an abandoned cycle
    /// within the same incarnation.
    pub(crate) async fn set_sync_progress(
        &self,
        account_id: &AccountId,
        generation: RuntimeGeneration,
        cycle: SyncCycleGeneration,
        progress: Option<SyncProgress>,
    ) {
        self.update_runtime_overview(account_id, Some(generation), Some(cycle), move |current| {
            match progress {
                Some(progress) => {
                    // Guarded against the committed overview under the lock: a
                    // late sync-progress write arriving after the sync settled
                    // (status no longer Syncing) is dropped, so it cannot revive
                    // a stale Syncing over a terminal Ready/failed status.
                    if !matches!(progress.stage, SyncProgressStage::Connecting)
                        && !matches!(current.status, AccountStatus::Syncing)
                    {
                        return false;
                    }
                    current.sync_progress = Some(progress);
                    current.status = AccountStatus::Syncing;
                }
                None => {
                    current.sync_progress = None;
                }
            }
            true
        })
        .await;
    }

    /// Update only the push status, preserving other overview fields.
    pub(crate) async fn set_push_status(
        &self,
        account_id: &AccountId,
        generation: RuntimeGeneration,
        push: PushStatus,
    ) {
        self.update_runtime_overview(account_id, Some(generation), None, move |current| {
            current.push = push;
            true
        })
        .await;
    }

    /// Record a successful sync: set status to Ready, clear error, update timestamp.
    pub(crate) async fn mark_sync_success(
        &self,
        account_id: &AccountId,
        generation: RuntimeGeneration,
    ) {
        self.update_runtime_overview(account_id, Some(generation), None, |current| {
            current.status = AccountStatus::Ready;
            current.last_sync_at = time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .ok();
            current.last_sync_error = None;
            current.last_sync_error_code = None;
            current.sync_progress = None;
            if matches!(current.push, PushStatus::Disabled) {
                current.push = PushStatus::Reconnecting;
            }
            true
        })
        .await;
    }

    /// Record a sync failure: derive status from error type, store error details.
    pub(crate) async fn mark_sync_failure(
        &self,
        account_id: &AccountId,
        generation: RuntimeGeneration,
        error: &ServiceError,
    ) {
        self.update_runtime_overview(account_id, Some(generation), None, |current| {
            current.status = match error {
                ServiceError::Gateway(GatewayError::Auth) => AccountStatus::AuthError,
                ServiceError::Gateway(GatewayError::Network(_))
                | ServiceError::Gateway(GatewayError::Unavailable(_))
                | ServiceError::Secret(_) => AccountStatus::Offline,
                _ => AccountStatus::Degraded,
            };
            current.last_sync_error = Some(error.to_string());
            current.last_sync_error_code = Some(error.code().to_string());
            current.sync_progress = None;
            if !matches!(current.push, PushStatus::Unsupported | PushStatus::Disabled) {
                current.push = PushStatus::Reconnecting;
            }
            true
        })
        .await;
    }

    /// Flip the account to `AuthError` directly from the OAuth refresh tick
    /// (A2 / D102). A proactive refresh that fails `invalid_grant` /
    /// `unauthorized_client` — classified `GatewayError::Auth`, a Permanent
    /// [`Terminality`](posthaste_domain_model::Terminality) — means the grant is
    /// revoked or the refresh token consumed; no retry or reconnect recovers it.
    /// Surfacing it here keeps status truthful *immediately* (XIII) rather than
    /// swallowing the error as a warning and waiting for a later connection
    /// rebuild to observe the failing resolve. Uses the same `auth_error` status
    /// vocabulary [`mark_sync_failure`](Self::mark_sync_failure) derives for a
    /// `GatewayError::Auth`, and is generation-guarded like the other status
    /// writers so a stale incarnation cannot overwrite a live one.
    pub(crate) async fn mark_account_auth_error(
        &self,
        account_id: &AccountId,
        generation: RuntimeGeneration,
        reason: &str,
    ) {
        self.update_runtime_overview(account_id, Some(generation), None, |current| {
            current.status = AccountStatus::AuthError;
            current.last_sync_error = Some(format!("OAuth token refresh failed: {reason}"));
            current.last_sync_error_code = Some(ServiceErrorKind::AuthError.code().to_string());
            current.sync_progress = None;
            if !matches!(current.push, PushStatus::Unsupported | PushStatus::Disabled) {
                current.push = PushStatus::Reconnecting;
            }
            true
        })
        .await;
    }

    /// Mark an account impaired by an internal runtime fault (a panic or an
    /// unexpected exit) that the watchdog is about to retry. `Degraded` is the
    /// truthful state (D61 / XIII): the account is malfunctioning, but the cause
    /// is neither an auth failure (`AuthError`) nor a network/secret failure
    /// (`Offline`) — the two classes `mark_sync_failure` already distinguishes.
    /// Written unconditionally (no generation guard): the faulted incarnation is
    /// already dead, and the next incarnation's own startup writes supersede this.
    pub(crate) async fn mark_account_faulted(
        &self,
        account_id: &AccountId,
        attempt: u32,
        reason: &str,
    ) {
        self.update_runtime_overview(account_id, None, None, |current| {
            current.status = AccountStatus::Degraded;
            current.last_sync_error = Some(format!(
                "account runtime fault (restart {attempt}/{WATCHDOG_MAX_RESTARTS}): {reason}"
            ));
            current.last_sync_error_code = Some("runtime_fault".to_string());
            current.sync_progress = None;
            if !matches!(current.push, PushStatus::Unsupported | PushStatus::Disabled) {
                current.push = PushStatus::Reconnecting;
            }
            true
        })
        .await;
    }

    /// Mark an account halted after the watchdog exhausted its restart budget. The
    /// runtime is no longer running, so `Offline` is the truthful state — it will
    /// not serve until an operator restarts it (D61 / XIII). Push is `Disabled`
    /// because nothing is left to reconnect it.
    pub(crate) async fn mark_account_halted(&self, account_id: &AccountId, reason: &str) {
        self.update_runtime_overview(account_id, None, None, |current| {
            current.status = AccountStatus::Offline;
            current.push = PushStatus::Disabled;
            current.last_sync_error = Some(format!(
                "account runtime halted after {WATCHDOG_MAX_RESTARTS} failed restarts: {reason}"
            ));
            current.last_sync_error_code = Some("runtime_halted".to_string());
            current.sync_progress = None;
            true
        })
        .await;
    }

    /// Record that a supervisor select!-loop arm's bounded call
    /// (`tokio::time::timeout`, RFC-L2-lifecycle D66 / M26) elapsed before
    /// completing. Marks the account `Degraded` — distinct from
    /// `mark_sync_failure`'s auth/network-derived classes, since the
    /// underlying operation may simply be hung rather than definitively
    /// rejected or offline — and otherwise leaves the loop running: the M21
    /// watchdog owns account lifecycle, not this per-arm backstop, so a
    /// timeout here never breaks the caller's loop.
    pub(crate) async fn mark_arm_timeout(
        &self,
        account_id: &AccountId,
        generation: RuntimeGeneration,
        arm: &'static str,
        budget: Duration,
    ) {
        self.update_runtime_overview(account_id, Some(generation), None, |current| {
            current.status = AccountStatus::Degraded;
            current.last_sync_error = Some(format!(
                "supervisor arm '{arm}' exceeded its {budget:?} budget (provider/store call hung)"
            ));
            current.last_sync_error_code = Some("arm_timeout".to_string());
            current.sync_progress = None;
            true
        })
        .await;
    }

    /// Snooze due-comparison clock (RFC-L2-lifecycle row 10 rider / D66).
    ///
    /// `until` in `message_snooze` is a wall-clock UNIX-epoch-seconds column
    /// set by the API when a user picks a return time, so the due-check
    /// genuinely needs wall-clock semantics — but sampling
    /// `SystemTime::now()` fresh on every tick is not resilient to a
    /// backward NTP correction: a jump back would make "now" look earlier
    /// than it truly is, which either strands an actually-due snooze past
    /// its return time (starving it — the RFC's named failure mode) or, on a
    /// boundary case, could let an event that already fired be re-evaluated.
    ///
    /// This anchors one wall-clock sample (first call, process-lifetime
    /// `OnceLock`) against `Instant::now()` taken at the same moment, then
    /// advances that anchor by `Instant::now() - anchor_instant` on every
    /// later call. `Instant` is documented monotonic — it never observes an
    /// OS clock correction — so the computed "now" can only advance: a
    /// snooze that is due now stays due on every later call (no starving),
    /// and the value never regresses to re-open an already-passed boundary
    /// (no double-firing), for the lifetime of this process. A restart
    /// re-anchors from the `SystemTime` sampled at that moment, so this does
    /// not (and cannot, from a single process) guard a jump that happens
    /// *before* the anchor is first taken — only jumps during the anchored
    /// process's run are covered, which is the realistic operational risk
    /// (an NTP drift correction on a long-lived process), not initial boot
    /// clock skew.
    pub(crate) fn monotonic_now_secs() -> i64 {
        static ANCHOR: OnceLock<(Instant, SystemTime)> = OnceLock::new();
        let &(anchor_instant, anchor_wall) =
            ANCHOR.get_or_init(|| (Instant::now(), SystemTime::now()));
        let elapsed = Instant::now().saturating_duration_since(anchor_instant);
        Self::anchored_now_secs(anchor_wall, elapsed)
    }

    /// Pure core of [`Self::monotonic_now_secs`], split out so a test can
    /// drive the anchor/elapsed pair directly instead of manipulating the
    /// real system clock (principle II: one declared seam a test can reach).
    pub(crate) fn anchored_now_secs(anchor_wall: SystemTime, elapsed: Duration) -> i64 {
        (anchor_wall + elapsed)
            .duration_since(UNIX_EPOCH)
            .map(|delta| delta.as_secs() as i64)
            .unwrap_or(0)
    }

    /// Handle a push stream disconnect: emit event and set push status to Reconnecting.
    pub(crate) async fn handle_push_disconnect(
        &self,
        account_id: &AccountId,
        generation: RuntimeGeneration,
        message: &str,
    ) {
        match self.store.append_event(
            account_id,
            EVENT_TOPIC_PUSH_DISCONNECTED,
            None,
            None,
            json!({ "message": message }),
        ) {
            Ok(event) => self.publish_events(&[event]),
            Err(error) => ph_warn!(
                events::SUPERVISOR_ACCOUNT_STATUS_PERSIST_FAILED,
                account_id = %account_id,
                topic = EVENT_TOPIC_PUSH_DISCONNECTED,
                error = %error,
                "failed to persist push disconnect event"
            ),
        }
        self.set_push_status(account_id, generation, PushStatus::Reconnecting)
            .await;
    }

    /// Mark push terminally unavailable for this account (PP6/D91): a
    /// structurally-broken transport (e.g. a 404 eventsource URL) that repeated
    /// reconnects cannot fix. Push status → `Unsupported` — the truthful terminal
    /// state, not `Reconnecting`, which would lie that recovery is imminent
    /// (XIII) — carrying the reason and an explicit "polling instead" detail. The
    /// account's *sync* status is deliberately left untouched: the 60 s
    /// safety-net poll keeps mail fresh, so the account is degraded in realtime
    /// only, not offline, and the sync path owns `AccountStatus`.
    pub(crate) async fn mark_push_terminal(
        &self,
        account_id: &AccountId,
        generation: RuntimeGeneration,
        transport: &str,
        reason: &str,
    ) {
        let poll_secs = self.poll_interval.as_secs();
        self.update_runtime_overview(account_id, Some(generation), None, move |current| {
            if current.push == PushStatus::Unsupported {
                return false;
            }
            current.push = PushStatus::Unsupported;
            current.last_sync_error = Some(format!(
                "push unavailable via {transport}: {reason}; polling every {poll_secs}s instead"
            ));
            current.last_sync_error_code = Some("push_terminal".to_string());
            true
        })
        .await;
    }

    /// Persist a runtime overview and emit status/push change events when transitions occur.
    ///
    /// @spec docs/L1-sync#event-propagation
    pub(crate) async fn set_runtime_overview(
        &self,
        account_id: &AccountId,
        overview: AccountRuntimeOverview,
    ) {
        self.update_runtime_overview(account_id, None, None, move |current| {
            *current = overview;
            true
        })
        .await;
    }

    pub(crate) async fn set_runtime_overview_for_generation(
        &self,
        account_id: &AccountId,
        generation: RuntimeGeneration,
        overview: AccountRuntimeOverview,
    ) {
        self.update_runtime_overview(account_id, Some(generation), None, move |current| {
            *current = overview;
            true
        })
        .await;
    }

    /// Atomically read-modify-write an account's runtime overview under the
    /// overviews write lock. The `update` closure receives the current overview
    /// (default if absent) and returns whether to commit; it runs while the
    /// lock is held, so any guard inside it sees committed state. This prevents
    /// a stale read from clobbering a concurrent write — e.g. a late spawned
    /// sync-progress update reviving `Syncing` after `mark_sync_success` settled
    /// `Ready`, which left accounts wedged in "syncing" until a manual sync.
    /// Emits status/push change events on commit.
    ///
    /// @spec docs/L1-sync#event-propagation
    ///
    /// `cycle`, when `Some`, additionally gates the write against
    /// [`Self::next_sync_cycle_generation`]'s current per-account token
    /// (N5 + the M26 flag / M27 sub-unit (d) — see [`SyncCycleGeneration`]'s
    /// doc comment for why `generation` alone does not close this gap). The
    /// `sync_cycle_generations` read lock is held for this whole call — from
    /// the check through the `overviews` commit below, mirroring how
    /// `generations` is already held — so a concurrent
    /// `next_sync_cycle_generation` bump (which needs the matching write
    /// lock) is always fully ordered before or after this write, never
    /// interleaved with it: tokio's `RwLock` is FIFO-fair, so a bump that
    /// arrives while this call already holds its read lock will queue behind
    /// it rather than jump ahead, guaranteeing that a bump issued by
    /// `record_arm_timeout` is visible to every progress write that starts
    /// after it returns.
    async fn update_runtime_overview(
        &self,
        account_id: &AccountId,
        generation: Option<RuntimeGeneration>,
        cycle: Option<SyncCycleGeneration>,
        update: impl FnOnce(&mut AccountRuntimeOverview) -> bool,
    ) {
        let generations = self.runtime_generations.read().await;
        if let Some(expected) = generation {
            let Some(current) = generations.get(account_id.as_str()) else {
                return;
            };
            if *current != expected {
                return;
            }
        }

        let cycles = self.sync_cycle_generations.read().await;
        if let Some(expected) = cycle {
            let current_cycle = cycles
                .get(account_id.as_str())
                .copied()
                .unwrap_or(SyncCycleGeneration::INITIAL);
            if current_cycle != expected {
                return;
            }
        }

        let mut overviews = self.runtime_overviews.write().await;
        let previous = overviews.get(account_id.as_str()).cloned();
        let mut overview = previous.clone().unwrap_or_default();
        if !update(&mut overview) {
            return;
        }

        let mut side_effects = Vec::new();
        if previous.as_ref().map(|item| &item.status) != Some(&overview.status)
            || previous.as_ref().map(|item| &item.push) != Some(&overview.push)
            || previous.as_ref().map(|item| &item.sync_progress) != Some(&overview.sync_progress)
            || previous.as_ref().map(|item| &item.last_sync_error_code)
                != Some(&overview.last_sync_error_code)
        {
            match self.store.append_event(
                account_id,
                EVENT_TOPIC_ACCOUNT_STATUS_CHANGED,
                None,
                None,
                json!({
                    "status": &overview.status,
                    "push": &overview.push,
                    "lastSyncAt": overview.last_sync_at,
                    "lastSyncError": overview.last_sync_error,
                    "lastSyncErrorCode": overview.last_sync_error_code,
                    "syncProgress": overview.sync_progress,
                }),
            ) {
                Ok(event) => side_effects.push(event),
                Err(error) => {
                    ph_warn!(
                        events::SUPERVISOR_ACCOUNT_STATUS_PERSIST_FAILED,
                        account_id = %account_id,
                        topic = EVENT_TOPIC_ACCOUNT_STATUS_CHANGED,
                        error = %error,
                        "failed to persist account status change event"
                    );
                    return;
                }
            }
        }

        match (previous.as_ref().map(|item| &item.push), &overview.push) {
            (Some(PushStatus::Connected), PushStatus::Connected) => {}
            (_, PushStatus::Connected) => match self.store.append_event(
                account_id,
                EVENT_TOPIC_PUSH_CONNECTED,
                None,
                None,
                json!({}),
            ) {
                Ok(event) => side_effects.push(event),
                Err(error) => ph_warn!(
                    events::SUPERVISOR_ACCOUNT_STATUS_PERSIST_FAILED,
                    account_id = %account_id,
                    topic = EVENT_TOPIC_PUSH_CONNECTED,
                    error = %error,
                    "failed to persist push connected event"
                ),
            },
            (Some(PushStatus::Connected), _) => match self.store.append_event(
                account_id,
                EVENT_TOPIC_PUSH_DISCONNECTED,
                None,
                None,
                json!({}),
            ) {
                Ok(event) => side_effects.push(event),
                Err(error) => ph_warn!(
                    events::SUPERVISOR_ACCOUNT_STATUS_PERSIST_FAILED,
                    account_id = %account_id,
                    topic = EVENT_TOPIC_PUSH_DISCONNECTED,
                    error = %error,
                    "failed to persist push disconnected event"
                ),
            },
            _ => {}
        }

        overviews.insert(account_id.to_string(), overview);
        drop(overviews);
        drop(cycles);
        drop(generations);
        self.publish_events(&side_effects);
    }
}
