use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use imap_client::client::tokio::Client as ImapClient;
use posthaste_domain::{
    now_iso8601, plan_imap_mailbox_sync, plan_imap_move, AccountId, BlobId, FetchedBody,
    GatewayError, Identity, ImapCapabilities, ImapMailboxSyncPlan, ImapMailboxSyncState,
    ImapMessageLocation, ImapMoveStrategy, ImapUid, ImapUidValidity, MailGateway, MailStore,
    MailboxId, MessageId, MutationOutcome, ProviderProfile, PushTransport, ReplyContext,
    SendMessageRequest, SetKeywordsCommand, StoreError, SyncBatch, SyncCursor, SyncProgress,
    SyncProgressReporter, SyncProgressStage, SyncTrigger,
};
use posthaste_observability::{events, ph_debug, ph_info, ph_warn};

use crate::discovery::connect_authenticated_client;
use crate::fetch::{
    fetch_mailbox_changed_since_snapshot_with_client, fetch_mailbox_header_snapshot_with_client,
    fetch_mailbox_headers_after_uid_with_client,
};
use crate::mailbox::{examine_selected_mailbox, status_imap_mailbox, ImapMailboxStatus};
use crate::{
    append_smtp_sent_copy, apply_imap_keyword_delta_by_location,
    copy_imap_message_to_mailbox_by_location, discover_imap_account,
    expunge_imap_message_by_location, fetch_imap_reply_context_by_location,
    fetch_message_body_by_location, fetch_raw_message_by_location,
    imap_attachment_bytes_from_raw_mime, imap_condstore_delta_sync_batch, imap_delta_sync_batch,
    imap_full_sync_batch, imap_mailbox_replacement_delta,
    imap_mailbox_state_from_changed_since_snapshot, imap_mailbox_state_from_header_snapshot,
    mark_imap_message_deleted_by_location, move_imap_message_to_mailbox_by_location,
    normalize_imap_capabilities, parse_imap_attachment_blob_id, smtp_sent_copy_strategy,
    submit_smtp_message, DiscoveredImapAccount, DiscoveredImapMailbox, ImapAdapterError,
    ImapChangedSinceSnapshot, ImapConnectionConfig, ImapMailboxHeaderSnapshot,
    ImapMailboxUidDeltaSnapshot, ImapMappedHeader, SmtpConnectionConfig, SmtpSentCopyStrategy,
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
    plan: PlannedImapMailboxSync,
}

enum PlannedImapMailboxSync {
    SkipUnchanged,
    Sync(ImapMailboxSyncPlan),
}

impl PlannedImapMailboxSync {
    fn requires_partial_delta_batch(&self) -> bool {
        matches!(
            self,
            Self::SkipUnchanged
                | Self::Sync(ImapMailboxSyncPlan::QresyncDelta { .. })
                | Self::Sync(ImapMailboxSyncPlan::FetchNewByUid { .. })
        )
    }

    fn is_full_snapshot(&self) -> bool {
        matches!(self, Self::Sync(ImapMailboxSyncPlan::FullSnapshot { .. }))
    }
}

#[derive(Default)]
struct SyncBatchAccumulator {
    headers: Vec<ImapMappedHeader>,
    local_locations: Vec<ImapMessageLocation>,
    mailbox_states: Vec<ImapMailboxSyncState>,
    explicit_deleted_uids: Vec<(MailboxId, ImapUidValidity, ImapUid)>,
}

struct ChangedSinceRecordSummary {
    header_count: usize,
    vanished_count: usize,
    fetch_mode: &'static str,
}

struct UidDeltaRecordSummary {
    header_count: usize,
    deleted_uid_count: usize,
}

impl SyncBatchAccumulator {
    fn add_local_locations(&mut self, locations: &[ImapMessageLocation]) {
        self.local_locations.extend(locations.iter().cloned());
    }

