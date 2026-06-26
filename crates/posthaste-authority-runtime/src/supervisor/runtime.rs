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
) {
    let account_id = account.id.clone();
    let mut connection = AccountRuntimeConnectionState::default();
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

    // Initial sync + gateway setup
    let _ = process_sync_trigger_with_state(
        &sync_state,
        &shared,
        &account,
        generation,
        SyncTrigger::Startup,
        SyncMode::Incremental,
        &mut connection,
        None,
    )
    .await;
    let mut interval = sync_poll_interval(shared.poll_interval);

    loop {
        let next_push = async {
            match connection.push_events_mut() {
                Some(stream) => stream.next().await,
                None => pending().await,
            }
        };

        tokio::select! {
            _ = interval.tick() => {
                handle_poll_tick(&sync_state, &shared, &account, generation, &mut connection).await;
                interval = sync_poll_interval(shared.poll_interval);
            }
            _ = backfill_interval.tick() => {
                handle_backfill_tick(&shared, &account_id, connection.gateway()).await;
            }
            _ = cache_interval.tick() => {
                handle_cache_tick(
                    &shared,
                    &account_id,
                    connection.gateway(),
                    CACHE_BACKGROUND_PRESSURE,
                    None,
                ).await;
            }
            Some(command) = command_rx.recv() => {
                if handle_runtime_command(
                    &sync_state,
                    &shared,
                    &account,
                    &account_id,
                    &mut connection,
                    generation,
                    command,
                ).await {
                    interval = sync_poll_interval(shared.poll_interval);
                }
            }
            Some(event) = next_push => {
                if handle_push_event(&sync_state, &shared, &account, &account_id, generation, &mut connection, event).await {
                    interval = sync_poll_interval(shared.poll_interval);
                }
            }
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
    trigger: SyncTrigger,
    mode: SyncMode,
    connection: &mut AccountRuntimeConnectionState,
    reply: Option<oneshot::Sender<Result<usize, ServiceError>>>,
) {
    let mut next = Some((trigger, mode, reply));
    while let Some((trigger, mode, reply)) = next {
        sync_state.increment_sync_cycle_count();
        sync_state.start_sync();
        let _ = process_sync_trigger(
            shared, account, generation, trigger, mode, connection, reply,
        )
        .await;
        sync_state.finish_sync();
        next = sync_state
            .take_pending()
            .await
            .map(|trigger| (trigger, SyncMode::Incremental, None));
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
        SyncTrigger::Poll,
        SyncMode::Incremental,
        connection,
        None,
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
                trigger,
                mode,
                connection,
                Some(reply),
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
                trigger,
                SyncMode::Incremental,
                connection,
                None,
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
                SyncTrigger::Push,
                SyncMode::Incremental,
                connection,
                None,
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
