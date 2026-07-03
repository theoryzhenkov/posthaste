use super::*;

pub(super) struct MutationGateway {
    pub(super) revision: Mutex<u64>,
    pub(super) batch: Option<SyncBatch>,
    /// When set, `sync_streamed` emits these chunks in order and returns the
    /// reconciliation set, exercising the progressive-delivery service path.
    pub(super) stream: Option<(Vec<SyncBatch>, posthaste_domain_model::SyncReconciliation)>,
    pub(super) fetch_body_result: Mutex<Option<Result<FetchedBody, GatewayError>>>,
    /// Result returned by `fetch_identity`; `None` falls back to an error (the
    /// default unused stub), preserving the pre-change test behavior.
    pub(super) fetch_identity_result: Mutex<Option<Result<Identity, GatewayError>>>,
    pub(super) fetch_attempts: Mutex<Vec<MessageId>>,
    /// Results returned by `save_draft`, popped front-first; empty falls back to
    /// a generated provider id.
    pub(super) save_draft_results: Mutex<Vec<Result<MessageId, GatewayError>>>,
    /// Records the `replace` argument of each `save_draft` call.
    pub(super) save_draft_calls: Mutex<Vec<Option<MessageId>>>,
    pub(super) delete_draft_calls: Mutex<Vec<MessageId>>,
    /// Subjects of each `send_message` call, in order (includes deduplicated
    /// re-forwards of an already-committed send).
    pub(super) send_calls: Mutex<Vec<String>>,
    /// Idempotency keys that have committed a submission server-side. A second
    /// `send_message` under a key already present is a re-forward that does
    /// **not** create a second submission (models JMAP `ifInState` / the stable
    /// `Message-ID` dedup — D84/D85).
    pub(super) committed_send_keys: Mutex<Vec<String>>,
    /// Outcomes for the *first* (committing) attempt of each new send key,
    /// popped front-first; empty falls back to `Ok`. An `Err` here models a send
    /// that committed but whose response was lost (e.g.
    /// `GatewayError::DispatchUncertain`).
    pub(super) send_results: Mutex<Vec<Result<(), GatewayError>>>,
    /// Results returned by `set_keywords`, popped front-first; empty falls back
    /// to the revision-based success path.
    pub(super) set_keywords_results: Mutex<Vec<Result<MutationOutcome, GatewayError>>>,
    /// Readbacks attached to each accepted message mutation's `MutationOutcome`,
    /// popped front-first (the `get` of set+get). Empty => `message: None`.
    pub(super) readbacks: Mutex<Vec<posthaste_domain_model::MessageReadback>>,
    /// When set, the next message mutation is rejected: returns
    /// `Err(MutationRejected { readback, reason })` so the flush reverts + surfaces.
    pub(super) reject_next: Mutex<Option<(posthaste_domain_model::MessageReadback, String)>>,
}

impl MutationGateway {
    pub(super) fn with_stream(
        chunks: Vec<SyncBatch>,
        reconciliation: posthaste_domain_model::SyncReconciliation,
    ) -> Self {
        Self {
            stream: Some((chunks, reconciliation)),
            ..Self::with_revision(1)
        }
    }

    pub(super) fn with_revision(revision: u64) -> Self {
        Self {
            revision: Mutex::new(revision),
            batch: None,
            stream: None,
            fetch_body_result: Mutex::new(None),
            fetch_identity_result: Mutex::new(None),
            fetch_attempts: Mutex::new(Vec::new()),
            save_draft_results: Mutex::new(Vec::new()),
            save_draft_calls: Mutex::new(Vec::new()),
            delete_draft_calls: Mutex::new(Vec::new()),
            send_calls: Mutex::new(Vec::new()),
            committed_send_keys: Mutex::new(Vec::new()),
            send_results: Mutex::new(Vec::new()),
            set_keywords_results: Mutex::new(Vec::new()),
            readbacks: Mutex::new(Vec::new()),
            reject_next: Mutex::new(None),
        }
    }

    pub(super) fn with_sync_batch(revision: u64, batch: SyncBatch) -> Self {
        Self {
            batch: Some(batch),
            ..Self::with_revision(revision)
        }
    }

    pub(super) fn with_fetch_body_result(result: Result<FetchedBody, GatewayError>) -> Self {
        Self {
            fetch_body_result: Mutex::new(Some(result)),
            ..Self::with_revision(1)
        }
    }

    pub(super) fn with_identity(identity: Identity) -> Self {
        Self {
            fetch_identity_result: Mutex::new(Some(Ok(identity))),
            ..Self::with_revision(1)
        }
    }

    fn apply(&self, expected_state: Option<&str>) -> Result<MutationOutcome, GatewayError> {
        let reject_next = self
            .reject_next
            .lock()
            .expect("reject_next lock poisoned")
            .take();
        if let Some((readback, reason)) = reject_next {
            return Err(GatewayError::MutationRejected {
                readback: Box::new(readback),
                reason,
            });
        }
        let mut revision = self.revision.lock().expect("revision lock poisoned");
        if let Some(expected_state) = expected_state {
            let current = format!("message-{}", *revision);
            if expected_state != current {
                return Err(GatewayError::StateMismatch);
            }
        }
        *revision += 1;
        let mut readbacks = self.readbacks.lock().expect("readbacks lock poisoned");
        let message = (!readbacks.is_empty()).then(|| readbacks.remove(0));
        Ok(MutationOutcome {
            cursor: Some(SyncCursor {
                object_type: SyncObject::Message,
                state: format!("message-{}", *revision),
                updated_at: posthaste_domain_model::RFC3339_EPOCH.to_string(),
            }),
            message,
        })
    }
}