    fn add_deleted_uid_identities(
        &mut self,
        identities: impl IntoIterator<Item = (MailboxId, ImapUidValidity, ImapUid)>,
    ) {
        self.explicit_deleted_uids.extend(identities);
    }

    fn record_header_snapshot(
        &mut self,
        snapshot: ImapMailboxHeaderSnapshot,
        updated_at: &str,
    ) -> usize {
        let header_count = snapshot.headers.len();
        self.mailbox_states
            .push(imap_mailbox_state_from_header_snapshot(
                &snapshot,
                updated_at.to_string(),
            ));
        self.headers.extend(snapshot.headers);
        header_count
    }

    fn record_changed_since_snapshot(
        &mut self,
        mailbox: &PlannedImapMailbox,
        snapshot: ImapChangedSinceSnapshot,
        updated_at: &str,
    ) -> ChangedSinceRecordSummary {
        let header_count = snapshot.headers.len();
        let vanished_count = snapshot.vanished_uids.len();
        let fetch_mode = if snapshot.is_full_snapshot {
            "qresync_fallback_full_snapshot"
        } else {
            "qresync_delta"
        };

        if let Some(stored_state) = mailbox.stored_state.as_ref() {
            self.mailbox_states
                .push(imap_mailbox_state_from_changed_since_snapshot(
                    stored_state,
                    &snapshot,
                    updated_at.to_string(),
                ));
        }
        if snapshot.is_full_snapshot {
            self.add_deleted_uid_identities(missing_location_identities(
                &mailbox.local_locations,
                &snapshot.headers,
            ));
        } else {
            self.add_deleted_uid_identities(
                snapshot
                    .vanished_uids
                    .iter()
                    .map(|uid| (mailbox.id.clone(), snapshot.selected.uid_validity, *uid)),
            );
        }
        self.headers.extend(snapshot.headers);

        ChangedSinceRecordSummary {
            header_count,
            vanished_count,
            fetch_mode,
        }
    }

    fn record_uid_delta_snapshot(
        &mut self,
        mailbox: &PlannedImapMailbox,
        snapshot: ImapMailboxUidDeltaSnapshot,
        updated_at: &str,
    ) -> UidDeltaRecordSummary {
        let header_count = snapshot.headers.len();
        let deleted_before = self.explicit_deleted_uids.len();
        self.add_deleted_uid_identities(missing_location_identities_from_uids(
            &mailbox.local_locations,
            &snapshot.current_uids,
        ));
        let deleted_uid_count = self.explicit_deleted_uids.len() - deleted_before;
        if let Some(stored_state) = mailbox.stored_state.as_ref() {
            self.mailbox_states
                .push(imap_mailbox_state_from_changed_since_snapshot(
                    stored_state,
                    &ImapChangedSinceSnapshot {
                        selected: snapshot.selected.clone(),
                        headers: snapshot.headers.clone(),
                        vanished_uids: Vec::new(),
                        is_full_snapshot: false,
                    },
                    updated_at.to_string(),
                ));
        }
        self.headers.extend(snapshot.headers);

        UidDeltaRecordSummary {
            header_count,
            deleted_uid_count,
        }
    }

    fn message_count(&self) -> usize {
        self.headers.len()
    }

    fn deleted_uid_count(&self) -> usize {
        self.explicit_deleted_uids.len()
    }

