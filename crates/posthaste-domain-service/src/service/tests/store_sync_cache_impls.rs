use super::*;

impl SyncWriteStore for TestStore {
    fn apply_sync_batch(
        &self,
        _base: &BaseWrite,
        _account_id: &AccountId,
        batch: &SyncBatch,
    ) -> Result<Vec<DomainEvent>, StoreError> {
        {
            let mut applied = self
                .applied_messages
                .lock()
                .expect("applied messages lock poisoned");
            // Mirror the real store: a deleted id loses its base record.
            applied.retain(|record| !batch.deleted_message_ids.contains(&record.id));
            applied.extend(batch.messages.iter().cloned());
        }
        let mut state = self
            .mutation_state
            .lock()
            .expect("mutation state lock poisoned");
        state.applied_message_chunks.push(batch.messages.len());
        if let Some(cursor) = batch
            .cursors
            .iter()
            .find(|cursor| cursor.object_type == SyncObject::Message)
        {
            state.cursor = Some(cursor.clone());
        }
        if let Some(message) = batch.messages.last() {
            state.mailbox_ids = message.mailbox_ids.clone();
        }
        if !batch.deleted_message_ids.is_empty() {
            state.mailbox_ids.clear();
        }
        Ok(Vec::new())
    }

    fn reconcile_sync(
        &self,
        _base: &BaseWrite,
        _account_id: &AccountId,
        reconciliation: &posthaste_domain_model::SyncReconciliation,
    ) -> Result<Vec<DomainEvent>, StoreError> {
        // Commit the cursors withheld until the stream succeeded; the real store
        // also prunes locals absent from the remote set (covered by store tests).
        let mut state = self
            .mutation_state
            .lock()
            .expect("mutation state lock poisoned");
        state.reconcile_calls += 1;
        if let Some(cursor) = reconciliation
            .cursors
            .iter()
            .find(|cursor| cursor.object_type == SyncObject::Message)
        {
            state.cursor = Some(cursor.clone());
        }
        Ok(Vec::new())
    }

    fn apply_message_body(
        &self,
        _base: &BaseWrite,
        _account_id: &AccountId,
        message_id: &MessageId,
        body: &FetchedBody,
    ) -> Result<CommandResult, StoreError> {
        if let Some(error) = &self.apply_body_error {
            return Err(StoreError::Failure(error.clone()));
        }
        self.applied_bodies
            .lock()
            .expect("applied bodies lock poisoned")
            .push((
                message_id.clone(),
                body.body_html.clone(),
                body.body_text.clone(),
            ));
        Ok(CommandResult {
            detail: None,
            events: Vec::new(),
        })
    }
}

impl crate::CacheStore for TestStore {
    fn upsert_cache_candidates(
        &self,
        candidates: &[posthaste_domain_model::CacheCandidate],
    ) -> Result<(), StoreError> {
        self.cache_candidates
            .lock()
            .expect("cache candidates lock poisoned")
            .extend(candidates.iter().cloned());
        Ok(())
    }

    fn record_cache_signal_updates(
        &self,
        updates: &[posthaste_domain_model::CacheSignalUpdate],
    ) -> Result<(), StoreError> {
        self.cache_signal_updates
            .lock()
            .expect("cache signal updates lock poisoned")
            .extend(updates.iter().cloned());
        Ok(())
    }

    fn list_cache_rescore_candidates(
        &self,
        account_id: &AccountId,
        limit: usize,
    ) -> Result<Vec<posthaste_domain_model::CacheRescoreCandidate>, StoreError> {
        Ok(self
            .cache_rescore_candidates
            .lock()
            .expect("cache rescore candidates lock poisoned")
            .iter()
            .filter(|candidate| candidate.account_id == account_id.as_str())
            .take(limit)
            .cloned()
            .collect())
    }

    fn queue_stale_cache_rescore_candidates(
        &self,
        account_id: &AccountId,
        stale_before: &str,
        limit: usize,
    ) -> Result<usize, StoreError> {
        self.stale_cache_rescore_requests
            .lock()
            .expect("stale cache rescore requests lock poisoned")
            .push((account_id.clone(), stale_before.to_string(), limit));
        Ok(self.stale_cache_rescore_result)
    }

    fn update_cache_priorities(
        &self,
        updates: &[posthaste_domain_model::CachePriorityUpdate],
    ) -> Result<(), StoreError> {
        self.cache_priority_updates
            .lock()
            .expect("cache priority updates lock poisoned")
            .extend(updates.iter().cloned());
        Ok(())
    }

