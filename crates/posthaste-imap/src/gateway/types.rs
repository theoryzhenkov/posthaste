use super::*;

pub struct LiveImapSmtpGateway {
    pub(crate) smtp_config: SmtpConnectionConfig,
    pub(crate) discovery: DiscoveredImapAccount,
    pub(crate) store: Option<Arc<dyn MailStore>>,
    pub(crate) secret_resolver: Arc<dyn SecretResolver>,
    /// Owner of the single reused authenticated IMAP session (D92/O3). Every
    /// IMAP operation borrows the session from here instead of opening its
    /// own connection; secrets are resolved by the manager at (re)connect.
    pub(crate) sessions: Arc<ImapSessionManager>,
}

impl LiveImapSmtpGateway {
    /// Resolve the current SMTP secret immediately before opening a connection.
    pub(crate) async fn resolve_smtp_config(&self) -> Result<SmtpConnectionConfig, GatewayError> {
        let secret = self.secret_resolver.resolve_secret().await?;
        let mut config = self.smtp_config.clone();
        config.secret = secret;
        Ok(config)
    }
}

pub(crate) struct PlannedImapMailbox {
    pub(crate) id: MailboxId,
    pub(crate) name: String,
    pub(crate) stored_state: Option<ImapMailboxSyncState>,
    pub(crate) local_locations: Vec<ImapMessageLocation>,
    pub(crate) plan: PlannedImapMailboxSync,
}

pub(crate) enum PlannedImapMailboxSync {
    SkipUnchanged,
    Sync(ImapMailboxSyncPlan),
}

impl PlannedImapMailboxSync {
    pub(crate) fn requires_partial_delta_batch(&self) -> bool {
        matches!(
            self,
            Self::SkipUnchanged
                | Self::Sync(ImapMailboxSyncPlan::QresyncDelta { .. })
                | Self::Sync(ImapMailboxSyncPlan::FetchNewByUid { .. })
        )
    }

    pub(crate) fn is_full_snapshot(&self) -> bool {
        matches!(self, Self::Sync(ImapMailboxSyncPlan::FullSnapshot { .. }))
    }
}

#[derive(Default)]
pub(crate) struct SyncBatchAccumulator {
    pub(crate) headers: Vec<ImapMappedHeader>,
    pub(crate) local_locations: Vec<ImapMessageLocation>,
    pub(crate) mailbox_states: Vec<ImapMailboxSyncState>,
    pub(crate) explicit_deleted_uids: Vec<(MailboxId, ImapUidValidity, ImapUid)>,
}

pub(crate) struct ChangedSinceRecordSummary {
    pub(crate) header_count: usize,
    pub(crate) vanished_count: usize,
    pub(crate) fetch_mode: &'static str,
}

pub(crate) struct UidDeltaRecordSummary {
    pub(crate) header_count: usize,
    pub(crate) deleted_uid_count: usize,
}

impl SyncBatchAccumulator {
    pub(crate) fn add_local_locations(&mut self, locations: &[ImapMessageLocation]) {
        self.local_locations.extend(locations.iter().cloned());
    }

    pub(crate) fn add_deleted_uid_identities(
        &mut self,
        identities: impl IntoIterator<Item = (MailboxId, ImapUidValidity, ImapUid)>,
    ) {
        self.explicit_deleted_uids.extend(identities);
    }

    pub(crate) fn record_header_snapshot(
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

    pub(crate) fn record_changed_since_snapshot(
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

    pub(crate) fn record_uid_delta_snapshot(
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

    pub(crate) fn message_count(&self) -> usize {
        self.headers.len()
    }

    pub(crate) fn deleted_uid_count(&self) -> usize {
        self.explicit_deleted_uids.len()
    }

    pub(crate) fn into_sync_batch(
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
