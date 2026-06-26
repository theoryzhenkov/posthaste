use super::*;

pub(crate) async fn sync_imap_account(
    gateway: &LiveImapSmtpGateway,
    account_id: &AccountId,
    progress: Option<SyncProgressReporter>,
) -> Result<SyncBatch, GatewayError> {
    let sync_started = Instant::now();
    report_sync_progress(
        &progress,
        ImapSyncProgressUpdate::new(SyncProgressStage::Discovering, "Checking IMAP capabilities"),
    );
    let mut discovery = gateway.discovery.clone();
    let imap_config = gateway.resolve_imap_config().await?;
    let mut client = connect_authenticated_client(&imap_config)
        .await
        .map_err(imap_error_to_gateway)?;
    client
        .refresh_capabilities()
        .await
        .map_err(ImapAdapterError::from)
        .map_err(imap_error_to_gateway)?;

    // Use the capabilities advertised on this connection for planning, not the
    // cached discovery snapshot. CONDSTORE/QRESYNC may only appear post-auth, or
    // the server's advertisement may change between discovery and sync.
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
