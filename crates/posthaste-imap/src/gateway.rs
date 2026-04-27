use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use posthaste_domain::{
    now_iso8601, plan_imap_mailbox_sync, plan_imap_move, AccountId, BlobId, FetchedBody,
    GatewayError, Identity, ImapCapabilities, ImapMailboxSyncPlan, ImapMailboxSyncState,
    ImapMessageLocation, ImapMoveStrategy, ImapUid, ImapUidValidity, MailGateway, MailStore,
    MailboxId, MessageId, MutationOutcome, PushTransport, ReplyContext, SendMessageRequest,
    SetKeywordsCommand, StoreError, SyncBatch, SyncCursor,
};
use tracing::{debug, info, warn};

use crate::{
    append_smtp_sent_copy, apply_imap_keyword_delta_by_location,
    copy_imap_message_to_mailbox_by_location, discover_imap_account, examine_imap_mailbox,
    expunge_imap_message_by_location, fetch_imap_reply_context_by_location,
    fetch_mailbox_changed_since_snapshot, fetch_mailbox_header_snapshot,
    fetch_mailbox_headers_after_uid, fetch_message_body_by_location, fetch_raw_message_by_location,
    imap_attachment_bytes_from_raw_mime, imap_condstore_delta_sync_batch, imap_delta_sync_batch,
    imap_full_sync_batch, imap_mailbox_replacement_delta,
    imap_mailbox_state_from_changed_since_snapshot, imap_mailbox_state_from_header_snapshot,
    mark_imap_message_deleted_by_location, move_imap_message_to_mailbox_by_location,
    parse_imap_attachment_blob_id, smtp_sent_copy_strategy, submit_smtp_message,
    DiscoveredImapAccount, ImapAdapterError, ImapChangedSinceSnapshot, ImapConnectionConfig,
    ImapMappedHeader, SmtpConnectionConfig, SmtpSentCopyStrategy,
};

/// Live IMAP/SMTP gateway after successful IMAP discovery.
///
/// The first implementation performs conservative full metadata snapshots.
/// Mutations use conservative IMAP commands where implemented and reject
/// unsupported command surfaces with typed gateway errors.
pub struct LiveImapSmtpGateway {
    config: ImapConnectionConfig,
    smtp_config: SmtpConnectionConfig,
    discovery: DiscoveredImapAccount,
    store: Option<Arc<dyn MailStore>>,
}

struct PlannedImapMailbox {
    id: MailboxId,
    name: String,
    stored_state: Option<ImapMailboxSyncState>,
    local_locations: Vec<ImapMessageLocation>,
    plan: ImapMailboxSyncPlan,
}

impl LiveImapSmtpGateway {
    pub async fn connect(
        config: ImapConnectionConfig,
        smtp_config: SmtpConnectionConfig,
        store: Option<Arc<dyn MailStore>>,
    ) -> Result<Self, ImapAdapterError> {
        let discovery = discover_imap_account(&config).await?;
        Ok(Self {
            config,
            smtp_config,
            discovery,
            store,
        })
    }

    pub fn discovery(&self) -> &DiscoveredImapAccount {
        &self.discovery
    }