    fn into_sync_batch(
        self,
        account_id: &AccountId,
        discovery: DiscoveredImapAccount,
        account_full_message_snapshot: bool,
        requires_partial_delta_batch: bool,
        has_full_mailbox_snapshot: bool,
        updated_at: String,
    ) -> SyncBatch {
        let use_explicit_deletion_batch = requires_partial_delta_batch
            || !self.explicit_deleted_uids.is_empty()
            || has_full_mailbox_snapshot;
        let Self {
            headers,
            local_locations,
            mailbox_states,
            explicit_deleted_uids,
        } = self;

        if account_full_message_snapshot {
            imap_full_sync_batch(account_id, discovery, headers, mailbox_states, updated_at)
        } else if use_explicit_deletion_batch {
            imap_condstore_delta_sync_batch(
                account_id,
                discovery,
                headers,
                mailbox_states,
                local_locations,
                explicit_deleted_uids,
                updated_at,
            )
        } else {
            imap_delta_sync_batch(
                account_id,
                discovery,
                headers,
                mailbox_states,
                local_locations,
                updated_at,
            )
        }
    }
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

async fn plan_mailboxes(
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

async fn plan_mailbox(
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

fn planned_mailboxes_include_full_snapshot(planned_mailboxes: &[PlannedImapMailbox]) -> bool {
    planned_mailboxes
        .iter()
        .any(|mailbox| mailbox.plan.is_full_snapshot())
}

fn planned_mailboxes_require_partial_delta_batch(planned_mailboxes: &[PlannedImapMailbox]) -> bool {
    planned_mailboxes
        .iter()
        .any(|mailbox| mailbox.plan.requires_partial_delta_batch())
}

struct MailboxPlanExecution<'a> {
    account_id: &'a AccountId,
    mailbox_ordinal: usize,
    mailbox_count: usize,
    fetch_modseq: bool,
    fetch_gmail_metadata: bool,
    account_full_message_snapshot: bool,
    updated_at: &'a str,
    progress: &'a Option<SyncProgressReporter>,
}

#[derive(Clone, Copy)]
struct MailboxPlanExecutionContext<'a> {
    account_id: &'a AccountId,
    fetch_modseq: bool,
    fetch_gmail_metadata: bool,
    account_full_message_snapshot: bool,
    updated_at: &'a str,
    progress: &'a Option<SyncProgressReporter>,
}

impl<'a> MailboxPlanExecutionContext<'a> {
    fn for_mailbox(self, mailbox_ordinal: usize, mailbox_count: usize) -> MailboxPlanExecution<'a> {
        MailboxPlanExecution {
            account_id: self.account_id,
            mailbox_ordinal,
            mailbox_count,
            fetch_modseq: self.fetch_modseq,
            fetch_gmail_metadata: self.fetch_gmail_metadata,
            account_full_message_snapshot: self.account_full_message_snapshot,
            updated_at: self.updated_at,
            progress: self.progress,
        }
    }
}

async fn execute_mailbox_plans(
    client: &mut ImapClient,
    planned_mailboxes: Vec<PlannedImapMailbox>,
    context: MailboxPlanExecutionContext<'_>,
) -> Result<SyncBatchAccumulator, GatewayError> {
    let mailbox_count = planned_mailboxes.len();
    let mut accumulator = SyncBatchAccumulator::default();

    for (mailbox_index, mailbox) in planned_mailboxes.into_iter().enumerate() {
        execute_mailbox_plan(
            client,
            mailbox,
            context.for_mailbox(mailbox_index + 1, mailbox_count),
            &mut accumulator,
        )
        .await?;
    }

    Ok(accumulator)
}

