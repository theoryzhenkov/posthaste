use super::*;

impl MailboxReadStore for TestStore {
    fn list_mailboxes(&self, _account_id: &AccountId) -> Result<Vec<MailboxSummary>, StoreError> {
        if let Some(error) = self.list_mailboxes_error.as_ref() {
            return Err(StoreError::Failure(error.clone()));
        }
        // Derive base (unfolded) counts from the projected rule page so the
        // overlay delta can be applied on top in tests, mirroring production.
        let messages = self.rule_page.lock().expect("rule page lock poisoned");
        let counts = |mailbox: &str| {
            let total = messages
                .iter()
                .filter(|m| m.mailbox_ids.iter().any(|id| id.as_str() == mailbox))
                .count() as i64;
            let unread = messages
                .iter()
                .filter(|m| m.mailbox_ids.iter().any(|id| id.as_str() == mailbox) && !m.is_read)
                .count() as i64;
            (unread, total)
        };
        let (inbox_unread, inbox_total) = counts("inbox");
        let (archive_unread, archive_total) = counts("archive");
        Ok(vec![
            MailboxSummary {
                id: MailboxId::from("inbox"),
                name: "Inbox".to_string(),
                role: Some("inbox".to_string()),
                unread_emails: inbox_unread,
                total_emails: inbox_total,
            },
            MailboxSummary {
                id: MailboxId::from("archive"),
                name: "Archive".to_string(),
                role: Some("archive".to_string()),
                unread_emails: archive_unread,
                total_emails: archive_total,
            },
        ])
    }
}

