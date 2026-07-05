use super::*;

pub(crate) async fn sync_imap_account(
    gateway: &LiveImapSmtpGateway,
    account_id: &AccountId,
    progress: Option<SyncProgressReporter>,
) -> Result<SyncBatch, GatewayError> {
    // One lease for the whole cycle: the sync borrows the account's single
    // reused session (D92/O3) instead of opening its own connection. An
    // in-flight IDLE hold is recalled by the acquire; a connection-fatal
    // failure inside the cycle drops the session so the next use reconnects.
    let mut lease = gateway
        .sessions
        .acquire("sync")
        .await
        .map_err(imap_error_to_gateway)?;
    let result = sync_imap_account_with_client(gateway, lease.client(), account_id, progress).await;
    lease.finish_gateway(result)
}

/// Shared sync prelude: refresh capabilities on the live session, re-plan every
/// selectable mailbox from stored per-mailbox state, and compute the fetch
/// flags. Produced once and consumed by both the batch path
/// ([`sync_imap_account_with_client`]) and the streamed resumable-initial-sync
/// path ([`super::sync_imap_account_streamed`]).
pub(crate) struct PlannedSync {
    pub(crate) discovery: DiscoveredImapAccount,
    pub(crate) planned_mailboxes: Vec<PlannedImapMailbox>,
    pub(crate) fetch_modseq: bool,
    pub(crate) fetch_gmail_metadata: bool,
    pub(crate) account_full_message_snapshot: bool,
    pub(crate) updated_at: String,
    pub(crate) sync_started: Instant,
}

pub(crate) async fn prepare_planned_sync(
    gateway: &LiveImapSmtpGateway,
    client: &mut ImapClient,
    account_id: &AccountId,
    progress: &Option<SyncProgressReporter>,
) -> Result<PlannedSync, GatewayError> {
    let sync_started = Instant::now();
    report_sync_progress(
        progress,
        ImapSyncProgressUpdate::new(SyncProgressStage::Discovering, "Checking IMAP capabilities"),
    );
    let mut discovery = gateway.discovery.clone();
    crate::timeout::with_deadline("refresh_capabilities", client.refresh_capabilities())
        .await
        .map_err(imap_error_to_gateway)?;
    discovery.capabilities = normalize_imap_capabilities(
        client
            .state
            .capabilities_iter()
            .map(std::string::ToString::to_string),
    );

    let fetch_modseq = discovery.capabilities.supports_condstore();
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
        capabilities = %discovery.capabilities.joined(),
        "IMAP sync discovery complete"
    );
    let updated_at = now_iso8601().map_err(GatewayError::Rejected)?;
    let store = gateway.store.as_deref();
    let account_full_message_snapshot = store.is_none();
    let planned_mailboxes = plan_mailboxes(
        client,
        account_id,
        &discovery,
        store,
        selectable_mailbox_count,
        progress,
    )
    .await?;

    Ok(PlannedSync {
        discovery,
        planned_mailboxes,
        fetch_modseq,
        fetch_gmail_metadata,
        account_full_message_snapshot,
        updated_at,
        sync_started,
    })
}

async fn sync_imap_account_with_client(
    gateway: &LiveImapSmtpGateway,
    client: &mut ImapClient,
    account_id: &AccountId,
    progress: Option<SyncProgressReporter>,
) -> Result<SyncBatch, GatewayError> {
    let PlannedSync {
        discovery,
        planned_mailboxes,
        fetch_modseq,
        fetch_gmail_metadata,
        account_full_message_snapshot,
        updated_at,
        sync_started,
    } = prepare_planned_sync(gateway, client, account_id, &progress).await?;
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
        client,
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
