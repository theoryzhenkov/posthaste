use super::*;

impl LiveImapSmtpGateway {
    pub async fn connect(
        config: ImapConnectionConfig,
        smtp_config: SmtpConnectionConfig,
        store: Option<Arc<dyn MailStore>>,
        secret_resolver: Arc<dyn SecretResolver>,
    ) -> Result<Self, ImapAdapterError> {
        let secret = secret_resolver
            .resolve_secret()
            .await
            .map_err(|error| ImapAdapterError::Auth(error.to_string()))?;
        let mut resolved_config = config.clone();
        resolved_config.secret = secret;
        let discovery = discover_imap_account(&resolved_config).await?;
        Ok(Self {
            config,
            smtp_config,
            discovery,
            store,
            secret_resolver,
        })
    }

    pub fn discovery(&self) -> &DiscoveredImapAccount {
        &self.discovery
    }

    pub(crate) fn location_and_mailbox_name(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(posthaste_domain::ImapMessageLocation, String), GatewayError> {
        let locations = self
            .store("message location lookup")?
            .list_imap_message_locations(account_id, message_id)
            .map_err(store_error_to_gateway)?;
        let location = locations.first().cloned().ok_or_else(|| {
            GatewayError::Rejected(format!("missing IMAP location for message {message_id}"))
        })?;
        let mailbox_name = self.mailbox_name_for_id(account_id, &location.mailbox_id)?;

        Ok((location, mailbox_name))
    }

    pub(crate) fn store(&self, operation: &str) -> Result<&Arc<dyn MailStore>, GatewayError> {
        self.store.as_ref().ok_or_else(|| unsupported(operation))
    }

    pub(crate) fn mailbox_name_for_id(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
    ) -> Result<String, GatewayError> {
        self.store("mailbox name lookup")?
            .get_imap_mailbox_state(account_id, mailbox_id)
            .map_err(store_error_to_gateway)?
            .map(|state| state.mailbox_name)
            .or_else(|| {
                self.discovery
                    .mailboxes
                    .iter()
                    .find(|mailbox| &mailbox.id == mailbox_id)
                    .map(|mailbox| mailbox.name.clone())
            })
            .ok_or_else(|| {
                GatewayError::Rejected(format!("missing IMAP mailbox name for {mailbox_id}"))
            })
    }
}

pub(crate) fn unsupported(operation: &str) -> GatewayError {
    GatewayError::Rejected(format!(
        "IMAP/SMTP {operation} is not implemented yet; discovery is available"
    ))
}

pub(crate) fn simple_imap_move_mailboxes<'a>(
    capabilities: &ImapCapabilities,
    delta: &'a crate::ImapMailboxReplacementDelta,
) -> Option<(&'a MailboxId, &'a MailboxId)> {
    if matches!(
        plan_imap_move(capabilities),
        ImapMoveStrategy::CopyDeleteThenResync
    ) || delta.add.len() != 1
        || delta.remove.len() != 1
    {
        return None;
    }

    Some((&delta.remove[0], &delta.add[0]))
}

pub(crate) async fn plan_mailboxes(
    client: &mut ImapClient,
    account_id: &AccountId,
    discovery: &DiscoveredImapAccount,
    store: Option<&dyn MailStore>,
    selectable_mailbox_count: usize,
    progress: &Option<SyncProgressReporter>,
) -> Result<Vec<PlannedImapMailbox>, GatewayError> {
    report_sync_progress(
        progress,
        ImapSyncProgressUpdate::new(SyncProgressStage::Planning, "Planning mailbox sync")
            .with_mailbox_count(selectable_mailbox_count),
    );

    let mut planned_mailboxes = Vec::new();
    let provider = discovery.provider_profile();
    for (mailbox_index, mailbox) in discovery
        .mailboxes
        .iter()
        .filter(|mailbox| mailbox.selectable)
        .enumerate()
    {
        report_sync_progress(
            progress,
            ImapSyncProgressUpdate::new(SyncProgressStage::Planning, "Planning mailbox sync")
                .with_mailbox(
                    mailbox.name.clone(),
                    mailbox_index + 1,
                    selectable_mailbox_count,
                ),
        );
        planned_mailboxes.push(
            plan_mailbox(
                client,
                account_id,
                mailbox,
                &discovery.capabilities,
                &provider,
                store,
            )
            .await?,
        );
    }

    Ok(planned_mailboxes)
}

