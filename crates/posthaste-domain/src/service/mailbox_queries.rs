use super::*;

impl MailService {
    /// List all mailboxes for an account.
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
}
