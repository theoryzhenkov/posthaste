use super::*;

impl MailService {
    /// Run a full sync cycle: load cursors, fetch delta, apply batch, emit events.
    ///
    /// @spec docs/L1-sync#sync-loop
    pub async fn sync_account(
        &self,
        account_id: &AccountId,
        trigger: SyncTrigger,
        gateway: &dyn MailGateway,
        progress: Option<crate::SyncProgressReporter>,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        self.sync_account_with_mode(
            account_id,
            trigger,
            SyncMode::Incremental,
            gateway,
            progress,
        )
        .await
    }

    /// Run a sync cycle with an explicit user-requested mode.
    ///
    /// @spec docs/L1-sync#sync-loop
    pub async fn sync_account_with_mode(
        &self,
        account_id: &AccountId,
        trigger: SyncTrigger,
        mode: SyncMode,
        gateway: &dyn MailGateway,
        progress: Option<crate::SyncProgressReporter>,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let mut cursors = self.sync_state.get_sync_cursors(account_id)?;
        if mode.requires_full_message_metadata() {
            cursors.retain(|cursor| cursor.object_type != SyncObject::Message);
        }
        // FLUSH: push pending local-first ops before the pull so the observe
        // sees post-mutation provider state. Successful message state
        // assertions rest in `applied` (folded by the read overlay); they are
        // retired below once this sync writes their effect into the projection.
        // Best-effort: offline leaves them pending and the overlay still folds.
        //
        // @spec docs/replication/L1#convergence-cycle
        let mut events = self.flush_account(account_id, gateway).await?;

        // OBSERVE: pull and write authoritative provider state.
        let batch = gateway.sync(account_id, &cursors, progress.clone()).await?;
        if let Some(progress) = progress {
            progress.report(crate::SyncProgress {
                sync_id: String::new(),
                trigger: trigger.clone(),
                started_at: String::new(),
                stage: crate::SyncProgressStage::Storing,
                detail: "Applying synced changes".to_string(),
                mailbox_name: None,
                mailbox_index: None,
                mailbox_count: None,
                message_count: Some(batch.messages.len()),
                total_count: None,
            });
        }
        events.extend(self.sync_writer.apply_sync_batch(account_id, &batch)?);
        let mut post_commit_errors = Vec::new();
        if let Some(account) = self.config.get_source(account_id)? {
            let settings = self.config.get_app_settings()?;
            if let Err(error) = self.upsert_body_cache_candidates(
                account_id,
                &account,
                &settings.cache_policy,
                &batch.messages,
            ) {
                ph_warn!(
                    events::DOMAIN_CACHE_CANDIDATE_POST_SYNC_FAILED,
                    account_id = %account_id,
                    error = %error,
                    "post-sync body cache candidate update failed after sync batch commit"
                );
                post_commit_errors.push(error.code().to_string());
            }
        }
        let action_events = match self
            .apply_automation_rules(account_id, &batch.messages, gateway)
            .await
        {
            Ok(events) => events,
            Err(error) => {
                ph_warn!(
                    events::DOMAIN_AUTOMATION_POST_SYNC_FAILED,
                    account_id = %account_id,
                    error = %error,
                    "post-sync automation failed after sync batch commit"
                );
                post_commit_errors.push(error.code().to_string());
                Vec::new()
            }
        };
        let action_count = action_events.len();
        events.extend(action_events);
        // Push any ops automation enqueued while applying the batch; they rest
        // in `applied` and retire on the next sync cycle.
        match self.flush_account(account_id, gateway).await {
            Ok(settlement_events) => events.extend(settlement_events),
            Err(error) => {
                ph_warn!(
                    events::DOMAIN_AUTOMATION_POST_SYNC_FAILED,
                    account_id = %account_id,
                    error = %error,
                    "post-sync automation outbox flush failed after sync batch commit"
                );
                post_commit_errors.push(error.code().to_string());
            }
        }
        // RETIRE: drop every applied message assertion the projection now
        // satisfies (folding it has become a no-op). Content-based, so it never
        // retires before the projection reflects the effect.
        //
        // @spec docs/replication/L1#retire-on-confirmation
        self.retire_satisfied_operations(account_id)?;
        let sync_event = self.events.append_event(
        account_id,
        EVENT_TOPIC_SYNC_COMPLETED,
        None,
        None,
        json!({
            "mailboxCount": batch.mailboxes.len(),
            "messageCount": batch.messages.len(),
            "deletedImapLocationCount": batch.deleted_imap_message_locations.len(),
            "deletedMessageCount": batch.deleted_message_ids.len(),
            "automationEventCount": action_count,
            "trigger": trigger.as_str(),
            "mode": mode.as_str(),
            "resources": [
                { "kind": "sync", "operation": "completed", "accountId": account_id.as_str(), "mode": mode.as_str() },
                { "kind": "mailbox", "operation": "refreshed", "accountId": account_id.as_str() },
                { "kind": "message", "operation": "refreshed", "accountId": account_id.as_str() },
            ],
            "postCommitErrors": post_commit_errors,
        }),
    )?;
        events.push(sync_event);
        Ok(events)
    }

    /// One convergence observation: flush pending ops, pull authoritative
    /// provider state into the projection, and retire the message assertions
    /// the pull confirmed. Used where a projection-walking loop (automation
    /// backfill) must see its own progress reflected in the projection before
    /// the next batch query, since query filters read the projection before the
    /// read overlay folds.
    ///
    /// @spec docs/replication/L1#convergence-cycle
    pub(crate) async fn flush_and_observe(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let mut events = self.flush_account(account_id, gateway).await?;
        let cursors = self.sync_state.get_sync_cursors(account_id)?;
        let batch = gateway.sync(account_id, &cursors, None).await?;
        events.extend(self.sync_writer.apply_sync_batch(account_id, &batch)?);
        self.retire_satisfied_operations(account_id)?;
        Ok(events)
    }

    /// Append a `sync.failed` event to the event log.
    ///
    /// @spec docs/L1-sync#error-handling
    pub fn record_sync_failure(
        &self,
        account_id: &AccountId,
        code: &str,
        message: &str,
        trigger: SyncTrigger,
        stage: &str,
    ) -> Result<DomainEvent, ServiceError> {
        self.events
        .append_event(
            account_id,
            EVENT_TOPIC_SYNC_FAILED,
            None,
            None,
            json!({
                "code": code,
                "message": message,
                "trigger": trigger.as_str(),
                "stage": stage,
                "resources": [
                    { "kind": "sync", "operation": "failed", "accountId": account_id.as_str() },
                    { "kind": "accountRuntime", "operation": "updated", "accountId": account_id.as_str() },
                ],
            }),
        )
        .map_err(Into::into)
    }
}
