use super::*;

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
        let mut current = self.runtime_overview(account_id).await;
        match progress {
            Some(progress) => {
                if !matches!(progress.stage, SyncProgressStage::Connecting)
                    && !matches!(current.status, AccountStatus::Syncing)
                {
                    return;
                }
                current.sync_progress = Some(progress);
                current.status = AccountStatus::Syncing;
            }
            None => {
                current.sync_progress = None;
            }
        }
        self.set_runtime_overview_for_generation(account_id, generation, current)
            .await;
    }

    /// Update only the push status, preserving other overview fields.
    pub(crate) async fn set_push_status(
        &self,
        account_id: &AccountId,
        generation: RuntimeGeneration,
        push: PushStatus,
    ) {
        let mut current = self.runtime_overview(account_id).await;
        current.push = push;
        self.set_runtime_overview_for_generation(account_id, generation, current)
            .await;
    }

    /// Record a successful sync: set status to Ready, clear error, update timestamp.
    pub(crate) async fn mark_sync_success(
        &self,
        account_id: &AccountId,
        generation: RuntimeGeneration,
    ) {
        let mut current = self.runtime_overview(account_id).await;
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
        self.set_runtime_overview_for_generation(account_id, generation, current)
            .await;
    }

    /// Record a sync failure: derive status from error type, store error details.
    pub(crate) async fn mark_sync_failure(
        &self,
        account_id: &AccountId,
        generation: RuntimeGeneration,
        error: &ServiceError,
    ) {
        let mut current = self.runtime_overview(account_id).await;
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
        self.set_runtime_overview_for_generation(account_id, generation, current)
            .await;
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
        self.set_runtime_overview_inner(account_id, None, overview)
            .await;
    }

    pub(crate) async fn set_runtime_overview_for_generation(
        &self,
        account_id: &AccountId,
        generation: RuntimeGeneration,
        overview: AccountRuntimeOverview,
    ) {
        self.set_runtime_overview_inner(account_id, Some(generation), overview)
            .await;
    }

    async fn set_runtime_overview_inner(
        &self,
        account_id: &AccountId,
        generation: Option<RuntimeGeneration>,
        overview: AccountRuntimeOverview,
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
