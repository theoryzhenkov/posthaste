use std::collections::HashSet;
use std::sync::Mutex;

use async_trait::async_trait;
use posthaste_domain::{
    AccountId, BlobId, FetchedBody, GatewayError, Identity, MailGateway, MailboxId, MailboxRecord,
    MessageId, MessageReadback, MessageRecord, MutationOutcome, PushTransport, Recipient,
    ReplyContext,
    SendMessageRequest, SetKeywordsCommand, SyncBatch, SyncCursor, SyncObject, ThreadId,
};

mod samples;
mod state;

use samples::{sample_attachment_bytes, sample_attachments, sample_mailboxes, sample_messages};
use state::{
    bump_revision, ensure_expected_state, mutation_outcome, reject_if_marked, validate_mailbox_role,
};

/// In-memory `MailGateway` for tests and local development.
///
/// Holds a fixed set of sample mailboxes and messages. Mutations update
/// internal state and bump a revision counter to simulate JMAP state strings.
pub struct MockJmapGateway {
    state: Mutex<MockState>,
}

/// Mutable inner state behind the `MockJmapGateway` mutex.
struct MockState {
    revision: u64,
    mailboxes: Vec<MailboxRecord>,
    messages: Vec<MessageRecord>,
    /// Message ids the mock should reject mutations for (test hook): the
    /// mutation returns `MutationRejected` with the unchanged record as readback.
    rejected: HashSet<MessageId>,
}

impl MockJmapGateway {
    /// Test hook: make subsequent message mutations on `message_id` reject
    /// (provider returns the unchanged state), exercising the revert + surface
    /// settle path.
    pub fn reject_message(&self, message_id: MessageId) {
        if let Ok(mut state) = self.state.lock() {
            state.rejected.insert(message_id);
        }
    }
}

impl Default for MockJmapGateway {
    fn default() -> Self {
        Self {
            state: Mutex::new(MockState {
                revision: 1,
                mailboxes: sample_mailboxes(),
                messages: sample_messages(),
                rejected: HashSet::new(),
            }),
        }
    }
}

/// Build a mock mutation outcome from the current revision.
#[async_trait]
impl MailGateway for MockJmapGateway {
    /// Return a full snapshot of all mock mailboxes and messages.
    async fn sync(
        &self,
        _account_id: &AccountId,
        _cursors: &[SyncCursor],
        _progress: Option<posthaste_domain::SyncProgressReporter>,
    ) -> Result<SyncBatch, GatewayError> {
        let state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        Ok(SyncBatch {
            mailboxes: state.mailboxes.clone(),
            messages: state.messages.clone(),
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: true,
            replace_all_messages: true,
            cursors: vec![
                SyncCursor {
                    object_type: SyncObject::Mailbox,
                    state: format!("mailbox-{}", state.revision),
                    updated_at: "2026-03-31T10:00:00Z".to_string(),
                },
                SyncCursor {
                    object_type: SyncObject::Message,
                    state: format!("message-{}", state.revision),
                    updated_at: "2026-03-31T10:00:00Z".to_string(),
                },
            ],
        })
    }

