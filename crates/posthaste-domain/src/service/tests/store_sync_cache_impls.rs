use super::*;

impl SyncWriteStore for TestStore {
    fn apply_sync_batch(
        &self,
        _account_id: &AccountId,
        batch: &SyncBatch,
    ) -> Result<Vec<DomainEvent>, StoreError> {
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
        _account_id: &AccountId,
        reconciliation: &crate::SyncReconciliation,
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
        candidates: &[crate::CacheCandidate],
    ) -> Result<(), StoreError> {
        self.cache_candidates
            .lock()
            .expect("cache candidates lock poisoned")
            .extend(candidates.iter().cloned());
        Ok(())
    }

    fn record_cache_signal_updates(
        &self,
        updates: &[crate::CacheSignalUpdate],
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
    ) -> Result<Vec<crate::CacheRescoreCandidate>, StoreError> {
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
        updates: &[crate::CachePriorityUpdate],
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
        layer: crate::CacheLayer,
        limit: usize,
    ) -> Result<Vec<crate::CacheFetchCandidate>, StoreError> {
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
        _layer: crate::CacheLayer,
        _object_id: Option<&str>,
        state: crate::CacheObjectState,
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
