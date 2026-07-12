use super::*;

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
            occurred_at: posthaste_domain_model::RFC3339_EPOCH.to_string(),
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

impl OperationOutboxStore for TestStore {
    fn enqueue_operation(&self, operation: &Operation) -> Result<Operation, StoreError> {
        let mut ops = self.outbox_operations.lock().expect("outbox lock poisoned");
        if !ops.iter().any(|existing| existing.id == operation.id) {
            ops.push(operation.clone());
        }
        ops.iter()
            .find(|existing| existing.id == operation.id)
            .cloned()
            .ok_or_else(|| StoreError::Failure("operation not persisted".to_string()))
    }

    fn list_flushable_operations(
        &self,
        account_id: &AccountId,
        wall_now: &str,
        mono_now: i64,
    ) -> Result<Vec<Operation>, StoreError> {
        let ops = self.outbox_operations.lock().expect("outbox lock poisoned");
        Ok(ops
            .iter()
            .filter(|op| {
                &op.account_id == account_id
                    && op.state.is_flushable()
                    // Mirrors the SQL two-clock gates (D152): send-later by
                    // wall, undo holds by the monotonic deadline.
                    && op.send_at.as_deref().is_none_or(|send_at| send_at <= wall_now)
                    && op.hold_until_mono.is_none_or(|hold| hold <= mono_now)
            })
            .cloned()
            .collect())
    }

    fn count_due_scheduled_sends(
        &self,
        account_id: &AccountId,
        wall_now: &str,
        mono_now: i64,
    ) -> Result<u64, StoreError> {
        let ops = self.outbox_operations.lock().expect("outbox lock poisoned");
        Ok(ops
            .iter()
            .filter(|op| {
                &op.account_id == account_id
                    && op.state.is_flushable()
                    && (op
                        .send_at
                        .as_deref()
                        .is_some_and(|send_at| send_at <= wall_now)
                        || op.hold_until_mono.is_some_and(|hold| hold <= mono_now))
            })
            .count() as u64)
    }

    fn replace_operation_payload(
        &self,
        id: &OperationId,
        payload: &serde_json::Value,
    ) -> Result<bool, StoreError> {
        let mut ops = self.outbox_operations.lock().expect("outbox lock poisoned");
        for op in ops.iter_mut() {
            if &op.id == id && op.state == OperationState::Pending {
                op.payload = payload.clone();
                op.attempts = 0;
                op.last_error = None;
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn list_pending_operations(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<Operation>, StoreError> {
        let ops = self.outbox_operations.lock().expect("outbox lock poisoned");
        Ok(ops
            .iter()
            .filter(|op| {
                &op.account_id == account_id && !matches!(op.state, OperationState::Applied)
            })
            .cloned()
            .collect())
    }

    fn list_unsettled_operations(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<Operation>, StoreError> {
        let ops = self.outbox_operations.lock().expect("outbox lock poisoned");
        Ok(ops
            .iter()
            .filter(|op| {
                &op.account_id == account_id && !matches!(op.state, OperationState::Failed)
            })
            .cloned()
            .collect())
    }

    fn get_operation(&self, id: &OperationId) -> Result<Option<Operation>, StoreError> {
        let ops = self.outbox_operations.lock().expect("outbox lock poisoned");
        Ok(ops.iter().find(|op| &op.id == id).cloned())
    }

    fn update_operation_state(
        &self,
        id: &OperationId,
        state: OperationState,
        attempts: u32,
        last_error: Option<&str>,
    ) -> Result<(), StoreError> {
        let mut ops = self.outbox_operations.lock().expect("outbox lock poisoned");
        if let Some(op) = ops.iter_mut().find(|op| &op.id == id) {
            op.state = state;
            op.attempts = attempts;
            op.last_error = last_error.map(str::to_string);
        }
        Ok(())
    }

    fn reconcile_operation_entity_id(
        &self,
        account_id: &AccountId,
        from_entity_id: &str,
        to_entity_id: &str,
    ) -> Result<(), StoreError> {
        let mut ops = self.outbox_operations.lock().expect("outbox lock poisoned");
        for op in ops.iter_mut() {
            if &op.account_id == account_id && op.entity.id == from_entity_id {
                op.entity.id = to_entity_id.to_string();
            }
        }
        Ok(())
    }

    fn remove_operation(&self, id: &OperationId) -> Result<(), StoreError> {
        let mut ops = self.outbox_operations.lock().expect("outbox lock poisoned");
        ops.retain(|op| &op.id != id);
        Ok(())
    }

    fn claim_operation_for_flush(&self, id: &OperationId) -> Result<bool, StoreError> {
        // Mirrors the real store's guarded conditional UPDATE: the mutex is the
        // serialization point, the state predicate and the write are one
        // critical section.
        let mut ops = self.outbox_operations.lock().expect("outbox lock poisoned");
        match ops
            .iter_mut()
            .find(|op| &op.id == id && op.state.is_flushable())
        {
            Some(op) => {
                op.state = OperationState::Inflight;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn remove_operation_unless_inflight(&self, id: &OperationId) -> Result<bool, StoreError> {
        // Mirrors the real store's guarded conditional DELETE (one critical
        // section with the not-inflight predicate).
        let mut ops = self.outbox_operations.lock().expect("outbox lock poisoned");
        let before = ops.len();
        ops.retain(|op| !(&op.id == id && op.state != OperationState::Inflight));
        Ok(ops.len() != before)
    }
}

/// M68/M69: the draft-identity methods behind the `DraftRegistry` port. Since
/// M69 (D135) the registry is the single authority — resolution is one lookup,
/// mirroring the real store's fallback-free `resolve_draft_entity` (sync keeps
/// the real table fresh via in-transaction write-through).
impl DraftRegistry for TestStore {
    fn resolve_draft_entity(
        &self,
        account_id: &AccountId,
        draft_key: &str,
    ) -> Result<Option<String>, StoreError> {
        let aliases = self.draft_aliases.lock().expect("alias lock poisoned");
        Ok(aliases
            .iter()
            .find(|(account, key, _)| account == account_id.as_str() && key == draft_key)
            .map(|(_, _, entity)| entity.clone()))
    }

    fn set_draft_alias(
        &self,
        account_id: &AccountId,
        draft_key: &str,
        entity_id: &str,
    ) -> Result<(), StoreError> {
        let mut aliases = self.draft_aliases.lock().expect("alias lock poisoned");
        if let Some(row) = aliases
            .iter_mut()
            .find(|(account, key, _)| account == account_id.as_str() && key == draft_key)
        {
            row.2 = entity_id.to_string();
        } else {
            aliases.push((
                account_id.to_string(),
                draft_key.to_string(),
                entity_id.to_string(),
            ));
        }
        Ok(())
    }

    fn remove_draft_alias(
        &self,
        account_id: &AccountId,
        draft_key: &str,
    ) -> Result<(), StoreError> {
        let mut aliases = self.draft_aliases.lock().expect("alias lock poisoned");
        aliases.retain(|(account, key, _)| !(account == account_id.as_str() && key == draft_key));
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
