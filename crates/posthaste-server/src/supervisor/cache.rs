use super::*;

pub(crate) async fn process_cache_maintenance_batch(
    shared: &Arc<SupervisorShared>,
    account_id: &AccountId,
    gateway: Option<SharedGateway>,
    interactive_pressure: f64,
    operation_id: Option<&str>,
) {
    let operation_id = operation_id.unwrap_or("");
    let is_interactive = !operation_id.is_empty();
    let started = Instant::now();
    let lease = {
        let mut governor = shared.cache_resources.lock().await;
        governor.grant(started, interactive_pressure)
    };
    let mut feedback = CacheMaintenanceFeedback::default();
    if is_interactive || lease.in_backoff {
        ph_debug!(
            events::CACHE_MAINTENANCE_LEASE_GRANTED,
            account_id = %account_id,
            operation_id,
            interactive_pressure,
            stale_rescore_limit = lease.stale_rescore_limit,
            rescore_limit = lease.rescore_limit,
            fetch_request_limit = lease.fetch.request_limit,
            fetch_byte_limit = lease.fetch.byte_limit,
            network_rate_multiplier = lease.network_rate_multiplier,
            in_backoff = lease.in_backoff,
            "cache maintenance resource lease granted"
        );
    }

    if lease.stale_rescore_limit > 0 {
        match shared.service.queue_stale_cache_rescore_batch(
            account_id,
            CACHE_STALE_RESCORE_AFTER,
            lease.stale_rescore_limit,
        ) {
            Ok(queued) => {
                feedback.stale_rescore_queued = queued;
                if queued > 0 {
                    ph_debug!(
                        events::CACHE_RESCORE_STALE_QUEUED,
                        account_id = %account_id,
                        operation_id,
                        queued,
                        stale_after_seconds = CACHE_STALE_RESCORE_AFTER.as_secs(),
                        lease_limit = lease.stale_rescore_limit,
                        "stale cache rescore candidates queued"
                    );
                }
            }
            Err(error) => {
                feedback.had_error = true;
                ph_warn!(
                    events::CACHE_RESCORE_STALE_QUEUE_FAILED,
                    account_id = %account_id,
                    operation_id,
                    error = %error,
                    "stale cache rescore queueing failed"
                );
            }
        }
    }

    if lease.rescore_limit > 0 {
        match shared
            .service
            .process_cache_rescore_batch(account_id, lease.rescore_limit)
        {
            Ok(outcome) => {
                feedback.rescore_scanned = outcome.scanned;
                if outcome.updated > 0 {
                    ph_debug!(
                        events::CACHE_RESCORE_COMPLETED,
                        account_id = %account_id,
                        operation_id,
                        scanned = outcome.scanned,
                        updated = outcome.updated,
                        skipped = outcome.skipped,
                        lease_limit = lease.rescore_limit,
                        "cache rescore batch completed"
                    );
                }
            }
            Err(error) => {
                feedback.had_error = true;
                ph_warn!(
                    events::CACHE_RESCORE_FAILED,
                    account_id = %account_id,
                    operation_id,
                    error = %error,
                    "cache rescore batch failed"
                );
            }
        }
    }

    match (gateway, lease.fetch.has_fetch_budget()) {
        (None, true) => {
            if is_interactive {
                ph_debug!(
                    events::CACHE_FETCH_SKIPPED_NO_GATEWAY,
                    account_id = %account_id,
                    operation_id,
                    fetch_request_limit = lease.fetch.request_limit,
                    fetch_byte_limit = lease.fetch.byte_limit,
                    "cache worker skipped because no gateway is connected"
                );
            }
        }
        (Some(gateway), true) => {
            match shared
                .service
                .process_body_cache_batch(account_id, gateway.as_ref(), lease.fetch)
                .await
            {
                Ok(outcome) => {
                    feedback.fetch_attempted = outcome.attempted;
                    feedback.fetch_attempted_bytes = outcome.attempted_bytes;
                    feedback.fetch_cached = outcome.cached;
                    feedback.fetch_failed = outcome.failed;
                    if !outcome.events.is_empty() {
                        shared.publish_events(&outcome.events);
                    }
                    if outcome.attempted > 0 || outcome.cached > 0 || outcome.failed > 0 {
                        ph_info!(
                            events::CACHE_FETCH_COMPLETED,
                            account_id = %account_id,
                            operation_id,
                            scanned = outcome.scanned,
                            attempted = outcome.attempted,
                            attempted_bytes = outcome.attempted_bytes,
                            cached = outcome.cached,
                            cached_bytes = outcome.cached_bytes,
                            failed = outcome.failed,
                            skipped = outcome.skipped,
                            event_count = outcome.events.len(),
                            "cache worker batch completed"
                        );
                    } else if outcome.skipped > 0 {
                        ph_debug!(
                            events::CACHE_FETCH_SKIPPED_BUDGET,
                            account_id = %account_id,
                            operation_id,
                            scanned = outcome.scanned,
                            skipped = outcome.skipped,
                            "cache worker skipped candidates outside current resource/cache budget"
                        );
                    } else {
                        if is_interactive {
                            ph_debug!(
                                events::CACHE_FETCH_NO_WORK,
                                account_id = %account_id,
                                operation_id,
                                scanned = outcome.scanned,
                                "cache worker batch completed without fetch work"
                            );
                        }
                    }
                }
                Err(error) => {
                    feedback.had_error = true;
                    feedback.had_fetch_error = true;
                    ph_warn!(
                        events::CACHE_FETCH_FAILED,
                        account_id = %account_id,
                        operation_id,
                        error = %error,
                        "cache worker batch failed"
                    );
                }
            }
        }
        _ => {
            if is_interactive || lease.in_backoff {
                ph_debug!(
                    events::CACHE_FETCH_SKIPPED_NO_LEASE,
                    account_id = %account_id,
                    operation_id,
                    in_backoff = lease.in_backoff,
                    "cache worker skipped because no fetch resource lease was granted"
                );
            }
        }
    }

    feedback.elapsed = started.elapsed();
    let now = Instant::now();
    let mut governor = shared.cache_resources.lock().await;
    governor.record_feedback(now, &lease, feedback);
    let has_work = feedback.stale_rescore_queued > 0
        || feedback.rescore_scanned > 0
        || feedback.fetch_attempted > 0
        || feedback.fetch_cached > 0
        || feedback.fetch_failed > 0;
    if is_interactive || has_work || feedback.had_error {
        ph_debug!(
            events::CACHE_MAINTENANCE_FEEDBACK_RECORDED,
            account_id = %account_id,
            operation_id,
            stale_rescore_queued = feedback.stale_rescore_queued,
            rescore_scanned = feedback.rescore_scanned,
            fetch_attempted = feedback.fetch_attempted,
            fetch_attempted_bytes = feedback.fetch_attempted_bytes,
            fetch_cached = feedback.fetch_cached,
            fetch_failed = feedback.fetch_failed,
            elapsed_ms = feedback.elapsed.as_millis(),
            had_error = feedback.had_error,
            had_fetch_error = feedback.had_fetch_error,
            network_rate_multiplier = governor.network_rate_multiplier(),
            in_backoff = governor.is_in_backoff(now),
            "cache maintenance resource feedback recorded"
        );
    }
}
