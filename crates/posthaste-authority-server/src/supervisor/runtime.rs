use super::*;

/// Main event loop for an account: polls on timer, push notifications, and
/// manual sync commands. Runs until the task is aborted.
///
/// @spec docs/L1-sync#sync-loop
pub(crate) async fn run_account_runtime(
    shared: Arc<SupervisorShared>,
    account: AccountSettings,
    generation: RuntimeGeneration,
    mut command_rx: mpsc::Receiver<RuntimeCommand>,
    sync_state: Arc<SyncTriggerState>,
    cancel: CancellationToken,
) {
    let account_id = account.id.clone();
    let mut connection = AccountRuntimeConnectionState::default();
    let mut oauth_refresh_state = OAuthRefreshState::new(&account);
    let mut oauth_refresh_interval = oauth_refresh_state.interval();
    let mut backfill_interval = tokio::time::interval_at(
        tokio::time::Instant::now() + AUTOMATION_BACKFILL_INITIAL_DELAY,
        AUTOMATION_BACKFILL_INTERVAL,
    );
    backfill_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut cache_interval = tokio::time::interval_at(
        tokio::time::Instant::now() + CACHE_WORKER_INITIAL_DELAY,
        CACHE_WORKER_INTERVAL,
    );
    cache_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut snooze_interval = tokio::time::interval_at(
        tokio::time::Instant::now() + SNOOZE_INITIAL_DELAY,
        SNOOZE_INTERVAL,
    );
    snooze_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    shared
        .set_runtime_overview_for_generation(
            &account_id,
            generation,
            AccountRuntimeOverview {
                status: AccountStatus::Offline,
                push: PushStatus::Reconnecting,
                ..Default::default()
            },
        )
        .await;

    // Initial sync + gateway setup. Bounded the same as an in-loop sync arm
    // (D66): a hung provider here would otherwise wedge the task before the
    // select! loop — and therefore this account's command/push handling —
    // ever starts.
    if tokio::time::timeout(
        ARM_BUDGET_SYNC,
        process_sync_trigger_with_state(
            &sync_state,
            &shared,
            &account,
            generation,
            SyncTriggerRequest::new(SyncTrigger::Startup, SyncMode::Incremental),
            &mut connection,
        ),
    )
    .await
    .is_err()
    {
        record_arm_timeout(
            &shared,
            &account_id,
            generation,
            "startup_sync",
            ARM_BUDGET_SYNC,
        )
        .await;
    }
    let mut interval = sync_poll_interval(shared.poll_interval);

    loop {
        let next_push = async {
            match connection.push_events_mut() {
                Some(stream) => stream.next().await,
                None => pending().await,
            }
        };

        tokio::select! {
            // Cooperative stop (D61): the supervisor cancels this account's token
            // to signal a graceful loop exit. In-flight work at the current await
            // point is dropped — store writes are transactional, so a dropped
            // cycle simply re-runs on the next start (no per-operation draining).
            () = cancel.cancelled() => {
                ph_info!(
                    events::SUPERVISOR_ACCOUNT_RUNTIME_STOPPED,
                    account_id = %account_id,
                    "account runtime cancelled; exiting loop"
                );
                break;
            }
            // D66: every inline await below is bounded by `tokio::time::timeout`
            // (a BACKSTOP over each provider call's own tighter "envelope"
            // deadline — see the ARM_BUDGET_* doc comments in types.rs). A
            // timeout logs, degrades the account, and falls through to the next
            // loop iteration — it never `break`s: the M21 watchdog owns
            // lifecycle, not this per-arm guard.
            _ = interval.tick() => {
                if tokio::time::timeout(
                    ARM_BUDGET_SYNC,
                    handle_poll_tick(&sync_state, &shared, &account, generation, &mut connection),
                ).await.is_err() {
                    record_arm_timeout(&shared, &account_id, generation, "poll_sync", ARM_BUDGET_SYNC).await;
                }
                interval = sync_poll_interval(shared.poll_interval);
            }
            _ = backfill_interval.tick() => {
                if tokio::time::timeout(
                    ARM_BUDGET_BACKFILL,
                    handle_backfill_tick(&shared, &account_id, connection.gateway()),
                ).await.is_err() {
                    record_arm_timeout(&shared, &account_id, generation, "backfill", ARM_BUDGET_BACKFILL).await;
                }
            }
            _ = cache_interval.tick() => {
                if tokio::time::timeout(
                    ARM_BUDGET_CACHE,
                    handle_cache_tick(
                        &shared,
                        &account_id,
                        connection.gateway(),
                        CACHE_BACKGROUND_PRESSURE,
                        None,
                    ),
                ).await.is_err() {
                    record_arm_timeout(&shared, &account_id, generation, "cache_maintenance", ARM_BUDGET_CACHE).await;
                }
            }
            _ = snooze_interval.tick() => {
                if tokio::time::timeout(
                    ARM_BUDGET_SNOOZE,
                    handle_snooze_tick(&shared, &account_id),
                ).await.is_err() {
                    record_arm_timeout(&shared, &account_id, generation, "snooze", ARM_BUDGET_SNOOZE).await;
                }
            }
            _ = async {
                match oauth_refresh_interval.as_mut() {
                    Some(interval) => interval.tick().await,
                    None => std::future::pending().await,
                }
            } => {
                if tokio::time::timeout(
                    ARM_BUDGET_OAUTH_REFRESH,
                    handle_oauth_refresh_tick(
                        &shared,
                        &account,
                        generation,
                        &account_id,
                        &mut connection,
                        &mut oauth_refresh_state,
                    ),
                ).await.is_err() {
                    record_arm_timeout(&shared, &account_id, generation, "oauth_refresh", ARM_BUDGET_OAUTH_REFRESH).await;
                }
            }
            Some(command) = command_rx.recv() => {
                match tokio::time::timeout(
                    ARM_BUDGET_SYNC,
                    handle_runtime_command(
                        &sync_state,
                        &shared,
                        &account,
                        &account_id,
                        &mut connection,
                        generation,
                        command,
                    ),
                ).await {
                    Ok(true) => interval = sync_poll_interval(shared.poll_interval),
                    Ok(false) => {}
                    Err(_) => record_arm_timeout(&shared, &account_id, generation, "command", ARM_BUDGET_SYNC).await,
                }
            }
            Some(event) = next_push => {
                match tokio::time::timeout(
                    ARM_BUDGET_SYNC,
                    handle_push_event(&sync_state, &shared, &account, &account_id, generation, &mut connection, event),
                ).await {
                    Ok(true) => interval = sync_poll_interval(shared.poll_interval),
                    Ok(false) => {}
                    Err(_) => record_arm_timeout(&shared, &account_id, generation, "push_event", ARM_BUDGET_SYNC).await,
                }
            }
        }
    }
}

