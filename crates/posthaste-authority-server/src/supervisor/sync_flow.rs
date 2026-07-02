use super::*;

pub(crate) fn sync_poll_interval(poll_interval: Duration) -> tokio::time::Interval {
    let mut interval =
        tokio::time::interval_at(tokio::time::Instant::now() + poll_interval, poll_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval
}

pub(crate) fn sync_progress_reporter(
    shared: &Arc<SupervisorShared>,
    account_id: AccountId,
    generation: RuntimeGeneration,
    sync_id: String,
    trigger: SyncTrigger,
    started_at: String,
) -> SyncProgressReporter {
    let shared = shared.clone();
    SyncProgressReporter::new(sync_id, trigger, started_at, move |progress| {
        let shared = shared.clone();
        let account_id = account_id.clone();
        tokio::spawn(async move {
            shared
                .set_sync_progress(&account_id, generation, Some(progress))
                .await;
        });
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
    let started = Instant::now();
    let started_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| GatewayError::Rejected(error.to_string()))?;
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
