use super::*;

pub(super) struct MutationGateway {
    pub(super) revision: Mutex<u64>,
    pub(super) batch: Option<SyncBatch>,
    pub(super) fetch_body_result: Mutex<Option<Result<FetchedBody, GatewayError>>>,
    pub(super) fetch_attempts: Mutex<Vec<MessageId>>,
    /// Results returned by `save_draft`, popped front-first; empty falls back to
    /// a generated provider id.
    pub(super) save_draft_results: Mutex<Vec<Result<MessageId, GatewayError>>>,
    /// Records the `replace` argument of each `save_draft` call.
    pub(super) save_draft_calls: Mutex<Vec<Option<MessageId>>>,
    pub(super) delete_draft_calls: Mutex<Vec<MessageId>>,
}

impl MutationGateway {
    pub(super) fn with_revision(revision: u64) -> Self {
        Self {
            revision: Mutex::new(revision),
            batch: None,
            fetch_body_result: Mutex::new(None),
            fetch_attempts: Mutex::new(Vec::new()),
            save_draft_results: Mutex::new(Vec::new()),
            save_draft_calls: Mutex::new(Vec::new()),
            delete_draft_calls: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn with_sync_batch(revision: u64, batch: SyncBatch) -> Self {
        Self {
            revision: Mutex::new(revision),
            batch: Some(batch),
            fetch_body_result: Mutex::new(None),
            fetch_attempts: Mutex::new(Vec::new()),
            save_draft_results: Mutex::new(Vec::new()),
            save_draft_calls: Mutex::new(Vec::new()),
            delete_draft_calls: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn with_fetch_body_result(result: Result<FetchedBody, GatewayError>) -> Self {
        Self {
            revision: Mutex::new(1),
            batch: None,
            fetch_body_result: Mutex::new(Some(result)),
            fetch_attempts: Mutex::new(Vec::new()),
            save_draft_results: Mutex::new(Vec::new()),
            save_draft_calls: Mutex::new(Vec::new()),
            delete_draft_calls: Mutex::new(Vec::new()),
        }
    }

    fn apply(&self, expected_state: Option<&str>) -> Result<MutationOutcome, GatewayError> {
        let mut revision = self.revision.lock().expect("revision lock poisoned");
        if let Some(expected_state) = expected_state {
            let current = format!("message-{}", *revision);
            if expected_state != current {
                return Err(GatewayError::StateMismatch);
            }
        }
        *revision += 1;
        Ok(MutationOutcome {
            cursor: Some(SyncCursor {
                object_type: SyncObject::Message,
                state: format!("message-{}", *revision),
                updated_at: crate::RFC3339_EPOCH.to_string(),
            }),
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
        _blob_id: &crate::BlobId,
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
        Err(GatewayError::Rejected("unused".to_string()))
    }

    async fn fetch_reply_context(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<crate::ReplyContext, GatewayError> {
        Err(GatewayError::Rejected("unused".to_string()))
    }

    async fn send_message(
        &self,
        _account_id: &AccountId,
        _request: &SendMessageRequest,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::Rejected("unused".to_string()))
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
