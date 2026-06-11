use super::*;

impl MessageCommandStore for TestStore {
    fn set_keywords(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        cursor: Option<&SyncCursor>,
        command: &SetKeywordsCommand,
    ) -> Result<CommandResult, StoreError> {
        self.keyword_adds
            .lock()
            .expect("keyword adds lock poisoned")
            .push((message_id.clone(), command.add.clone()));
        if let Some(cursor) = cursor {
            self.mutation_state
                .lock()
                .expect("mutation state lock poisoned")
                .cursor = Some(cursor.clone());
        }
        Ok(CommandResult {
            detail: None,
            events: Vec::new(),
        })
    }

    fn replace_mailboxes(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        cursor: Option<&SyncCursor>,
        command: &ReplaceMailboxesCommand,
    ) -> Result<CommandResult, StoreError> {
        let mut state = self
            .mutation_state
            .lock()
            .expect("mutation state lock poisoned");
        state.mailbox_ids = command.mailbox_ids.clone();
        if let Some(cursor) = cursor {
            state.cursor = Some(cursor.clone());
        }
        Ok(CommandResult {
            detail: None,
            events: Vec::new(),
        })
    }

    fn destroy_message(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        cursor: Option<&SyncCursor>,
    ) -> Result<CommandResult, StoreError> {
        let mut state = self
            .mutation_state
            .lock()
            .expect("mutation state lock poisoned");
        state.mailbox_ids.clear();
        if let Some(cursor) = cursor {
            state.cursor = Some(cursor.clone());
        }
        Ok(CommandResult {
            detail: None,
            events: Vec::new(),
        })
    }
}

impl EventStore for TestStore {
    fn list_events(&self, _filter: &EventFilter) -> Result<Vec<DomainEvent>, StoreError> {
        Ok(Vec::new())
    }

    fn append_event(
        &self,
        account_id: &AccountId,
        topic: &str,
        mailbox_id: Option<&MailboxId>,
        message_id: Option<&MessageId>,
        payload: serde_json::Value,
    ) -> Result<DomainEvent, StoreError> {
        Ok(DomainEvent {
            seq: 1,
            account_id: account_id.clone(),
            topic: topic.to_string(),
            occurred_at: crate::RFC3339_EPOCH.to_string(),
            mailbox_id: mailbox_id.cloned(),
            message_id: message_id.cloned(),
            payload,
        })
    }
}

impl SourceProjectionStore for TestStore {
    fn upsert_source_projection(
        &self,
        source_id: &AccountId,
        _name: &str,
    ) -> Result<(), StoreError> {
        self.projection_calls
            .lock()
            .expect("projection lock poisoned")
            .push(source_id.to_string());
        Ok(())
    }

    fn delete_source_projection(&self, source_id: &AccountId) -> Result<(), StoreError> {
        self.projection_deletes
            .lock()
            .expect("projection deletes lock poisoned")
            .push(source_id.to_string());
        Ok(())
    }
}

impl SourceDataStore for TestStore {
    fn delete_source_data(&self, account_id: &AccountId) -> Result<(), StoreError> {
        self.source_data_deletes
            .lock()
            .expect("source data deletes lock poisoned")
            .push(account_id.to_string());
        Ok(())
    }
}

impl SenderAddressCacheStore for TestStore {
    fn list_sender_address_cache(&self) -> Result<Vec<CachedSenderAddress>, StoreError> {
        Ok(Vec::new())
    }

    fn remember_sender_address(
        &self,
        _account_id: &AccountId,
        _sender: &Recipient,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}
