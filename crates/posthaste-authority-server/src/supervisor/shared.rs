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

    /// Update the running sync progress, setting account status to Syncing while present.
    pub(crate) async fn set_sync_progress(
        &self,
        account_id: &AccountId,
        generation: RuntimeGeneration,
        progress: Option<SyncProgress>,
    ) {
        self.update_runtime_overview(account_id, Some(generation), move |current| {
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
        self.update_runtime_overview(account_id, Some(generation), move |current| {
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
        self.update_runtime_overview(account_id, Some(generation), |current| {
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
        self.update_runtime_overview(account_id, Some(generation), |current| {
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
        self.update_runtime_overview(account_id, None, |current| {
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
        self.update_runtime_overview(account_id, None, |current| {
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
        self.update_runtime_overview(account_id, Some(generation), |current| {
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

    /// Persist a runtime overview and emit status/push change events when transitions occur.
    ///
    /// @spec docs/L1-sync#event-propagation
    pub(crate) async fn set_runtime_overview(
        &self,
        account_id: &AccountId,
        overview: AccountRuntimeOverview,
    ) {
        self.update_runtime_overview(account_id, None, move |current| {
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
        self.update_runtime_overview(account_id, Some(generation), move |current| {
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
    async fn update_runtime_overview(
        &self,
        account_id: &AccountId,
        generation: Option<RuntimeGeneration>,
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
        drop(generations);
        self.publish_events(&side_effects);
    }
}