    fn list_cache_fetch_candidates(
        &self,
        account_id: &AccountId,
        layer: posthaste_domain_model::CacheLayer,
        limit: usize,
    ) -> Result<Vec<posthaste_domain_model::CacheFetchCandidate>, StoreError> {
        Ok(self
            .cache_fetch_candidates
            .lock()
            .expect("cache fetch candidates lock poisoned")
            .iter()
            .filter(|candidate| {
                candidate.account_id == account_id.as_str() && candidate.layer == layer
            })
            .take(limit)
            .cloned()
            .collect())
    }

    fn mark_cache_object_state(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        _layer: posthaste_domain_model::CacheLayer,
        _object_id: Option<&str>,
        state: posthaste_domain_model::CacheObjectState,
        error_code: Option<&str>,
    ) -> Result<(), StoreError> {
        self.cache_state_changes
            .lock()
            .expect("cache state changes lock poisoned")
            .push((
                message_id.clone(),
                state,
                error_code.map(ToString::to_string),
            ));
        Ok(())
    }

    fn cache_used_bytes(&self) -> Result<u64, StoreError> {
        Ok(*self
            .cache_used_bytes
            .lock()
            .expect("cache used bytes lock poisoned"))
    }
}

impl MessageOverlayStore for TestStore {
    fn upsert_overlay_message(
        &self,
        _account_id: &AccountId,
        message: &MessageRecord,
    ) -> Result<(), StoreError> {
        self.overlay_rows
            .lock()
            .expect("overlay rows lock poisoned")
            .insert(message.id.as_str().to_string(), Some(message.clone()));
        Ok(())
    }

    fn tombstone_overlay_message(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), StoreError> {
        self.overlay_rows
            .lock()
            .expect("overlay rows lock poisoned")
            .insert(message_id.as_str().to_string(), None);
        Ok(())
    }

    fn remove_overlay_message(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), StoreError> {
        self.overlay_rows
            .lock()
            .expect("overlay rows lock poisoned")
            .remove(message_id.as_str());
        Ok(())
    }

    fn list_overlay_message_ids(
        &self,
        _account_id: &AccountId,
    ) -> Result<Vec<MessageId>, StoreError> {
        Ok(self
            .overlay_rows
            .lock()
            .expect("overlay rows lock poisoned")
            .keys()
            .map(|id| MessageId::from(id.as_str()))
            .collect())
    }

    fn read_overlay_message(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<Option<MessageRecord>>, StoreError> {
        Ok(self
            .overlay_rows
            .lock()
            .expect("overlay rows lock poisoned")
            .get(message_id.as_str())
            .cloned())
    }

    fn find_base_message_id_by_rfc_prefix(
        &self,
        _account_id: &AccountId,
        prefix: &str,
    ) -> Result<Option<MessageId>, StoreError> {
        Ok(self
            .applied_messages
            .lock()
            .expect("applied messages lock poisoned")
            .iter()
            .find(|record| {
                record
                    .rfc_message_id
                    .as_deref()
                    .is_some_and(|rfc| rfc.starts_with(prefix))
            })
            .map(|record| record.id.clone()))
    }

    fn read_base_message_record(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageRecord>, StoreError> {
        // "Base" in the mock is whatever sync last applied: the newest
        // `apply_sync_batch` record for the id wins. Fixtures seeded via
        // `with_message_state` (no applied record) fall back to a synthetic
        // base carrying `mutation_state.mailbox_ids` — the same pretense the
        // `get_message_mailboxes` mock has always made.
        let applied = self
            .applied_messages
            .lock()
            .expect("applied messages lock poisoned")
            .iter()
            .rev()
            .find(|record| record.id == *message_id)
            .cloned();
        if let Some(record) = applied {
            return Ok(Some(record));
        }
        let state = self
            .mutation_state
            .lock()
            .expect("mutation state lock poisoned");
        if state.mailbox_ids.is_empty() {
            return Ok(None);
        }
        Ok(Some(MessageRecord {
            id: message_id.clone(),
            source_thread_id: posthaste_domain_model::ThreadId::from("thread-1"),
            received_at: posthaste_domain_model::RFC3339_EPOCH.to_string(),
            mailbox_ids: state.mailbox_ids.clone(),
            ..Default::default()
        }))
    }
}
