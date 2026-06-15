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
        self.set_runtime_overview(account_id, current).await;
    }

    /// Update only the push status, preserving other overview fields.
    pub(crate) async fn set_push_status(&self, account_id: &AccountId, push: PushStatus) {
        let mut current = self.runtime_overview(account_id).await;
        current.push = push;
        self.set_runtime_overview(account_id, current).await;
    }

    /// Record a successful sync: set status to Ready, clear error, update timestamp.
    pub(crate) async fn mark_sync_success(&self, account_id: &AccountId) {
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
        self.set_runtime_overview(account_id, current).await;
    }

    /// Record a sync failure: derive status from error type, store error details.
    pub(crate) async fn mark_sync_failure(&self, account_id: &AccountId, error: &ServiceError) {
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
        self.set_runtime_overview(account_id, current).await;
    }

    /// Handle a push stream disconnect: emit event and set push status to Reconnecting.
    pub(crate) async fn handle_push_disconnect(&self, account_id: &AccountId, message: &str) {
        let event = self
            .store
            .append_event(
                account_id,
                EVENT_TOPIC_PUSH_DISCONNECTED,
                None,
                None,
                json!({ "message": message }),
            )
            .ok();
        if let Some(event) = event {
            self.publish_events(&[event]);
        }
        self.set_push_status(account_id, PushStatus::Reconnecting)
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
        let previous = self
            .runtime_overviews
            .write()
            .await
            .insert(account_id.to_string(), overview.clone());

        let mut side_effects = Vec::new();
        if previous.as_ref().map(|item| &item.status) != Some(&overview.status)
            || previous.as_ref().map(|item| &item.sync_progress) != Some(&overview.sync_progress)
        {
            if let Ok(event) = self.store.append_event(
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
                side_effects.push(event);
            }
        }

        match (previous.as_ref().map(|item| &item.push), &overview.push) {
            (Some(PushStatus::Connected), PushStatus::Connected) => {}
            (_, PushStatus::Connected) => {
                if let Ok(event) = self.store.append_event(
                    account_id,
                    EVENT_TOPIC_PUSH_CONNECTED,
                    None,
                    None,
                    json!({}),
                ) {
                    side_effects.push(event);
                }
            }
            (Some(PushStatus::Connected), _) => {
                if let Ok(event) = self.store.append_event(
                    account_id,
                    EVENT_TOPIC_PUSH_DISCONNECTED,
                    None,
                    None,
                    json!({}),
                ) {
                    side_effects.push(event);
                }
            }
            _ => {}
        }

        self.publish_events(&side_effects);
    }
}