impl MailboxRoleOverrideStore for TestStore {
    fn set_mailbox_role_override(
        &self,
        _account_id: &AccountId,
        _mailbox_id: &MailboxId,
        _role: Option<&str>,
        _clear_role_from: Option<&MailboxId>,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

impl MessageListStore for TestStore {
    fn list_messages(
        &self,
        _account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        if let Some(error) = &self.messages_error {
            return Err(StoreError::Failure(error.clone()));
        }
        let messages = self.rule_page.lock().expect("rule page lock poisoned");
        Ok(messages
            .iter()
            .filter(|summary| {
                mailbox_id.is_none_or(|mailbox_id| summary.mailbox_ids.contains(mailbox_id))
            })
            .cloned()
            .collect())
    }

    fn list_message_page(
        &self,
        _account_id: &AccountId,
        _mailbox_id: Option<&MailboxId>,
        _limit: usize,
        _cursor: Option<&MessageCursor>,
        _sort_field: MessageSortField,
        _sort_direction: SortDirection,
    ) -> Result<MessagePage, StoreError> {
        Ok(MessagePage {
            items: Vec::new(),
            next_cursor: None,
        })
    }
}

impl TagReadStore for TestStore {
    fn list_tags(&self, _account_id: &AccountId) -> Result<Vec<TagSummary>, StoreError> {
        Ok(Vec::new())
    }
}

impl SmartMailboxStore for TestStore {
    fn query_messages_by_rule(
        &self,
        _rule: &MailQueryRule,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        Ok(Vec::new())
    }

    fn query_messages_by_rule_sorted(
        &self,
        _rule: &MailQueryRule,
        _sort_field: MessageSortField,
        _sort_direction: SortDirection,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        Ok(self
            .rule_page
            .lock()
            .expect("rule page lock poisoned")
            .clone())
    }

    fn query_message_page_by_rule(
        &self,
        _rule: &MailQueryRule,
        limit: usize,
        _cursor: Option<&MessageCursor>,
        _sort_field: MessageSortField,
        _sort_direction: SortDirection,
    ) -> Result<MessagePage, StoreError> {
        let items = self
            .rule_page
            .lock()
            .expect("rule page lock poisoned")
            .iter()
            .take(limit)
            .cloned()
            .collect();
        Ok(MessagePage {
            items,
            next_cursor: None,
        })
    }

    fn query_conversations_by_rule(
        &self,
        _rule: &MailQueryRule,
        _limit: usize,
        _cursor: Option<&ConversationCursor>,
        _sort_field: ConversationSortField,
        _sort_direction: SortDirection,
    ) -> Result<ConversationPage, StoreError> {
        Ok(ConversationPage {
            items: Vec::new(),
            next_cursor: None,
        })
    }

    fn query_smart_mailbox_counts(&self, _rule: &MailQueryRule) -> Result<(i64, i64), StoreError> {
        self.smart_mailbox_counts_error
            .as_ref()
            .map_or(Ok((1, 2)), |error| Err(StoreError::Failure(error.clone())))
    }
}

impl ConversationReadStore for TestStore {
    fn list_conversations(
        &self,
        _account_id: Option<&AccountId>,
        _mailbox_id: Option<&MailboxId>,
        _limit: usize,
        _cursor: Option<&ConversationCursor>,
        _sort_field: ConversationSortField,
        _sort_direction: SortDirection,
    ) -> Result<ConversationPage, StoreError> {
        Ok(ConversationPage {
            items: Vec::new(),
            next_cursor: None,
        })
    }

    fn get_conversation(
        &self,
        _conversation_id: &ConversationId,
    ) -> Result<Option<ConversationView>, StoreError> {
        Ok(self
            .conversation_view
            .lock()
            .expect("conversation view lock poisoned")
            .clone())
    }
}

impl MessageDetailStore for TestStore {
    fn get_message_detail(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, StoreError> {
        Ok(None)
    }

    /// EFFECTIVE-read mock (NS1): the overlay wins (a tombstone reads as
    /// gone), else base — the same merge the real `_effective` views serve.
    fn get_message_summary(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageSummary>, StoreError> {
        if let Some(entry) = self
            .overlay_rows
            .lock()
            .expect("overlay rows lock poisoned")
            .get(message_id.as_str())
        {
            return Ok(entry.as_ref().map(record_to_summary));
        }
        Ok(self
            .read_base_message_record(account_id, message_id)?
            .as_ref()
            .map(record_to_summary))
    }

    fn get_thread(
        &self,
        _account_id: &AccountId,
        _thread_id: &ThreadId,
    ) -> Result<Option<ThreadView>, StoreError> {
        Ok(None)
    }
}

/// Mock-grade record→summary projection for the effective-read mock.
fn record_to_summary(record: &MessageRecord) -> MessageSummary {
    MessageSummary {
        id: record.id.clone(),
        source_id: AccountId::from("primary"),
        source_name: "Primary".to_string(),
        source_thread_id: record.source_thread_id.clone(),
        conversation_id: posthaste_domain_model::ConversationId::from(record.id.as_str()),
        subject: record.subject.clone(),
        from_name: record.from_name.clone(),
        from_email: record.from_email.clone(),
        to: record.to.clone(),
        preview: record.preview.clone(),
        received_at: record.received_at.clone(),
        has_attachment: record.has_attachment,
        is_read: record.keywords.iter().any(|keyword| keyword == "$seen"),
        is_flagged: record.keywords.iter().any(|keyword| keyword == "$flagged"),
        mailbox_ids: record.mailbox_ids.clone(),
        keywords: record.keywords.clone(),
        version: None,
        rfc_message_id: record.rfc_message_id.clone(),
        in_reply_to: record.in_reply_to.clone(),
        draft_id: record.draft_id.clone(),
    }
}

impl SyncStateStore for TestStore {
    fn get_sync_cursors(&self, _account_id: &AccountId) -> Result<Vec<SyncCursor>, StoreError> {
        Ok(self
            .mutation_state
            .lock()
            .expect("mutation state lock poisoned")
            .cursor
            .clone()
            .into_iter()
            .collect())
    }

    fn get_cursor(
        &self,
        _account_id: &AccountId,
        object_type: SyncObject,
    ) -> Result<Option<SyncCursor>, StoreError> {
        if object_type == SyncObject::Message {
            return Ok(self
                .mutation_state
                .lock()
                .expect("mutation state lock poisoned")
                .cursor
                .clone());
        }
        Ok(None)
    }
}

impl MessageMailboxStore for TestStore {
    fn get_message_mailboxes(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<Vec<MailboxId>, StoreError> {
        Ok(self
            .mutation_state
            .lock()
            .expect("mutation state lock poisoned")
            .mailbox_ids
            .clone())
    }
}

impl ImapSyncStateStore for TestStore {
    fn list_imap_mailbox_states(
        &self,
        _account_id: &AccountId,
    ) -> Result<Vec<ImapMailboxSyncState>, StoreError> {
        Ok(Vec::new())
    }

    fn get_imap_mailbox_state(
        &self,
        _account_id: &AccountId,
        _mailbox_id: &MailboxId,
    ) -> Result<Option<ImapMailboxSyncState>, StoreError> {
        Ok(None)
    }
}

impl ImapMessageLocationStore for TestStore {
    fn list_imap_message_locations(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<Vec<ImapMessageLocation>, StoreError> {
        Ok(Vec::new())
    }

    fn list_imap_mailbox_message_locations(
        &self,
        _account_id: &AccountId,
        _mailbox_id: &MailboxId,
    ) -> Result<Vec<ImapMessageLocation>, StoreError> {
        Ok(Vec::new())
    }
}

/// The IMAP location write side is part of the `MailStore` composite (the draft
/// save path registers the appended UID's location under its canonical id, D128).
/// The domain-service tests don't observe the location store, so the double is a
/// no-op writer.
impl ImapMessageLocationWriteStore for TestStore {
    fn put_imap_message_location(
        &self,
        _account_id: &AccountId,
        _location: &ImapMessageLocation,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    fn delete_imap_message_locations(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

/// Phase 2: the `RevLog` store is part of the `MailStore` composite (a supertrait
/// of `MailStore`). The domain service tests don't exercise undo/redo history, so
/// the test double stubs the three methods as empty/no-op (an empty snapshot,
/// a no-op append, a no-op cursor set). @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
impl RevLogStore for TestStore {
    fn rev_log_snapshot(&self, _account_id: &AccountId) -> Result<RevLogSnapshot, StoreError> {
        Ok(RevLogSnapshot::default())
    }

    fn append_rev_log_step(
        &self,
        _account_id: &AccountId,
        _step_id: &str,
        _message_id: &str,
        _source_id: &str,
        _diff: &serde_json::Value,
        _created_at: &str,
    ) -> Result<u32, StoreError> {
        Ok(0)
    }

    fn set_rev_cursor(
        &self,
        _account_id: &AccountId,
        _cursor_step_id: Option<&str>,
        _redo_tail: &[String],
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

impl SnoozeStore for TestStore {
    fn insert_snooze(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        until: i64,
    ) -> Result<(), StoreError> {
        let mut snoozes = self.snoozes.lock().expect("snoozes lock poisoned");
        if let Some(entry) = snoozes.iter_mut().find(|(id, _)| id == message_id) {
            entry.1 = until; // upsert — replace the until on conflict
        } else {
            snoozes.push((message_id.clone(), until));
        }
        Ok(())
    }

    fn delete_snooze(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), StoreError> {
        let mut snoozes = self.snoozes.lock().expect("snoozes lock poisoned");
        snoozes.retain(|(id, _)| id != message_id);
        Ok(())
    }

    fn list_due_snoozes(
        &self,
        _account_id: &AccountId,
        now: i64,
    ) -> Result<Vec<(MessageId, i64)>, StoreError> {
        let snoozes = self.snoozes.lock().expect("snoozes lock poisoned");
        Ok(snoozes
            .iter()
            .filter(|(_, until)| *until <= now)
            .cloned()
            .collect())
    }
}