/// Logs and marks the account `Degraded` when a select!-loop arm's bounded
/// call (`tokio::time::timeout`, D66) elapses. Called at every wrapped
/// call-site above; never breaks the caller's loop.
async fn record_arm_timeout(
    shared: &Arc<SupervisorShared>,
    account_id: &AccountId,
    generation: RuntimeGeneration,
    arm: &'static str,
    budget: Duration,
) {
    ph_warn!(
        events::SUPERVISOR_ARM_TIMEOUT,
        account_id = %account_id,
        arm,
        budget_ms = budget.as_millis() as u64,
        "supervisor select-loop arm exceeded its bounded budget; account degraded, loop continues"
    );
    shared
        .mark_arm_timeout(account_id, generation, arm, budget)
        .await;
}

/// A single sync request bundled to avoid a multi-argument explosion in
/// [`process_sync_trigger_with_state`].
#[derive(Debug)]
pub(crate) struct SyncTriggerRequest {
    pub(crate) trigger: SyncTrigger,
    pub(crate) mode: SyncMode,
    pub(crate) reply: Option<oneshot::Sender<Result<usize, ServiceError>>>,
}

impl SyncTriggerRequest {
    pub(crate) fn new(trigger: SyncTrigger, mode: SyncMode) -> Self {
        Self {
            trigger,
            mode,
            reply: None,
        }
    }

    pub(crate) fn with_reply(
        trigger: SyncTrigger,
        mode: SyncMode,
        reply: oneshot::Sender<Result<usize, ServiceError>>,
    ) -> Self {
        Self {
            trigger,
            mode,
            reply: Some(reply),
        }
    }
}