    fn location_and_mailbox_name(
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

    fn store(&self, operation: &str) -> Result<&Arc<dyn MailStore>, GatewayError> {
        self.store.as_ref().ok_or_else(|| unsupported(operation))
    }

    fn mailbox_name_for_id(
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

fn unsupported(operation: &str) -> GatewayError {
    GatewayError::Rejected(format!(
        "IMAP/SMTP {operation} is not implemented yet; discovery is available"
    ))
}

fn simple_imap_move_mailboxes<'a>(
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

#[async_trait]
impl MailGateway for LiveImapSmtpGateway {
    async fn sync(
        &self,
        account_id: &AccountId,
        _cursors: &[SyncCursor],
    ) -> Result<SyncBatch, GatewayError> {
        let sync_started = Instant::now();
        let discovery = discover_imap_account(&self.config)
            .await
            .map_err(imap_error_to_gateway)?;
        let selectable_mailbox_count = discovery
            .mailboxes
            .iter()
            .filter(|mailbox| mailbox.selectable)
            .count();
        info!(
            account_id = %account_id,
            mailbox_count = discovery.mailboxes.len(),
            selectable_mailbox_count,
            supports_qresync = discovery.capabilities.supports_qresync(),
            supports_condstore = discovery.capabilities.supports_condstore(),
            supports_gmail_extensions = discovery.capabilities.supports_gmail_extensions(),
            "IMAP sync discovery complete"
        );
        let updated_at = now_iso8601().map_err(GatewayError::Rejected)?;
        let store = self.store.as_ref();
        let mut planned_mailboxes = Vec::new();
        let account_full_message_snapshot = store.is_none();
        let mut has_full_mailbox_snapshot = account_full_message_snapshot;

        for mailbox in discovery
            .mailboxes
            .iter()
            .filter(|mailbox| mailbox.selectable)
        {
            if let Some(store) = store {
                let selected = examine_imap_mailbox(&self.config, &mailbox.name)
                    .await
                    .map_err(imap_error_to_gateway)?;
                let stored_state = store
                    .get_imap_mailbox_state(account_id, &mailbox.id)
                    .map_err(store_error_to_gateway)?;
                let plan = plan_imap_mailbox_sync(
                    &discovery.capabilities,
                    stored_state.as_ref(),
                    &selected,
                );
                let local_locations = store
                    .list_imap_mailbox_message_locations(account_id, &mailbox.id)
                    .map_err(store_error_to_gateway)?;
                info!(
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
                debug!(
                    account_id = %account_id,
                    mailbox_id = %mailbox.id,
                    mailbox_name = %mailbox.name,
                    plan = imap_sync_plan_name(&plan),
                    "IMAP mailbox sync plan detail"
                );
                if matches!(plan, ImapMailboxSyncPlan::FullSnapshot { .. }) {
                    has_full_mailbox_snapshot = true;
                }
                planned_mailboxes.push(PlannedImapMailbox {
                    id: mailbox.id.clone(),
                    name: mailbox.name.clone(),
                    stored_state,
                    local_locations,
                    plan,
                });
            } else {
                info!(
                    account_id = %account_id,
                    mailbox_id = %mailbox.id,
                    plan = "full_snapshot",
                    has_stored_state = false,
                    local_message_count = 0usize,
                    "IMAP mailbox sync planned"
                );
                planned_mailboxes.push(PlannedImapMailbox {
                    id: mailbox.id.clone(),
                    name: mailbox.name.clone(),
                    stored_state: None,
                    local_locations: Vec::new(),
                    plan: ImapMailboxSyncPlan::FullSnapshot {
                        reason: posthaste_domain::ImapFullSyncReason::InitialSync,
                    },
                });
            }
        }

        let mut headers = Vec::new();
        let mut local_locations = Vec::new();
        let mut mailbox_states = Vec::new();
        let mut explicit_deleted_uids = Vec::new();
        let uses_partial_delta = planned_mailboxes.iter().any(|mailbox| {
            matches!(
                mailbox.plan,
                ImapMailboxSyncPlan::QresyncDelta { .. }
                    | ImapMailboxSyncPlan::FetchNewByUid { .. }
            )
        });
        let planned_mailbox_count = planned_mailboxes.len();

        info!(
            account_id = %account_id,
            mailbox_count = planned_mailbox_count,
            account_full_message_snapshot,
            has_full_mailbox_snapshot,
            uses_partial_delta,
            "IMAP sync fetch started"
        );

        for (mailbox_index, mailbox) in planned_mailboxes.into_iter().enumerate() {
            local_locations.extend(mailbox.local_locations.clone());

            let plan_name = imap_sync_plan_name(&mailbox.plan);
            match mailbox.plan {
                ImapMailboxSyncPlan::FullSnapshot { .. } => {
                    let started = Instant::now();
                    info!(
                        account_id = %account_id,
                        mailbox_id = %mailbox.id,
                        mailbox_index = mailbox_index + 1,
                        mailbox_count = planned_mailbox_count,
                        mode = "full_snapshot",
                        "IMAP mailbox header fetch started"
                    );
                    let snapshot = fetch_mailbox_header_snapshot(
                        &self.config,
                        &mailbox.name,
                        updated_at.clone(),
                    )
                    .await
                    .map_err(imap_error_to_gateway)?;
                    let header_count = snapshot.headers.len();
                    if !account_full_message_snapshot {
                        explicit_deleted_uids.extend(missing_location_identities(
                            &mailbox.local_locations,
                            &snapshot.headers,
                        ));
                    }
                    mailbox_states.push(imap_mailbox_state_from_header_snapshot(
                        &snapshot,
                        updated_at.clone(),
                    ));
                    headers.extend(snapshot.headers);
                    info!(
                        account_id = %account_id,
                        mailbox_id = %mailbox.id,
                        mailbox_index = mailbox_index + 1,
                        mailbox_count = planned_mailbox_count,
                        mode = "full_snapshot",
                        message_count = header_count,
                        duration_ms = started.elapsed().as_millis() as u64,
                        "IMAP mailbox header fetch completed"
                    );
                }
                ImapMailboxSyncPlan::QresyncDelta { since_modseq, .. } => {
                    let started = Instant::now();
                    info!(
                        account_id = %account_id,
                        mailbox_id = %mailbox.id,
                        mailbox_index = mailbox_index + 1,
                        mailbox_count = planned_mailbox_count,
                        mode = "qresync_delta",
                        since_modseq = since_modseq.0,
                        "IMAP mailbox header fetch started"
                    );
                    let snapshot = fetch_mailbox_changed_since_snapshot(
                        &self.config,
                        &mailbox.name,
                        since_modseq,
                        true,
                        updated_at.clone(),
                    )
                    .await
                    .map_err(imap_error_to_gateway)?;
                    let header_count = snapshot.headers.len();
                    let vanished_count = snapshot.vanished_uids.len();
                    let fetch_mode = if snapshot.is_full_snapshot {
                        "qresync_fallback_full_snapshot"
                    } else {
                        "qresync_delta"
                    };
                    if let Some(stored_state) = mailbox.stored_state.as_ref() {
                        mailbox_states.push(imap_mailbox_state_from_changed_since_snapshot(
                            stored_state,
                            &snapshot,
                            updated_at.clone(),
                        ));
                    }
                    if snapshot.is_full_snapshot {
                        explicit_deleted_uids.extend(missing_location_identities(
                            &mailbox.local_locations,
                            &snapshot.headers,
                        ));
                    } else {
                        explicit_deleted_uids.extend(
                            snapshot.vanished_uids.iter().map(|uid| {
                                (mailbox.id.clone(), snapshot.selected.uid_validity, *uid)
                            }),
                        );
                    }
                    headers.extend(snapshot.headers);
                    info!(
                        account_id = %account_id,
                        mailbox_id = %mailbox.id,
                        mailbox_index = mailbox_index + 1,
                        mailbox_count = planned_mailbox_count,
                        mode = fetch_mode,
                        message_count = header_count,
                        vanished_count,
                        duration_ms = started.elapsed().as_millis() as u64,
                        "IMAP mailbox header fetch completed"
                    );
                }
                ImapMailboxSyncPlan::CondstoreDelta { .. } => {
                    let started = Instant::now();
                    info!(
                        account_id = %account_id,
                        mailbox_id = %mailbox.id,
                        mailbox_index = mailbox_index + 1,
                        mailbox_count = planned_mailbox_count,
                        mode = plan_name,
                        "IMAP mailbox header fetch started"
                    );
                    let snapshot = fetch_mailbox_header_snapshot(
                        &self.config,
                        &mailbox.name,
                        updated_at.clone(),
                    )
                    .await
                    .map_err(imap_error_to_gateway)?;
                    let header_count = snapshot.headers.len();
                    explicit_deleted_uids.extend(missing_location_identities(
                        &mailbox.local_locations,
                        &snapshot.headers,
                    ));
                    mailbox_states.push(imap_mailbox_state_from_header_snapshot(
                        &snapshot,
                        updated_at.clone(),
                    ));
                    headers.extend(snapshot.headers);
                    info!(
                        account_id = %account_id,
                        mailbox_id = %mailbox.id,
                        mailbox_index = mailbox_index + 1,
                        mailbox_count = planned_mailbox_count,
                        mode = plan_name,
                        message_count = header_count,
                        duration_ms = started.elapsed().as_millis() as u64,
                        "IMAP mailbox header fetch completed"
                    );
                }
                ImapMailboxSyncPlan::FetchNewByUid { after_uid } => {
                    let started = Instant::now();
                    info!(
                        account_id = %account_id,
                        mailbox_id = %mailbox.id,
                        mailbox_index = mailbox_index + 1,
                        mailbox_count = planned_mailbox_count,
                        mode = "fetch_new_by_uid",
                        after_uid = after_uid.0,
                        "IMAP mailbox header fetch started"
                    );
                    let snapshot = fetch_mailbox_headers_after_uid(
                        &self.config,
                        &mailbox.name,
                        after_uid,
                        updated_at.clone(),
                    )
                    .await
                    .map_err(imap_error_to_gateway)?;
                    let header_count = snapshot.headers.len();
                    let deleted_before = explicit_deleted_uids.len();
                    explicit_deleted_uids.extend(missing_location_identities_from_uids(
                        &mailbox.local_locations,
                        &snapshot.current_uids,
                    ));
                    let deleted_uid_count = explicit_deleted_uids.len() - deleted_before;
                    if let Some(stored_state) = mailbox.stored_state.as_ref() {
                        mailbox_states.push(imap_mailbox_state_from_changed_since_snapshot(
                            stored_state,
                            &ImapChangedSinceSnapshot {
                                selected: snapshot.selected,
                                headers: snapshot.headers.clone(),
                                vanished_uids: Vec::new(),
                                is_full_snapshot: false,
                            },
                            updated_at.clone(),
                        ));
                    }
                    headers.extend(snapshot.headers);
                    info!(
                        account_id = %account_id,
                        mailbox_id = %mailbox.id,
                        mailbox_index = mailbox_index + 1,
                        mailbox_count = planned_mailbox_count,
                        mode = "fetch_new_by_uid",
                        message_count = header_count,
                        deleted_uid_count,
                        duration_ms = started.elapsed().as_millis() as u64,
                        "IMAP mailbox header fetch completed"
                    );
                }
            }
        }

        info!(
            account_id = %account_id,
            mailbox_count = planned_mailbox_count,
            message_count = headers.len(),
            deleted_uid_count = explicit_deleted_uids.len(),
            duration_ms = sync_started.elapsed().as_millis() as u64,
            "IMAP sync fetch completed"
        );

        if account_full_message_snapshot {
            Ok(imap_full_sync_batch(
                account_id,
                discovery,
                headers,
                mailbox_states,
                updated_at,
            ))
        } else if uses_partial_delta
            || !explicit_deleted_uids.is_empty()
            || has_full_mailbox_snapshot
        {
            Ok(imap_condstore_delta_sync_batch(
                account_id,
                discovery,
                headers,
                mailbox_states,
                local_locations,
                explicit_deleted_uids,
                updated_at,
            ))
        } else {
            Ok(imap_delta_sync_batch(
                account_id,
                discovery,
                headers,
                mailbox_states,
                local_locations,
                updated_at,
            ))
        }
    }

    async fn fetch_message_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError> {
        let (location, mailbox_name) = self.location_and_mailbox_name(account_id, message_id)?;

        fetch_message_body_by_location(&self.config, &mailbox_name, &location)
            .await
            .map_err(imap_error_to_gateway)
    }

    async fn download_blob(
        &self,
        account_id: &AccountId,
        blob_id: &BlobId,
    ) -> Result<Vec<u8>, GatewayError> {
        let (message_id, _attachment_index) =
            parse_imap_attachment_blob_id(blob_id).map_err(imap_error_to_gateway)?;
        let (location, mailbox_name) = self.location_and_mailbox_name(account_id, &message_id)?;
        let raw_mime = fetch_raw_message_by_location(&self.config, &mailbox_name, &location)
            .await
            .map_err(imap_error_to_gateway)?;

        imap_attachment_bytes_from_raw_mime(blob_id, raw_mime).map_err(imap_error_to_gateway)
    }

    async fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        _expected_state: Option<&str>,
        command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError> {
        let (location, mailbox_name) = self.location_and_mailbox_name(account_id, message_id)?;

        apply_imap_keyword_delta_by_location(&self.config, &mailbox_name, &location, command)
            .await
            .map_err(imap_error_to_gateway)
    }

    async fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        _expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError> {
        let store = self.store("mailbox replacement state lookup")?;
        let current_mailbox_ids = store
            .get_message_mailboxes(account_id, message_id)
            .map_err(store_error_to_gateway)?;
        let locations = store
            .list_imap_message_locations(account_id, message_id)
            .map_err(store_error_to_gateway)?;
        let delta = imap_mailbox_replacement_delta(&current_mailbox_ids, mailbox_ids);

        if let Some((source_mailbox_id, target_mailbox_id)) =
            simple_imap_move_mailboxes(&self.discovery.capabilities, &delta)
        {
            let source_location = locations
                .iter()
                .find(|location| &location.mailbox_id == source_mailbox_id)
                .ok_or_else(|| {
                    imap_error_to_gateway(ImapAdapterError::MissingMessageLocation(
                        source_mailbox_id.to_string(),
                    ))
                })?;
            let source_mailbox_name = self.mailbox_name_for_id(account_id, source_mailbox_id)?;
            let target_mailbox_name = self.mailbox_name_for_id(account_id, target_mailbox_id)?;
            move_imap_message_to_mailbox_by_location(
                &self.config,
                &source_mailbox_name,
                source_location,
                &target_mailbox_name,
            )
            .await
            .map_err(imap_error_to_gateway)?;

            return Ok(MutationOutcome { cursor: None });
        }

        let source_location = locations.first().cloned().ok_or_else(|| {
            GatewayError::Rejected(format!("missing IMAP location for message {message_id}"))
        })?;
        let source_mailbox_name =
            self.mailbox_name_for_id(account_id, &source_location.mailbox_id)?;

        for mailbox_id in &delta.add {
            let target_mailbox_name = self.mailbox_name_for_id(account_id, mailbox_id)?;
            copy_imap_message_to_mailbox_by_location(
                &self.config,
                &source_mailbox_name,
                &source_location,
                &target_mailbox_name,
            )
            .await
            .map_err(imap_error_to_gateway)?;
        }

        for mailbox_id in &delta.remove {
            let location = locations
                .iter()
                .find(|location| &location.mailbox_id == mailbox_id)
                .ok_or_else(|| {
                    imap_error_to_gateway(ImapAdapterError::MissingMessageLocation(
                        mailbox_id.to_string(),
                    ))
                })?;
            let mailbox_name = self.mailbox_name_for_id(account_id, mailbox_id)?;
            mark_imap_message_deleted_by_location(&self.config, &mailbox_name, location)
                .await
                .map_err(imap_error_to_gateway)?;
        }

        Ok(MutationOutcome { cursor: None })
    }

    async fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        _expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError> {
        let locations = self
            .store("message deletion state lookup")?
            .list_imap_message_locations(account_id, message_id)
            .map_err(store_error_to_gateway)?;
        if locations.is_empty() {
            return Err(GatewayError::Rejected(format!(
                "missing IMAP location for message {message_id}"
            )));
        }

        for location in &locations {
            let mailbox_name = self.mailbox_name_for_id(account_id, &location.mailbox_id)?;
            if self.discovery.capabilities.supports_uidplus() {
                expunge_imap_message_by_location(&self.config, &mailbox_name, location)
                    .await
                    .map_err(imap_error_to_gateway)?;
            } else {
                mark_imap_message_deleted_by_location(&self.config, &mailbox_name, location)
                    .await
                    .map_err(imap_error_to_gateway)?;
            }
        }

        Ok(MutationOutcome { cursor: None })
    }

    async fn set_mailbox_role(
        &self,
        _account_id: &AccountId,
        _mailbox_id: &MailboxId,
        _expected_state: Option<&str>,
        _role: Option<&str>,
        _clear_role_from: Option<&MailboxId>,
    ) -> Result<MutationOutcome, GatewayError> {
        Err(unsupported("mailbox role mutation"))
    }

    async fn fetch_identity(&self, _account_id: &AccountId) -> Result<Identity, GatewayError> {
        Ok(Identity {
            id: "imap-smtp-default".to_string(),
            name: self.smtp_config.sender_name.clone().unwrap_or_else(|| {
                self.smtp_config
                    .sender_email
                    .split('@')
                    .next()
                    .unwrap_or(self.smtp_config.sender_email.as_str())
                    .to_string()
            }),
            email: self.smtp_config.sender_email.clone(),
        })
    }

    async fn fetch_reply_context(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<ReplyContext, GatewayError> {
        let (location, mailbox_name) = self.location_and_mailbox_name(account_id, message_id)?;

        fetch_imap_reply_context_by_location(&self.config, &mailbox_name, &location)
            .await
            .map_err(imap_error_to_gateway)
    }

    async fn send_message(
        &self,
        _account_id: &AccountId,
        request: &SendMessageRequest,
    ) -> Result<(), GatewayError> {
        let submitted = submit_smtp_message(&self.smtp_config, request)
            .await
            .map_err(imap_error_to_gateway)?;

        if smtp_sent_copy_strategy(&self.smtp_config.provider)
            == SmtpSentCopyStrategy::AppendToSentMailbox
        {
            if let Some(sent_mailbox) = self
                .discovery
                .mailboxes
                .iter()
                .find(|mailbox| mailbox.selectable && mailbox.role == Some("sent"))
            {
                if let Err(error) =
                    append_smtp_sent_copy(&self.config, &sent_mailbox.name, &submitted.raw_message)
                        .await
                {
                    warn!(
                        mailbox = sent_mailbox.name,
                        error = %error,
                        "SMTP send accepted but IMAP Sent copy append failed"
                    );
                }
            } else {
                warn!("SMTP send accepted but no selectable IMAP Sent mailbox was discovered");
            }
        }

        Ok(())
    }

    fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
        Vec::new()
    }
}

fn missing_location_identities(
    local_locations: &[ImapMessageLocation],
    remote_headers: &[ImapMappedHeader],
) -> Vec<(MailboxId, ImapUidValidity, ImapUid)> {
    let remote_locations = remote_headers
        .iter()
        .map(|header| {
            (
                header.location.mailbox_id.clone(),
                header.location.uid_validity.0,
                header.location.uid,
            )
        })
        .collect::<std::collections::BTreeSet<_>>();

    local_locations
        .iter()
        .filter(|location| {
            !remote_locations.contains(&(
                location.mailbox_id.clone(),
                location.uid_validity.0,
                location.uid,
            ))
        })
        .map(|location| {
            (
                location.mailbox_id.clone(),
                location.uid_validity,
                location.uid,
            )
        })
        .collect()
}

fn missing_location_identities_from_uids(
    local_locations: &[ImapMessageLocation],
    remote_uids: &[ImapUid],
) -> Vec<(MailboxId, ImapUidValidity, ImapUid)> {
    let remote_uids = remote_uids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();

    local_locations
        .iter()
        .filter(|location| !remote_uids.contains(&location.uid))
        .map(|location| {
            (
                location.mailbox_id.clone(),
                location.uid_validity,
                location.uid,
            )
        })
        .collect()
}

fn imap_sync_plan_name(plan: &ImapMailboxSyncPlan) -> &'static str {
    match plan {
        ImapMailboxSyncPlan::FullSnapshot { .. } => "full_snapshot",
        ImapMailboxSyncPlan::FetchNewByUid { .. } => "fetch_new_by_uid",
        ImapMailboxSyncPlan::CondstoreDelta { .. } => "condstore_delta",
        ImapMailboxSyncPlan::QresyncDelta { .. } => "qresync_delta",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fetch_identity_uses_configured_sender_identity() {
        let gateway = LiveImapSmtpGateway {
            config: test_config(),
            smtp_config: test_smtp_config(),
            discovery: DiscoveredImapAccount {
                capabilities: posthaste_domain::ImapCapabilities::default(),
                mailboxes: Vec::new(),
            },
            store: None,
        };

        let identity = gateway
            .fetch_identity(&AccountId::from("primary"))
            .await
            .expect("identity");

        assert_eq!(identity.email, "alice@example.test");
        assert_eq!(identity.name, "Alice Example");
    }

    #[test]
    fn names_imap_sync_plans_for_logs() {
        assert_eq!(
            imap_sync_plan_name(&ImapMailboxSyncPlan::FullSnapshot {
                reason: posthaste_domain::ImapFullSyncReason::InitialSync,
            }),
            "full_snapshot"
        );
        assert_eq!(
            imap_sync_plan_name(&ImapMailboxSyncPlan::FetchNewByUid {
                after_uid: ImapUid(42),
            }),
            "fetch_new_by_uid"
        );
        assert_eq!(
            imap_sync_plan_name(&ImapMailboxSyncPlan::CondstoreDelta {
                since_modseq: posthaste_domain::ImapModSeq(9),
                after_uid: None,
            }),
            "condstore_delta"
        );
        assert_eq!(
            imap_sync_plan_name(&ImapMailboxSyncPlan::QresyncDelta {
                uid_validity: ImapUidValidity(1),
                since_modseq: posthaste_domain::ImapModSeq(9),
                after_uid: None,
            }),
            "qresync_delta"
        );
    }

    #[test]
    fn detects_missing_uid_locations_from_current_uid_listing() {
        let mailbox_id = MailboxId::from("imap:mailbox:inbox");
        let kept = ImapMessageLocation {
            message_id: MessageId::from("message-kept"),
            mailbox_id: mailbox_id.clone(),
            uid_validity: ImapUidValidity(7),
            uid: ImapUid(10),
            modseq: None,
            updated_at: "2026-04-27T00:00:00Z".to_string(),
        };
        let missing = ImapMessageLocation {
            message_id: MessageId::from("message-missing"),
            uid: ImapUid(11),
            ..kept.clone()
        };

        let deleted = missing_location_identities_from_uids(&[kept, missing], &[ImapUid(10)]);

        assert_eq!(deleted, vec![(mailbox_id, ImapUidValidity(7), ImapUid(11))]);
    }

    #[tokio::test]
    async fn fetch_body_reports_clear_unsupported_error() {
        let gateway = LiveImapSmtpGateway {
            config: test_config(),
            smtp_config: test_smtp_config(),
            discovery: DiscoveredImapAccount {
                capabilities: posthaste_domain::ImapCapabilities::default(),
                mailboxes: Vec::new(),
            },
            store: None,
        };

        let error = gateway
            .fetch_message_body(&AccountId::from("primary"), &MessageId::from("message"))
            .await
            .expect_err("body fetch is not implemented");

        assert!(matches!(error, GatewayError::Rejected(message) if message.contains("discovery")));
    }

    #[test]
    fn simple_move_uses_uid_move_when_server_supports_move() {
        let delta = crate::ImapMailboxReplacementDelta {
            add: vec![MailboxId::from("archive")],
            remove: vec![MailboxId::from("inbox")],
        };

        let planned = simple_imap_move_mailboxes(&ImapCapabilities::from_tokens(["MOVE"]), &delta)
            .map(|(source, target)| (source.clone(), target.clone()));

        assert_eq!(
            planned,
            Some((MailboxId::from("inbox"), MailboxId::from("archive")))
        );
    }

    #[test]
    fn simple_move_falls_back_when_move_is_unavailable() {
        let delta = crate::ImapMailboxReplacementDelta {
            add: vec![MailboxId::from("archive")],
            remove: vec![MailboxId::from("inbox")],
        };

        let planned =
            simple_imap_move_mailboxes(&ImapCapabilities::from_tokens(["IMAP4rev1"]), &delta);

        assert!(planned.is_none());
    }

    #[test]
    fn simple_move_does_not_apply_to_copy_or_multi_mailbox_changes() {
        let copy_delta = crate::ImapMailboxReplacementDelta {
            add: vec![MailboxId::from("archive")],
            remove: Vec::new(),
        };
        let multi_delta = crate::ImapMailboxReplacementDelta {
            add: vec![MailboxId::from("archive"), MailboxId::from("project")],
            remove: vec![MailboxId::from("inbox")],
        };
        let capabilities = ImapCapabilities::from_tokens(["MOVE", "UIDPLUS"]);

        assert!(simple_imap_move_mailboxes(&capabilities, &copy_delta).is_none());
        assert!(simple_imap_move_mailboxes(&capabilities, &multi_delta).is_none());
    }

    fn test_config() -> ImapConnectionConfig {
        ImapConnectionConfig {
            host: "imap.example.test".to_string(),
            port: 993,
            security: posthaste_domain::TransportSecurity::Tls,
            username: "alice@example.test".to_string(),
            secret: "secret".to_string(),
            auth: posthaste_domain::ProviderAuthKind::Password,
        }
    }

    fn test_smtp_config() -> SmtpConnectionConfig {
        SmtpConnectionConfig {
            host: "smtp.example.test".to_string(),
            port: 587,
            security: posthaste_domain::TransportSecurity::StartTls,
            sender_name: Some("Alice Example".to_string()),
            sender_email: "alice@example.test".to_string(),
            username: "alice-login".to_string(),
            secret: "secret".to_string(),
            auth: posthaste_domain::ProviderAuthKind::Password,
            provider: posthaste_domain::ProviderHint::Generic,
        }
    }
}

fn imap_error_to_gateway(error: ImapAdapterError) -> GatewayError {
    match error {
        ImapAdapterError::MissingTransport
        | ImapAdapterError::MissingSmtpTransport
        | ImapAdapterError::MissingUsername
        | ImapAdapterError::MissingSmtpSenderEmail
        | ImapAdapterError::MissingSecret
        | ImapAdapterError::InvalidMailboxName(_)
        | ImapAdapterError::MissingSelectData(_)
        | ImapAdapterError::UidValidityMismatch { .. }
        | ImapAdapterError::MissingFetchData(_)
        | ImapAdapterError::InvalidUidSequence(_)
        | ImapAdapterError::InvalidModSeq(_)
        | ImapAdapterError::InvalidKeywordFlag { .. }
        | ImapAdapterError::MissingMessageLocation(_)
        | ImapAdapterError::InvalidBlobId(_)
        | ImapAdapterError::ParseMessageHeaders
        | ImapAdapterError::ParseMessageBody
        | ImapAdapterError::MissingAttachment { .. }
        | ImapAdapterError::InvalidSmtpAddress { .. }
        | ImapAdapterError::BuildSmtpMessage(_) => GatewayError::Rejected(error.to_string()),
        ImapAdapterError::Client(message) | ImapAdapterError::Smtp(message) => {
            GatewayError::Network(message)
        }
    }
}

fn store_error_to_gateway(error: StoreError) -> GatewayError {
    GatewayError::Rejected(format!("IMAP local state lookup failed: {error}"))
}