pub(crate) async fn plan_mailbox(
    client: &mut ImapClient,
    account_id: &AccountId,
    mailbox: &DiscoveredImapMailbox,
    capabilities: &ImapCapabilities,
    provider: &ProviderProfile,
    store: Option<&dyn MailStore>,
) -> Result<PlannedImapMailbox, GatewayError> {
    let Some(store) = store else {
        ph_info!(
            events::IMAP_MAILBOX_SYNC_PLANNED,
            account_id = %account_id,
            mailbox_id = %mailbox.id,
            plan = "full_snapshot",
            has_stored_state = false,
            local_message_count = 0usize,
            "IMAP mailbox sync planned"
        );
        return Ok(PlannedImapMailbox {
            id: mailbox.id.clone(),
            name: mailbox.name.clone(),
            stored_state: None,
            local_locations: Vec::new(),
            plan: PlannedImapMailboxSync::Sync(ImapMailboxSyncPlan::FullSnapshot {
                reason: posthaste_domain::ImapFullSyncReason::InitialSync,
            }),
        });
    };

    let stored_state = store
        .get_imap_mailbox_state(account_id, &mailbox.id)
        .map_err(store_error_to_gateway)?;
    let local_locations = store
        .list_imap_mailbox_message_locations(account_id, &mailbox.id)
        .map_err(store_error_to_gateway)?;
    if provider.imap().allows_status_skip() {
        if let Some(stored_state) = stored_state.as_ref() {
            let status = status_imap_mailbox(
                client,
                &mailbox.name,
                capabilities.supports_condstore() && stored_state.highest_modseq.is_some(),
            )
            .await
            .map_err(imap_error_to_gateway)?;
            if mailbox_status_proves_unchanged(stored_state, local_locations.len(), &status) {
                ph_info!(
                    events::IMAP_MAILBOX_SYNC_PLANNED,
                    account_id = %account_id,
                    mailbox_id = %mailbox.id,
                    plan = "skip_unchanged",
                    has_stored_state = true,
                    uid_validity = status.uid_validity.map(|uid_validity| uid_validity.0),
                    uid_next = status.uid_next.map(|uid| uid.0),
                    message_count = status.messages,
                    local_message_count = local_locations.len(),
                    "IMAP mailbox sync planned"
                );
                ph_debug!(
                    events::IMAP_MAILBOX_SYNC_PLAN_DETAIL,
                    account_id = %account_id,
                    mailbox_id = %mailbox.id,
                    mailbox_name = %mailbox.name,
                    plan = "skip_unchanged",
                    "IMAP mailbox sync plan detail"
                );
                return Ok(PlannedImapMailbox {
                    id: mailbox.id.clone(),
                    name: mailbox.name.clone(),
                    stored_state: Some(stored_state.clone()),
                    local_locations,
                    plan: PlannedImapMailboxSync::SkipUnchanged,
                });
            }
        }
    }

    let selected = examine_selected_mailbox(client, &mailbox.name)
        .await
        .map_err(imap_error_to_gateway)?;
    let plan = plan_imap_mailbox_sync(capabilities, provider, stored_state.as_ref(), &selected);
    ph_info!(
        events::IMAP_MAILBOX_SYNC_PLANNED,
        account_id = %account_id,
        mailbox_id = %mailbox.id,
        plan = imap_sync_plan_name(&plan),
        has_stored_state = stored_state.is_some(),
        uid_validity = selected.uid_validity.0,
        uid_next = selected.uid_next.map(|uid| uid.0),
        highest_modseq = selected.highest_modseq.map(|modseq| modseq.0),
        local_message_count = local_locations.len(),
        "IMAP mailbox sync planned"
    );
    ph_debug!(
        events::IMAP_MAILBOX_SYNC_PLAN_DETAIL,
        account_id = %account_id,
        mailbox_id = %mailbox.id,
        mailbox_name = %mailbox.name,
        plan = imap_sync_plan_name(&plan),
        "IMAP mailbox sync plan detail"
    );

    Ok(PlannedImapMailbox {
        id: mailbox.id.clone(),
        name: mailbox.name.clone(),
        stored_state,
        local_locations,
        plan: PlannedImapMailboxSync::Sync(plan),
    })
}

pub(crate) fn planned_mailboxes_include_full_snapshot(
    planned_mailboxes: &[PlannedImapMailbox],
) -> bool {
    planned_mailboxes
        .iter()
        .any(|mailbox| mailbox.plan.is_full_snapshot())
}

pub(crate) fn planned_mailboxes_require_partial_delta_batch(
    planned_mailboxes: &[PlannedImapMailbox],
) -> bool {
    planned_mailboxes
        .iter()
        .any(|mailbox| mailbox.plan.requires_partial_delta_batch())
}