/// Run a single sync cycle, keeping [`SyncTriggerState`] informed so that
/// fire-and-forget triggers can be coalesced. After the requested cycle
/// finishes, any trigger that was coalesced into `pending` while it was running
/// is drained by running exactly one follow-up cycle.
pub(crate) async fn process_sync_trigger_with_state(
    sync_state: &Arc<SyncTriggerState>,
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    generation: RuntimeGeneration,
    request: SyncTriggerRequest,
    connection: &mut AccountRuntimeConnectionState,
) {
    let mut next = Some(request);
    while let Some(request) = next {
        sync_state.begin_cycle().await;
        let _ = process_sync_trigger(
            shared,
            account,
            generation,
            request.trigger,
            request.mode,
            connection,
            request.reply,
        )
        .await;
        // Finish + take-pending is atomic with the trigger source's
        // coalesce-or-claim, so a trigger coalesced while this cycle ran is
        // always drained here rather than stranded.
        next = sync_state
            .finish_cycle_take_pending()
            .await
            .map(|trigger| SyncTriggerRequest::new(trigger, SyncMode::Incremental));
    }
}

pub(crate) async fn handle_poll_tick(
    sync_state: &Arc<SyncTriggerState>,
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    generation: RuntimeGeneration,
    connection: &mut AccountRuntimeConnectionState,
) {
    let _ = process_sync_trigger_with_state(
        sync_state,
        shared,
        account,
        generation,
        SyncTriggerRequest::new(SyncTrigger::Poll, SyncMode::Incremental),
        connection,
    )
    .await;
}

pub(crate) async fn handle_backfill_tick(
    shared: &Arc<SupervisorShared>,
    account_id: &AccountId,
    gateway: Option<SharedGateway>,
) {
    let _ = process_automation_backfill_batch(shared, account_id, gateway).await;
}

pub(crate) async fn handle_cache_tick(
    shared: &Arc<SupervisorShared>,
    account_id: &AccountId,
    gateway: Option<SharedGateway>,
    interactive_pressure: f64,
    operation_id: Option<&str>,
) {
    process_cache_maintenance_batch(
        shared,
        account_id,
        gateway,
        interactive_pressure,
        operation_id,
    )
    .await;
}

/// Snooze scheduler tick: return every due snoozed message to the Inbox. The
/// move reuses the client path's `replace_mailboxes` write-through, so the
/// provider move is enqueued (flushed on the next sync) + the store invariant
/// clears the snooze row immediately. Not user-initiated → no undo step.
///
/// The due-comparison clock is monotonic-anchored, not a raw
/// `SystemTime::now()` sample (RFC-L2-lifecycle row 10 rider / D66,
/// [`SupervisorShared::monotonic_now_secs`]): a backward NTP correction
/// cannot make an already-due snooze look not-yet-due.
///
/// @spec docs/eph/DESIGN-L2-snooze
pub(crate) async fn handle_snooze_tick(shared: &Arc<SupervisorShared>, account_id: &AccountId) {
    let now = SupervisorShared::monotonic_now_secs();
    match shared
        .service
        .auto_return_snoozed_messages(account_id, now)
        .await
    {
        Ok(count) if count > 0 => {
            ph_debug!(
                events::SUPERVISOR_SNOOZE_AUTO_RETURNED,
                account_id = %account_id,
                count,
                "snooze scheduler returned messages to the inbox"
            );
        }
        _ => {}
    }
}

pub(crate) async fn handle_runtime_command(
    sync_state: &Arc<SyncTriggerState>,
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    account_id: &AccountId,
    connection: &mut AccountRuntimeConnectionState,
    generation: RuntimeGeneration,
    command: RuntimeCommand,
) -> bool {
    match command {
        RuntimeCommand::Trigger {
            trigger,
            mode,
            reply,
        } => {
            let _ = process_sync_trigger_with_state(
                sync_state,
                shared,
                account,
                generation,
                SyncTriggerRequest::with_reply(trigger, mode, reply),
                connection,
            )
            .await;
            true
        }
        RuntimeCommand::TriggerOnly { trigger } => {
            let _ = process_sync_trigger_with_state(
                sync_state,
                shared,
                account,
                generation,
                SyncTriggerRequest::new(trigger, SyncMode::Incremental),
                connection,
            )
            .await;
            true
        }
        RuntimeCommand::CacheMaintenance {
            interactive_pressure,
            operation_id,
        } => {
            handle_cache_tick(
                shared,
                account_id,
                connection.gateway(),
                interactive_pressure,
                operation_id.as_deref(),
            )
            .await;
            false
        }
    }
}

