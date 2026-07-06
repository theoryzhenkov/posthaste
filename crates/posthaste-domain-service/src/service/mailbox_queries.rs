use super::*;

impl MailService {
    /// List all mailboxes for an account. Counts come straight from canonical:
    /// the `message_mailbox`/`is_read` triggers maintain the denormalized mailbox
    /// counters, and S2 writes optimism through to those rows, so the sidebar
    /// reflects pending assertions with no overlay (folding again would
    /// double-count).
    pub fn list_mailboxes(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<MailboxSummary>, ServiceError> {
        self.mailbox_reader
            .list_mailboxes(account_id)
            .map_err(Into::into)
    }

    /// List user-facing tags for one account.
    pub fn list_tags(&self, account_id: &AccountId) -> Result<Vec<TagSummary>, ServiceError> {
        self.tag_reader.list_tags(account_id).map_err(Into::into)
    }

    /// List user-facing tags merged across the provided accounts.
    pub fn list_merged_tags(
        &self,
        account_ids: &[AccountId],
    ) -> Result<Vec<TagSummary>, ServiceError> {
        let mut tag_totals = std::collections::BTreeMap::<String, (i64, i64)>::new();
        for account_id in account_ids {
            for tag in self.tag_reader.list_tags(account_id)? {
                let entry = tag_totals.entry(tag.name).or_insert((0, 0));
                entry.0 += tag.unread_messages;
                entry.1 += tag.total_messages;
            }
        }
        Ok(tag_totals
            .into_iter()
            .map(|(name, (unread_messages, total_messages))| TagSummary {
                name,
                unread_messages,
                total_messages,
            })
            .collect())
    }

    /// Update server-side mailbox metadata and refresh the local mailbox projection.
    ///
    /// @spec docs/L1-api#conversations-and-messages
    /// @spec docs/L1-jmap#methods-used
    pub async fn set_mailbox_role(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
        role: Option<&str>,
        gateway: &dyn MailGateway,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let expected_state = self
            .sync_state
            .get_cursor(account_id, SyncObject::Mailbox)?;
        let clear_role_from = match role {
            Some(role) => self
                .mailbox_reader
                .list_mailboxes(account_id)?
                .into_iter()
                .find(|mailbox| mailbox.id != *mailbox_id && mailbox.role.as_deref() == Some(role))
                .map(|mailbox| mailbox.id),
            None => None,
        };
        if is_local_only_role(role) {
            // Posthaste-specific roles (e.g. "snooze") have no provider
            // equivalent — JMAP/IMAP reject them as a mailbox `role`. Write the
            // local override (which updates `mailbox.role` directly) + skip the
            // gateway round-trip. @spec docs/eph/DESIGN-L2-snooze
            self.mailbox_role_overrides.set_mailbox_role_override(
                account_id,
                mailbox_id,
                role,
                clear_role_from.as_ref(),
            )?;
            let event = self.events.append_event(
                account_id,
                EVENT_TOPIC_MAILBOX_UPDATED,
                Some(mailbox_id),
                None,
                json!({ "mailboxId": mailbox_id.as_str() }),
            )?;
            return Ok(vec![event]);
        }
        gateway
            .set_mailbox_role(
                account_id,
                mailbox_id,
                expected_state.as_ref().map(|cursor| cursor.state.as_str()),
                role,
                clear_role_from.as_ref(),
            )
            .await?;
        self.sync_account(account_id, SyncTrigger::Manual, gateway, None)
            .await
    }

    /// Create a new top-level mailbox on the provider and refresh the local
    /// mailbox projection so it surfaces in the sidebar.
    ///
    /// Mailbox mutations are synchronous, not optimistic (mirroring
    /// [`set_mailbox_role`](Self::set_mailbox_role)): a blocking provider
    /// round-trip creates the mailbox, then a resync reads it back. Flat create
    /// only — no parent (nesting is out of scope).
    ///
    /// @spec docs/eph/RFC-L2-mailbox-management
    pub async fn create_mailbox(
        &self,
        account_id: &AccountId,
        name: &str,
        gateway: &dyn MailGateway,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let mailbox_id = gateway.create_mailbox(account_id, name).await?;
        let created_event = self.events.append_event(
            account_id,
            EVENT_TOPIC_MAILBOX_UPDATED,
            Some(&mailbox_id),
            None,
            json!({ "mailboxId": mailbox_id.as_str() }),
        )?;
        let mut events = vec![created_event];
        events.extend(
            self.sync_account(account_id, SyncTrigger::Manual, gateway, None)
                .await?,
        );
        Ok(events)
    }

    /// Destroy a server-side mailbox, then resync so its disappearance tears down
    /// the local rows (the resync-observed-deletion path reuses the store's
    /// `mailbox_cleanup` teardown — `message_mailbox` + `imap_message_location` +
    /// the `mailbox` row).
    ///
    /// **The M2 safety gate (un-bypassable from the API):** a NON-EMPTY mailbox is
    /// refused unless the caller explicitly confirms `remove_emails`. The mailbox's
    /// `total_emails` is read from the local projection *before* the gateway is
    /// touched; when `total_emails > 0 && !remove_emails` this returns
    /// [`GatewayError::MailboxNotEmpty`] (→ 409) WITHOUT calling the gateway, so a
    /// REST `DELETE` without `removeEmails` can never destroy a non-empty mailbox.
    /// Only an empty mailbox, or one with the confirmed flag, proceeds to the
    /// blocking `gateway.destroy_mailbox` + resync.
    ///
    /// Synchronous, not optimistic (mirroring
    /// [`create_mailbox`](Self::create_mailbox)).
    ///
    /// @spec docs/eph/RFC-L2-mailbox-management
    pub async fn destroy_mailbox(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
        remove_emails: bool,
        gateway: &dyn MailGateway,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        // Read the canonical count BEFORE any gateway round-trip: the gate must
        // hold on the local projection so a non-empty destroy without the
        // confirmed flag never reaches the provider.
        let mailbox = self
            .mailbox_reader
            .list_mailboxes(account_id)?
            .into_iter()
            .find(|mailbox| mailbox.id == *mailbox_id)
            .ok_or_else(|| {
                StoreError::NotFound(format!("mailbox {} not found", mailbox_id.as_str()))
            })?;
        if mailbox.total_emails > 0 && !remove_emails {
            return Err(ServiceError::Gateway(GatewayError::MailboxNotEmpty {
                count: mailbox.total_emails,
            }));
        }

        gateway
            .destroy_mailbox(account_id, mailbox_id, remove_emails)
            .await?;
        let removed_event = self.events.append_event(
            account_id,
            EVENT_TOPIC_MAILBOX_UPDATED,
            Some(mailbox_id),
            None,
            json!({ "mailboxId": mailbox_id.as_str(), "deleted": true }),
        )?;
        let mut events = vec![removed_event];
        events.extend(
            self.sync_account(account_id, SyncTrigger::Manual, gateway, None)
                .await?,
        );
        Ok(events)
    }
}

/// Whether `role` is a provider-native mailbox role (one JMAP/IMAP accepts on
/// the mailbox itself), vs a Posthaste-local role like "snooze".
fn is_provider_role(role: &str) -> bool {
    matches!(
        posthaste_domain_model::MailboxRole::parse(role),
        Some(role) if role != posthaste_domain_model::MailboxRole::Snooze
    )
}

/// Whether `role` is Posthaste-local (no provider equivalent) — the gateway
/// round-trip is skipped + the local override is written instead. `None`
/// (clearing) is NOT local-only: clearing a provider role still goes through the
/// gateway.
fn is_local_only_role(role: Option<&str>) -> bool {
    role.map(|r| !is_provider_role(r)).unwrap_or(false)
}