async fn execute_mailbox_plan(
    client: &mut ImapClient,
    mailbox: PlannedImapMailbox,
    execution: MailboxPlanExecution<'_>,
    accumulator: &mut SyncBatchAccumulator,
) -> Result<(), GatewayError> {
    accumulator.add_local_locations(&mailbox.local_locations);

    let plan_name = planned_imap_sync_plan_name(&mailbox.plan);
    match &mailbox.plan {
        PlannedImapMailboxSync::SkipUnchanged => {
            report_sync_progress(
                execution.progress,
                ImapSyncProgressUpdate::new(SyncProgressStage::Fetching, "Mailbox unchanged")
                    .with_mailbox(
                        mailbox.name.clone(),
                        execution.mailbox_ordinal,
                        execution.mailbox_count,
                    )
                    .with_message_count(0),
            );
            ph_info!(
                events::IMAP_MAILBOX_HEADER_FETCH_COMPLETED,
                account_id = %execution.account_id,
                mailbox_id = %mailbox.id,
                mailbox_index = execution.mailbox_ordinal,
                mailbox_count = execution.mailbox_count,
                mode = "skip_unchanged",
                message_count = 0usize,
                deleted_uid_count = 0usize,
                duration_ms = 0u64,
                "IMAP mailbox header fetch completed"
            );
        }
        PlannedImapMailboxSync::Sync(ImapMailboxSyncPlan::FullSnapshot { .. }) => {
            let started = Instant::now();
            report_sync_progress(
                execution.progress,
                ImapSyncProgressUpdate::new(SyncProgressStage::Fetching, "Fetching mailbox")
                    .with_mailbox(
                        mailbox.name.clone(),
                        execution.mailbox_ordinal,
                        execution.mailbox_count,
                    ),
            );
            ph_info!(
                events::IMAP_MAILBOX_HEADER_FETCH_STARTED,
                account_id = %execution.account_id,
                mailbox_id = %mailbox.id,
                mailbox_index = execution.mailbox_ordinal,
                mailbox_count = execution.mailbox_count,
                mode = "full_snapshot",
                "IMAP mailbox header fetch started"
            );
            let snapshot = fetch_mailbox_header_snapshot_with_client(
                client,
                &mailbox.name,
                execution.fetch_modseq,
                execution.fetch_gmail_metadata,
                execution.updated_at.to_string(),
            )
            .await
            .map_err(imap_error_to_gateway)?;
            if !execution.account_full_message_snapshot {
                accumulator.add_deleted_uid_identities(missing_location_identities(
                    &mailbox.local_locations,
                    &snapshot.headers,
                ));
            }
            let header_count = accumulator.record_header_snapshot(snapshot, execution.updated_at);
            ph_info!(
                events::IMAP_MAILBOX_HEADER_FETCH_COMPLETED,
                account_id = %execution.account_id,
                mailbox_id = %mailbox.id,
                mailbox_index = execution.mailbox_ordinal,
                mailbox_count = execution.mailbox_count,
                mode = "full_snapshot",
                message_count = header_count,
                duration_ms = started.elapsed().as_millis() as u64,
                "IMAP mailbox header fetch completed"
            );
        }
        PlannedImapMailboxSync::Sync(ImapMailboxSyncPlan::QresyncDelta {
            since_modseq, ..
        }) => {
            let started = Instant::now();
            report_sync_progress(
                execution.progress,
                ImapSyncProgressUpdate::new(
                    SyncProgressStage::Fetching,
                    "Fetching mailbox changes",
                )
                .with_mailbox(
                    mailbox.name.clone(),
                    execution.mailbox_ordinal,
                    execution.mailbox_count,
                ),
            );
            ph_info!(
                events::IMAP_MAILBOX_HEADER_FETCH_STARTED,
                account_id = %execution.account_id,
                mailbox_id = %mailbox.id,
                mailbox_index = execution.mailbox_ordinal,
                mailbox_count = execution.mailbox_count,
                mode = "qresync_delta",
                since_modseq = since_modseq.0,
                "IMAP mailbox header fetch started"
            );
            let snapshot = fetch_mailbox_changed_since_snapshot_with_client(
                client,
                &mailbox.name,
                *since_modseq,
                true,
                execution.fetch_gmail_metadata,
                execution.updated_at.to_string(),
            )
            .await
            .map_err(imap_error_to_gateway)?;
            let summary =
                accumulator.record_changed_since_snapshot(&mailbox, snapshot, execution.updated_at);
            ph_info!(
                events::IMAP_MAILBOX_HEADER_FETCH_COMPLETED,
                account_id = %execution.account_id,
                mailbox_id = %mailbox.id,
                mailbox_index = execution.mailbox_ordinal,
                mailbox_count = execution.mailbox_count,
                mode = summary.fetch_mode,
                message_count = summary.header_count,
                vanished_count = summary.vanished_count,
                duration_ms = started.elapsed().as_millis() as u64,
                "IMAP mailbox header fetch completed"
            );
        }
        PlannedImapMailboxSync::Sync(ImapMailboxSyncPlan::CondstoreDelta { .. }) => {
            let started = Instant::now();
            report_sync_progress(
                execution.progress,
                ImapSyncProgressUpdate::new(
                    SyncProgressStage::Fetching,
                    "Fetching mailbox changes",
                )
                .with_mailbox(
                    mailbox.name.clone(),
                    execution.mailbox_ordinal,
                    execution.mailbox_count,
                ),
            );
            ph_info!(
                events::IMAP_MAILBOX_HEADER_FETCH_STARTED,
                account_id = %execution.account_id,
                mailbox_id = %mailbox.id,
                mailbox_index = execution.mailbox_ordinal,
                mailbox_count = execution.mailbox_count,
                mode = plan_name,
                "IMAP mailbox header fetch started"
            );
            let snapshot = fetch_mailbox_header_snapshot_with_client(
                client,
                &mailbox.name,
                execution.fetch_modseq,
                execution.fetch_gmail_metadata,
                execution.updated_at.to_string(),
            )
            .await
            .map_err(imap_error_to_gateway)?;
            accumulator.add_deleted_uid_identities(missing_location_identities(
                &mailbox.local_locations,
                &snapshot.headers,
            ));
            let header_count = accumulator.record_header_snapshot(snapshot, execution.updated_at);
            ph_info!(
                events::IMAP_MAILBOX_HEADER_FETCH_COMPLETED,
                account_id = %execution.account_id,
                mailbox_id = %mailbox.id,
                mailbox_index = execution.mailbox_ordinal,
                mailbox_count = execution.mailbox_count,
                mode = plan_name,
                message_count = header_count,
                duration_ms = started.elapsed().as_millis() as u64,
                "IMAP mailbox header fetch completed"
            );
        }
        PlannedImapMailboxSync::Sync(ImapMailboxSyncPlan::FetchNewByUid { after_uid }) => {
            let started = Instant::now();
            report_sync_progress(
                execution.progress,
                ImapSyncProgressUpdate::new(
                    SyncProgressStage::Fetching,
                    "Checking mailbox for new messages",
                )
                .with_mailbox(
                    mailbox.name.clone(),
                    execution.mailbox_ordinal,
                    execution.mailbox_count,
                ),
            );
            ph_info!(
                events::IMAP_MAILBOX_HEADER_FETCH_STARTED,
                account_id = %execution.account_id,
                mailbox_id = %mailbox.id,
                mailbox_index = execution.mailbox_ordinal,
                mailbox_count = execution.mailbox_count,
                mode = "fetch_new_by_uid",
                after_uid = after_uid.0,
                "IMAP mailbox header fetch started"
            );
            let snapshot = fetch_mailbox_headers_after_uid_with_client(
                client,
                &mailbox.name,
                *after_uid,
                execution.fetch_modseq,
                execution.fetch_gmail_metadata,
                execution.updated_at.to_string(),
            )
            .await
            .map_err(imap_error_to_gateway)?;
            let summary =
                accumulator.record_uid_delta_snapshot(&mailbox, snapshot, execution.updated_at);
            ph_info!(
                events::IMAP_MAILBOX_HEADER_FETCH_COMPLETED,
                account_id = %execution.account_id,
                mailbox_id = %mailbox.id,
                mailbox_index = execution.mailbox_ordinal,
                mailbox_count = execution.mailbox_count,
                mode = "fetch_new_by_uid",
                message_count = summary.header_count,
                deleted_uid_count = summary.deleted_uid_count,
                duration_ms = started.elapsed().as_millis() as u64,
                "IMAP mailbox header fetch completed"
            );
        }
    }

    Ok(())
}