pub(crate) async fn handle_push_event(
    sync_state: &Arc<SyncTriggerState>,
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    account_id: &AccountId,
    generation: RuntimeGeneration,
    connection: &mut AccountRuntimeConnectionState,
    event: PushStreamEvent,
) -> bool {
    match event {
        PushStreamEvent::Notification(ref notification) => {
            let remote_observation = connection
                .remote_observation()
                .unwrap_or_else(|| remote_observation_policy_for_account(account));
            ph_debug!(
                events::PUSH_NOTIFICATION_RECEIVED,
                account_id = %account_id,
                changed = ?notification.changed,
                checkpoint = notification.checkpoint.as_deref(),
                hints_incomplete = remote_observation.treats_hints_as_incomplete(),
                "push notification received"
            );
            if !push_notification_triggers_sync(remote_observation, notification) {
                return false;
            }
            let _ = process_sync_trigger_with_state(
                sync_state,
                shared,
                account,
                generation,
                SyncTriggerRequest::new(SyncTrigger::Push, SyncMode::Incremental),
                connection,
            )
            .await;
            true
        }
        PushStreamEvent::Connected { transport } => {
            ph_info!(
                events::PUSH_CONNECTED,
                account_id = %account_id,
                transport,
                "push connected"
            );
            shared
                .set_push_status(account_id, generation, PushStatus::Connected)
                .await;
            false
        }
        PushStreamEvent::Disconnected { transport, reason } => {
            ph_warn!(
                events::PUSH_DISCONNECTED,
                account_id = %account_id,
                transport,
                reason = %reason,
                "push disconnected"
            );
            shared
                .handle_push_disconnect(account_id, generation, &format!("{transport}: {reason}"))
                .await;
            false
        }
        PushStreamEvent::Fallback { from, to } => {
            ph_warn!(
                events::PUSH_FALLING_BACK,
                account_id = %account_id,
                from,
                to,
                "push falling back"
            );
            shared
                .handle_push_disconnect(
                    account_id,
                    generation,
                    &format!("falling back from {from} to {to}"),
                )
                .await;
            false
        }
    }
}

pub(crate) async fn handle_oauth_refresh_tick(
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    generation: RuntimeGeneration,
    account_id: &AccountId,
    connection: &mut AccountRuntimeConnectionState,
    state: &mut OAuthRefreshState,
) {
    if !state.enabled() {
        return;
    }
    let Some(resolver) = connection.secret_resolver() else {
        return;
    };
    let current_secret = match resolver.resolve_secret().await {
        Ok(secret) => secret,
        Err(error) => {
            ph_warn!(
                events::SUPERVISOR_OAUTH_REFRESH_FAILED,
                account_id = %account_id,
                error = %error,
                "OAuth token refresh check failed"
            );
            return;
        }
    };

    if let Some(last_secret) = state.last_secret() {
        if last_secret != current_secret {
            ph_info!(
                events::SUPERVISOR_OAUTH_TOKEN_REFRESHED,
                account_id = %account_id,
                "OAuth access token refreshed; rebuilding gateway"
            );
            connection.disconnect();
            if let Err(error) = ensure_connection(shared, account, generation, connection).await {
                ph_warn!(
                    events::SUPERVISOR_OAUTH_REFRESH_FAILED,
                    account_id = %account_id,
                    error = %error,
                    "OAuth gateway rebuild after token refresh failed"
                );
                return;
            }
        }
    }
    state.set_last_secret(current_secret);
}

pub(crate) fn remote_observation_policy_for_account(
    account: &AccountSettings,
) -> RemoteObservationPolicy {
    match account.driver {
        AccountDriver::Jmap => account
            .transport
            .provider_profile()
            .jmap()
            .remote_observation(),
        AccountDriver::ImapSmtp => account
            .transport
            .provider_profile()
            .imap()
            .remote_observation(),
        AccountDriver::Mock => RemoteObservationPolicy::disabled(),
    }
}

pub(crate) fn push_notification_triggers_sync(
    remote_observation: RemoteObservationPolicy,
    notification: &PushNotification,
) -> bool {
    !notification.changed.is_empty()
        || notification.checkpoint.is_some()
        || remote_observation.observes_empty_hints()
}