    /// Return the pre-populated body for a mock message.
    async fn fetch_message_body(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError> {
        let state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        let message = state
            .messages
            .iter()
            .find(|message| &message.id == message_id)
            .ok_or_else(|| GatewayError::Rejected("unknown message".to_string()))?;
        Ok(FetchedBody {
            body_html: message.body_html.clone(),
            body_text: message.body_text.clone(),
            raw_mime: message.raw_mime.clone(),
            attachments: sample_attachments(message.id.as_str()),
        })
    }

    async fn download_blob(
        &self,
        _account_id: &AccountId,
        blob_id: &BlobId,
    ) -> Result<Vec<u8>, GatewayError> {
        sample_attachment_bytes(blob_id.as_str())
            .ok_or_else(|| GatewayError::Rejected("unknown blob".to_string()))
    }

    /// Apply keyword changes to a mock message, respecting optimistic concurrency.
    async fn set_keywords(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        ensure_expected_state(&state, expected_state, SyncObject::Message)?;
        reject_if_marked(&state, message_id)?;
        let message = state
            .messages
            .iter_mut()
            .find(|message| &message.id == message_id)
            .ok_or_else(|| GatewayError::Rejected("unknown message".to_string()))?;
        for keyword in &command.add {
            if !message.keywords.contains(keyword) {
                message.keywords.push(keyword.clone());
            }
        }
        message
            .keywords
            .retain(|keyword| !command.remove.contains(keyword));
        bump_revision(&mut state);
        let updated = state.messages.iter().find(|m| &m.id == message_id).cloned();
        Ok(MutationOutcome {
            message: updated.map(MessageReadback::Present),
            ..mutation_outcome(&state, SyncObject::Message)
        })
    }

    /// Replace a mock message's mailbox membership.
    async fn replace_mailboxes(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        ensure_expected_state(&state, expected_state, SyncObject::Message)?;
        reject_if_marked(&state, message_id)?;
        let message = state
            .messages
            .iter_mut()
            .find(|message| &message.id == message_id)
            .ok_or_else(|| GatewayError::Rejected("unknown message".to_string()))?;
        message.mailbox_ids = mailbox_ids.to_vec();
        bump_revision(&mut state);
        let updated = state.messages.iter().find(|m| &m.id == message_id).cloned();
        Ok(MutationOutcome {
            message: updated.map(MessageReadback::Present),
            ..mutation_outcome(&state, SyncObject::Message)
        })
    }

    /// Remove a message from mock state.
    async fn destroy_message(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        ensure_expected_state(&state, expected_state, SyncObject::Message)?;
        reject_if_marked(&state, message_id)?;
        state.messages.retain(|message| &message.id != message_id);
        bump_revision(&mut state);
        Ok(MutationOutcome {
            message: Some(MessageReadback::Removed),
            ..mutation_outcome(&state, SyncObject::Message)
        })
    }

    /// Update a mock mailbox role.
    async fn set_mailbox_role(
        &self,
        _account_id: &AccountId,
        mailbox_id: &MailboxId,
        expected_state: Option<&str>,
        role: Option<&str>,
        clear_role_from: Option<&MailboxId>,
    ) -> Result<MutationOutcome, GatewayError> {
        validate_mailbox_role(role)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        ensure_expected_state(&state, expected_state, SyncObject::Mailbox)?;
        if let Some(clear_role_from) = clear_role_from.filter(|id| *id != mailbox_id) {
            let mailbox = state
                .mailboxes
                .iter_mut()
                .find(|mailbox| &mailbox.id == clear_role_from)
                .ok_or_else(|| GatewayError::Rejected("unknown mailbox".to_string()))?;
            mailbox.role = None;
        }
        let mailbox = state
            .mailboxes
            .iter_mut()
            .find(|mailbox| &mailbox.id == mailbox_id)
            .ok_or_else(|| GatewayError::Rejected("unknown mailbox".to_string()))?;
        mailbox.role = role.map(str::to_string);
        bump_revision(&mut state);
        Ok(mutation_outcome(&state, SyncObject::Mailbox))
    }

    /// Return a hard-coded mock sender identity.
    async fn fetch_identity(&self, _account_id: &AccountId) -> Result<Identity, GatewayError> {
        Ok(Identity {
            id: "mock-identity".to_string(),
            name: "Mock Sender".to_string(),
            email: "mock@example.com".to_string(),
        })
    }

    /// Build a reply context from mock message data.
    async fn fetch_reply_context(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<ReplyContext, GatewayError> {
        let state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        let message = state
            .messages
            .iter()
            .find(|message| &message.id == message_id)
            .ok_or_else(|| GatewayError::Rejected("unknown message".to_string()))?;
        Ok(ReplyContext {
            to: vec![Recipient {
                name: message.from_name.clone(),
                email: message
                    .from_email
                    .clone()
                    .unwrap_or_else(|| "unknown@example.com".to_string()),
            }],
            cc: Vec::new(),
            reply_subject: format!("Re: {}", message.subject.clone().unwrap_or_default()),
            forward_subject: format!("Fwd: {}", message.subject.clone().unwrap_or_default()),
            quoted_body: message.body_text.clone(),
            forwarded_body: message.body_text.clone(),
            in_reply_to: Some(format!("<{}@mock>", message.id.as_str())),
            references: Some(format!("<{}@mock>", message.id.as_str())),
        })
    }

    /// No-op: accept the send request without side effects.
    async fn send_message(
        &self,
        _account_id: &AccountId,
        _request: &SendMessageRequest,
    ) -> Result<(), GatewayError> {
        Ok(())
    }

    /// Store a draft message and return a deterministic created id. When
    /// `replace` is set, the prior draft is removed first (create-new +
    /// destroy-old), mirroring the live gateway.
    ///
    /// @spec docs/L1-outbox#operation-model
    async fn save_draft(
        &self,
        _account_id: &AccountId,
        request: &SendMessageRequest,
        replace: Option<&MessageId>,
    ) -> Result<MessageId, GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        if let Some(replace) = replace {
            state.messages.retain(|message| &message.id != replace);
        }
        bump_revision(&mut state);
        let new_id = MessageId::from(format!("draft-created-{}", state.revision));
        state.messages.push(MessageRecord {
            id: new_id.clone(),
            source_thread_id: ThreadId::from(new_id.as_str()),
            remote_blob_id: None,
            subject: Some(request.subject.clone()),
            from_name: request.from.as_ref().and_then(|from| from.name.clone()),
            from_email: request.from.as_ref().map(|from| from.email.clone()),
            to: request.to.clone(),
            preview: Some(request.body.clone()),
            received_at: "2026-03-31T10:00:00Z".to_string(),
            has_attachment: !request.attachments.is_empty(),
            size: request.body.len() as i64,
            mailbox_ids: Vec::new(),
            keywords: vec!["$draft".to_string()],
            body_html: None,
            body_text: Some(request.body.clone()),
            raw_mime: None,
            rfc_message_id: Some(format!("<{}@mock>", new_id.as_str())),
            in_reply_to: request.in_reply_to.clone(),
            references: request
                .references
                .as_ref()
                .map(|references| references.split_whitespace().map(str::to_string).collect())
                .unwrap_or_default(),
            // Simulate the provider round-tripping the X-Posthaste-Draft-Id header.
            draft_id: request.draft_id.clone(),
        });
        Ok(new_id)
    }

    /// Remove a draft message by id.
    ///
    /// @spec docs/L1-outbox#operation-model
    async fn delete_draft(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        state.messages.retain(|message| &message.id != message_id);
        bump_revision(&mut state);
        Ok(())
    }

    /// Mock gateway has no push transports.
    fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
        vec![]
    }
}

#[cfg(test)]
mod tests;