#[async_trait]
impl MailGateway for LiveImapSmtpGateway {
    async fn sync(
        &self,
        account_id: &AccountId,
        _cursors: &[SyncCursor],
        progress: Option<SyncProgressReporter>,
    ) -> Result<SyncBatch, GatewayError> {
        let sync_started = Instant::now();
        report_sync_progress(
            &progress,
            ImapSyncProgressUpdate::new(
                SyncProgressStage::Discovering,
                "Checking IMAP capabilities",
            ),
        );
        let discovery = self.discovery.clone();
        let mut client = connect_authenticated_client(&self.config)
            .await
            .map_err(imap_error_to_gateway)?;
        client
            .refresh_capabilities()
            .await
            .map_err(ImapAdapterError::from)
            .map_err(imap_error_to_gateway)?;
        let fetch_modseq = normalize_imap_capabilities(
            client
                .state
                .capabilities_iter()
                .map(std::string::ToString::to_string),
        )
        .supports_condstore();
        let fetch_gmail_metadata = discovery.capabilities.supports_gmail_extensions();
        let selectable_mailbox_count = discovery
            .mailboxes
            .iter()
            .filter(|mailbox| mailbox.selectable)
            .count();
        ph_info!(
            events::IMAP_SYNC_DISCOVERY_COMPLETED,
            account_id = %account_id,
            mailbox_count = discovery.mailboxes.len(),
            selectable_mailbox_count,
            supports_qresync = discovery.capabilities.supports_qresync(),
            supports_condstore = discovery.capabilities.supports_condstore(),
            supports_gmail_extensions = discovery.capabilities.supports_gmail_extensions(),
            "IMAP sync discovery complete"
        );
        let updated_at = now_iso8601().map_err(GatewayError::Rejected)?;
        let store = self.store.as_deref();
        let account_full_message_snapshot = store.is_none();
        let planned_mailboxes = plan_mailboxes(
            &mut client,
            account_id,
            &discovery,
            store,
            selectable_mailbox_count,
            &progress,
        )
        .await?;
        let has_full_mailbox_snapshot = account_full_message_snapshot
            || planned_mailboxes_include_full_snapshot(&planned_mailboxes);
        let requires_partial_delta_batch =
            planned_mailboxes_require_partial_delta_batch(&planned_mailboxes);
        let planned_mailbox_count = planned_mailboxes.len();

        ph_info!(
            events::IMAP_SYNC_FETCH_STARTED,
            account_id = %account_id,
            mailbox_count = planned_mailbox_count,
            account_full_message_snapshot,
            has_full_mailbox_snapshot,
            uses_partial_delta = requires_partial_delta_batch,
            "IMAP sync fetch started"
        );
        report_sync_progress(
            &progress,
            ImapSyncProgressUpdate::new(SyncProgressStage::Fetching, "Fetching mailbox changes")
                .with_mailbox_count(planned_mailbox_count),
        );

        let accumulator = execute_mailbox_plans(
            &mut client,
            planned_mailboxes,
            MailboxPlanExecutionContext {
                account_id,
                fetch_modseq,
                fetch_gmail_metadata,
                account_full_message_snapshot,
                updated_at: &updated_at,
                progress: &progress,
            },
        )
        .await?;

        ph_info!(
            events::IMAP_SYNC_FETCH_COMPLETED,
            account_id = %account_id,
            mailbox_count = planned_mailbox_count,
            message_count = accumulator.message_count(),
            deleted_uid_count = accumulator.deleted_uid_count(),
            duration_ms = sync_started.elapsed().as_millis() as u64,
            "IMAP sync fetch completed"
        );

        Ok(accumulator.into_sync_batch(
            account_id,
            discovery,
            account_full_message_snapshot,
            requires_partial_delta_batch,
            has_full_mailbox_snapshot,
            updated_at,
        ))
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
        account_id: &AccountId,
        mailbox_id: &MailboxId,
        _expected_state: Option<&str>,
        role: Option<&str>,
        clear_role_from: Option<&MailboxId>,
    ) -> Result<MutationOutcome, GatewayError> {
        self.store("mailbox role override")?
            .set_mailbox_role_override(account_id, mailbox_id, role, clear_role_from)
            .map_err(store_error_to_gateway)?;

        Ok(MutationOutcome { cursor: None })
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
                    ph_warn!(
                        events::IMAP_SMTP_SENT_APPEND_FAILED,
                        mailbox = sent_mailbox.name,
                        error = %error,
                        "SMTP send accepted but IMAP Sent copy append failed"
                    );
                }
            } else {
                ph_warn!(
                    events::IMAP_SMTP_SENT_MAILBOX_MISSING,
                    "SMTP send accepted but no selectable IMAP Sent mailbox was discovered"
                );
            }
        }

        Ok(())
    }

    fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
        Vec::new()
    }
}