#[async_trait]
impl MailGateway for MutationGateway {
    async fn sync(
        &self,
        _account_id: &AccountId,
        _cursors: &[SyncCursor],
        _progress: Option<crate::SyncProgressReporter>,
    ) -> Result<SyncBatch, GatewayError> {
        self.batch
            .clone()
            .ok_or_else(|| GatewayError::Rejected("unused".to_string()))
    }

    async fn sync_streamed(
        &self,
        account_id: &AccountId,
        cursors: &[SyncCursor],
        progress: Option<crate::SyncProgressReporter>,
        sink: &mut dyn crate::SyncChunkSink,
    ) -> Result<posthaste_domain_model::SyncOutcome, GatewayError> {
        if let Some((chunks, reconciliation)) = &self.stream {
            for chunk in chunks {
                sink.emit(chunk.clone()).await?;
            }
            return Ok(posthaste_domain_model::SyncOutcome {
                reconciliation: Some(reconciliation.clone()),
            });
        }
        let batch = self.sync(account_id, cursors, progress).await?;
        sink.emit(batch).await?;
        Ok(posthaste_domain_model::SyncOutcome::single_batch())
    }

    async fn fetch_message_body(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError> {
        self.fetch_attempts
            .lock()
            .expect("fetch attempts lock poisoned")
            .push(message_id.clone());
        self.fetch_body_result
            .lock()
            .expect("fetch body result lock poisoned")
            .take()
            .unwrap_or_else(|| Err(GatewayError::Rejected("unused".to_string())))
    }

    async fn download_blob(
        &self,
        _account_id: &AccountId,
        _blob_id: &posthaste_domain_model::BlobId,
    ) -> Result<Vec<u8>, GatewayError> {
        Err(GatewayError::Rejected("unused".to_string()))
    }

    async fn set_keywords(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        expected_state: Option<&str>,
        _command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError> {
        let set_keywords_result = self
            .set_keywords_results
            .lock()
            .expect("set keywords results poisoned")
            .pop();
        if let Some(result) = set_keywords_result {
            return result;
        }
        self.apply(expected_state)
    }

    async fn replace_mailboxes(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        expected_state: Option<&str>,
        _mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError> {
        self.apply(expected_state)
    }

    async fn destroy_message(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError> {
        self.apply(expected_state)
    }

    async fn set_mailbox_role(
        &self,
        _account_id: &AccountId,
        _mailbox_id: &MailboxId,
        _expected_state: Option<&str>,
        _role: Option<&str>,
        _clear_role_from: Option<&MailboxId>,
    ) -> Result<MutationOutcome, GatewayError> {
        Err(GatewayError::Rejected("unused".to_string()))
    }

    async fn fetch_identity(&self, _account_id: &AccountId) -> Result<Identity, GatewayError> {
        self.fetch_identity_result
            .lock()
            .expect("fetch identity result lock poisoned")
            .take()
            .unwrap_or_else(|| Err(GatewayError::Rejected("unused".to_string())))
    }

    async fn fetch_reply_context(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<posthaste_domain_model::ReplyContext, GatewayError> {
        Err(GatewayError::Rejected("unused".to_string()))
    }

    async fn send_message(
        &self,
        _account_id: &AccountId,
        request: &SendMessageRequest,
        idempotency_key: &str,
    ) -> Result<(), GatewayError> {
        self.send_calls
            .lock()
            .expect("send calls poisoned")
            .push(request.subject.clone());
        let mut committed = self
            .committed_send_keys
            .lock()
            .expect("committed send keys poisoned");
        if committed.iter().any(|key| key == idempotency_key) {
            // The prior attempt already committed under this identity; a
            // re-forward is deduplicated — no second submission (D84/D85).
            return Ok(());
        }
        // Record the commit *before* applying the configured outcome, so an
        // `Err` outcome models "committed server-side but the response was lost."
        committed.push(idempotency_key.to_string());
        drop(committed);
        let mut results = self.send_results.lock().expect("send results poisoned");
        if results.is_empty() {
            Ok(())
        } else {
            results.remove(0)
        }
    }

    async fn save_draft(
        &self,
        _account_id: &AccountId,
        _request: &SendMessageRequest,
        replace: Option<&MessageId>,
    ) -> Result<MessageId, GatewayError> {
        let call_index = {
            let mut calls = self
                .save_draft_calls
                .lock()
                .expect("save draft calls poisoned");
            calls.push(replace.cloned());
            calls.len()
        };
        let mut results = self
            .save_draft_results
            .lock()
            .expect("save draft results poisoned");
        if results.is_empty() {
            Ok(MessageId::from(format!("provider-draft-{call_index}")))
        } else {
            results.remove(0)
        }
    }

    async fn delete_draft(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), GatewayError> {
        self.delete_draft_calls
            .lock()
            .expect("delete draft calls poisoned")
            .push(message_id.clone());
        Ok(())
    }

    fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
        vec![]
    }
}
