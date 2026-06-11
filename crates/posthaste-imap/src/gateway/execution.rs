use super::*;

pub(crate) struct MailboxPlanExecution<'a> {
    pub(crate) account_id: &'a AccountId,
    pub(crate) mailbox_ordinal: usize,
    pub(crate) mailbox_count: usize,
    pub(crate) fetch_modseq: bool,
    pub(crate) fetch_gmail_metadata: bool,
    pub(crate) account_full_message_snapshot: bool,
    pub(crate) updated_at: &'a str,
    pub(crate) progress: &'a Option<SyncProgressReporter>,
}

#[derive(Clone, Copy)]
pub(crate) struct MailboxPlanExecutionContext<'a> {
    pub(crate) account_id: &'a AccountId,
    pub(crate) fetch_modseq: bool,
    pub(crate) fetch_gmail_metadata: bool,
    pub(crate) account_full_message_snapshot: bool,
    pub(crate) updated_at: &'a str,
    pub(crate) progress: &'a Option<SyncProgressReporter>,
}

impl<'a> MailboxPlanExecutionContext<'a> {
    pub(crate) fn for_mailbox(
        self,
        mailbox_ordinal: usize,
        mailbox_count: usize,
    ) -> MailboxPlanExecution<'a> {
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

pub(crate) async fn execute_mailbox_plans(
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

pub(crate) async fn execute_mailbox_plan(
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
