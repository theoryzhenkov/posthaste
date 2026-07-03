use super::*;

pub(crate) fn sync_poll_interval(poll_interval: Duration) -> tokio::time::Interval {
    let mut interval =
        tokio::time::interval_at(tokio::time::Instant::now() + poll_interval, poll_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval
}

/// Builds a [`SyncProgressReporter`] whose callback forwards progress updates
/// to a *single* per-cycle writer task over a bounded channel, rather than
/// spawning a fresh detached `tokio::spawn` per progress event (N5 / M27
/// sub-unit (d)). Two problems this fixes together:
///
/// - **Unbounded spawns**: a chatty sync previously spawned one orphaned task
///   per progress callback, with no retained handle and no bound. Now there
///   is exactly one forwarder task per cycle, and the callback itself never
///   spawns.
/// - **Ordering** (a side effect of the above): concurrent per-callback tasks
///   could race and let an earlier progress value land after a later one.
///   The single forwarder drains its channel and awaits each
///   `set_sync_progress` call before the next, so writes for one cycle are
///   strictly ordered.
///
/// `tx` is captured by the returned `SyncProgressReporter`'s callback and
/// dropped along with it when the cycle ends (normally, or because the arm
/// budget abandoned it) — closing the channel and letting the forwarder task
/// drain whatever is left and exit on its own, rather than living forever.
/// The remaining race (a write already dequeued when the cycle is abandoned)
/// is closed by `cycle`: `shared.set_sync_progress` rejects it once
/// `record_arm_timeout` invalidates the account's current cycle token (the
/// M26 flag).
pub(crate) fn sync_progress_reporter(
    shared: &Arc<SupervisorShared>,
    account_id: AccountId,
    generation: RuntimeGeneration,
    cycle: SyncCycleGeneration,
    sync_id: String,
    trigger: SyncTrigger,
    started_at: String,
) -> SyncProgressReporter {
    let (tx, mut rx) = mpsc::channel::<SyncProgress>(SYNC_PROGRESS_CHANNEL_CAPACITY);

    let forwarder_shared = shared.clone();
    let forwarder_account_id = account_id.clone();
    tokio::spawn(async move {
        while let Some(progress) = rx.recv().await {
            forwarder_shared
                .set_sync_progress(&forwarder_account_id, generation, cycle, Some(progress))
                .await;
        }
    });

    SyncProgressReporter::new(sync_id, trigger, started_at, move |progress| {
        // A full channel means the forwarder is momentarily behind a burst;
        // dropping the newest update (rather than blocking this synchronous
        // callback, or growing unboundedly) is fine — progress is
        // display-only and monotonically superseded by whatever arrives
        // next.
        let _ = tx.try_send(progress);
    })
}

pub(crate) async fn process_automation_backfill_batch(
    shared: &Arc<SupervisorShared>,
    account_id: &AccountId,
    gateway: Option<SharedGateway>,
) -> bool {
    let Some(gateway) = gateway else {
        return true;
    };

    match shared
        .service
        .process_automation_backfill_job_batch(
            account_id,
            gateway.as_ref(),
            AUTOMATION_BACKFILL_BATCH_SIZE,
        )
        .await
    {
        Ok(outcome) => {
            if !outcome.ran {
                return false;
            }
            let events = outcome.events;
            let has_more = outcome.has_more;
            if !events.is_empty() {
                ph_info!(
                    events::SUPERVISOR_AUTOMATION_BACKFILL_COMPLETED,
                    account_id = %account_id,
                    event_count = events.len(),
                    has_more,
                    "automation backfill batch completed"
                );
                shared.publish_events(&events);
            }
            has_more
        }
        Err(error) => {
            ph_warn!(
                events::SUPERVISOR_AUTOMATION_BACKFILL_FAILED,
                account_id = %account_id,
                error = %error,
                "automation backfill batch failed"
            );
            true
        }
    }
}

/// Execute one sync cycle: ensure connection, run sync, publish events,
/// and update runtime status. On failure, tears down the connection and
/// records the error.
///
/// @spec docs/L1-sync#sync-loop
/// @spec docs/L1-sync#error-handling
pub(crate) async fn process_sync_trigger(
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    generation: RuntimeGeneration,
    trigger: SyncTrigger,
    mode: SyncMode,
    connection: &mut AccountRuntimeConnectionState,
    reply: Option<oneshot::Sender<Result<usize, ServiceError>>>,
) -> Result<(), ServiceError> {
    let account_id = account.id.clone();
    let sync_id = Id::generate().to_string();
    let span = info_span!(
        "sync.cycle",
        account_id = %account_id,
        sync_id = %sync_id,
        trigger = trigger.as_str()
    );

    process_sync_trigger_inner(
        shared, account, generation, trigger, mode, connection, reply, sync_id,
    )
    .instrument(span)
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn process_sync_trigger_inner(
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    generation: RuntimeGeneration,
    trigger: SyncTrigger,
    mode: SyncMode,
    connection: &mut AccountRuntimeConnectionState,
    reply: Option<oneshot::Sender<Result<usize, ServiceError>>>,
    sync_id: String,
) -> Result<(), ServiceError> {
    let account_id = account.id.clone();
    // Global concurrent-sync cap (D98(b) / R4 / O7): hold one slot from the
    // supervisor's dedicated sync governor for the whole cycle, so N accounts
    // syncing at boot open at most `GLOBAL_CONCURRENT_SYNC_LIMIT` provider syncs
    // at once rather than one per account. Every sync cycle — startup, poll,
    // push, manual, and coalesced follow-up — funnels through here, so this is
    // the single chokepoint. Distinct from the `CacheResourceGovernor` (cache
    // fetches only). Acquired before the connect/pull work below; released when
    // this future returns (or is dropped by an arm-budget timeout).
    let _sync_slot = shared.acquire_sync_slot().await;
    let started = Instant::now();
    let started_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| GatewayError::Rejected(error.to_string()))?;
    // Mint this cycle's token (N5 + the M26 flag / M27 sub-unit (d)): every
    // progress write for this cycle — the one below and every one the
    // forwarder task below writes on this cycle's behalf — carries it, so
    // `record_arm_timeout` can invalidate exactly this cycle if a
    // select!-loop arm abandons it before it reaches `mark_sync_success`/
    // `mark_sync_failure`.
    let cycle = shared.next_sync_cycle_generation(&account_id).await;
    ph_info!(
        events::SUPERVISOR_SYNC_STARTED,
        account_id = %account_id,
        sync_id = %sync_id,
        trigger = trigger.as_str(),
        "sync started"
    );
    shared
        .set_sync_progress(
            &account_id,
            generation,
            cycle,
            Some(SyncProgress {
                sync_id: sync_id.clone(),
                trigger: trigger.clone(),
                started_at: started_at.clone(),
                stage: SyncProgressStage::Connecting,
                detail: "Connecting account".to_string(),
                mailbox_name: None,
                mailbox_index: None,
                mailbox_count: None,
                message_count: None,
                total_count: None,
            }),
        )
        .await;

    let result = match ensure_connection(shared, account, generation, connection).await {
        Ok(()) => {
            if let AccountRuntimeConnectionState::Connected(connection) = connection {
                // The flush → observe → retire cycle is owned by
                // `sync_account_with_mode`: it flushes pending local-first ops
                // before the pull and retires confirmed assertions after it.
                //
                // @spec docs/replication/L1#convergence-cycle
                let progress = sync_progress_reporter(
                    shared,
                    account_id.clone(),
                    generation,
                    cycle,
                    sync_id.clone(),
                    trigger.clone(),
                    started_at.clone(),
                );
                // Broadcast each event group as the sync produces it so mail
                // surfaces progressively instead of after the whole sync.
                let mut publish = |events: &[DomainEvent]| shared.publish_events(events);
                shared
                    .service
                    .sync_account_with_mode(
                        &account_id,
                        trigger.clone(),
                        mode,
                        connection.gateway.as_ref(),
                        Some(progress),
                        &mut publish,
                    )
                    .await
            } else {
                Err(GatewayError::Unavailable(account_id.to_string()).into())
            }
        }
        Err(error) => Err(error),
    };

    match result {
        Ok(events) => {
            let event_count = events.len();
            ph_info!(
                events::SUPERVISOR_SYNC_COMPLETED,
                account_id = %account_id,
                sync_id = %sync_id,
                trigger = trigger.as_str(),
                event_count,
                duration_ms = started.elapsed().as_millis() as u64,
                "sync completed"
            );
            // Events were already broadcast per group by the `publish` callback
            // during the sync; only the terminal status transition remains.
            shared.mark_sync_success(&account_id, generation).await;
            if let Some(reply) = reply {
                let _ = reply.send(Ok(event_count));
            }
        }
        Err(error) => {
            shared.remove_gateway(&account_id).await;
            connection.disconnect(); // tears down gateway + push stream together
            let stage = sync_failure_stage(&error);
            ph_error!(
                events::SUPERVISOR_SYNC_FAILED,
                account_id = %account_id,
                sync_id = %sync_id,
                trigger = trigger.as_str(),
                error = %error,
                stage,
                duration_ms = started.elapsed().as_millis() as u64,
                "sync failed"
            );
            if let Ok(event) = shared.service.record_sync_failure(
                &account_id,
                error.code(),
                &error.to_string(),
                trigger,
                stage,
            ) {
                shared.publish_events(&[event]);
            }
            shared
                .mark_sync_failure(&account_id, generation, &error)
                .await;
            if let Some(reply) = reply {
                let _ = reply.send(Err(error));
            }
        }
    }

    Ok(())
}

pub(crate) fn sync_failure_stage(error: &ServiceError) -> &'static str {
    match error.kind() {
        ServiceErrorKind::GatewayUnavailable
        | ServiceErrorKind::AuthError
        | ServiceErrorKind::NetworkError => "connect",
        _ => "sync",
    }
}