/// Project a local message location to its (mailbox, uid-validity, uid) identity tuple.
fn location_identity(location: &ImapMessageLocation) -> (MailboxId, ImapUidValidity, ImapUid) {
    (
        location.mailbox_id.clone(),
        location.uid_validity,
        location.uid,
    )
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
        .map(location_identity)
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
        .map(location_identity)
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

fn planned_imap_sync_plan_name(plan: &PlannedImapMailboxSync) -> &'static str {
    match plan {
        PlannedImapMailboxSync::SkipUnchanged => "skip_unchanged",
        PlannedImapMailboxSync::Sync(plan) => imap_sync_plan_name(plan),
    }
}

struct ImapSyncProgressUpdate {
    stage: SyncProgressStage,
    detail: &'static str,
    mailbox_name: Option<String>,
    mailbox_index: Option<usize>,
    mailbox_count: Option<usize>,
    message_count: Option<usize>,
    total_count: Option<usize>,
}

impl ImapSyncProgressUpdate {
    fn new(stage: SyncProgressStage, detail: &'static str) -> Self {
        Self {
            stage,
            detail,
            mailbox_name: None,
            mailbox_index: None,
            mailbox_count: None,
            message_count: None,
            total_count: None,
        }
    }

    fn with_mailbox_count(mut self, mailbox_count: usize) -> Self {
        self.mailbox_count = Some(mailbox_count);
        self
    }

    fn with_mailbox(
        mut self,
        mailbox_name: String,
        mailbox_index: usize,
        mailbox_count: usize,
    ) -> Self {
        self.mailbox_name = Some(mailbox_name);
        self.mailbox_index = Some(mailbox_index);
        self.mailbox_count = Some(mailbox_count);
        self
    }

    fn with_message_count(mut self, message_count: usize) -> Self {
        self.message_count = Some(message_count);
        self
    }
}

fn report_sync_progress(reporter: &Option<SyncProgressReporter>, update: ImapSyncProgressUpdate) {
    if let Some(reporter) = reporter {
        reporter.report(SyncProgress {
            sync_id: String::new(),
            trigger: SyncTrigger::Manual,
            started_at: String::new(),
            stage: update.stage,
            detail: update.detail.to_string(),
            mailbox_name: update.mailbox_name,
            mailbox_index: update.mailbox_index,
            mailbox_count: update.mailbox_count,
            message_count: update.message_count,
            total_count: update.total_count,
        });
    }
}

fn mailbox_status_proves_unchanged(
    stored: &ImapMailboxSyncState,
    local_message_count: usize,
    status: &ImapMailboxStatus,
) -> bool {
    if status.uid_validity != Some(stored.uid_validity) {
        return false;
    }
    if status.messages != Some(local_message_count as u32) {
        return false;
    }
    if let Some(stored_modseq) = stored.highest_modseq {
        return status.highest_modseq == Some(stored_modseq);
    }

    false
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

#[cfg(test)]
mod tests;
