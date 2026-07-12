use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use posthaste_domain_model::{
    AccountId, BlobId, FetchedBody, GatewayError, Identity, MailboxId, MailboxRecord, MessageId,
    MessageRecord, MutationOutcome, ReplyContext, SendMessageRequest, SetKeywordsCommand,
    SyncBatch, SyncCursor, SyncObject, RFC3339_EPOCH,
};
use posthaste_domain_service::{MailGateway, PushTransport};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum RecordedMutation {
    SetKeywords {
        account_id: String,
        message_id: String,
        add: Vec<String>,
        remove: Vec<String>,
    },
    ReplaceMailboxes {
        account_id: String,
        message_id: String,
        mailbox_ids: Vec<String>,
    },
}

pub(super) struct ScriptedGateway {
    state: Mutex<GatewayState>,
}

struct GatewayState {
    revision: u64,
    mailboxes: Vec<MailboxRecord>,
    messages: BTreeMap<String, MessageRecord>,
    mutations: Vec<RecordedMutation>,
}

impl ScriptedGateway {
    pub(super) fn new(mailboxes: Vec<MailboxRecord>, messages: Vec<MessageRecord>) -> Self {
        Self {
            state: Mutex::new(GatewayState {
                revision: 1,
                mailboxes,
                messages: messages
                    .into_iter()
                    .map(|message| (message.id.to_string(), message))
                    .collect(),
                mutations: Vec::new(),
            }),
        }
    }

    pub(super) fn mutations(&self) -> Vec<RecordedMutation> {
        self.state
            .lock()
            .expect("gateway state lock should not be poisoned")
            .mutations
            .clone()
    }
}

fn mutation_outcome(state: &mut GatewayState, object_type: SyncObject) -> MutationOutcome {
    state.revision += 1;
    MutationOutcome {
        cursor: Some(SyncCursor {
            object_type,
            state: format!("{}-{}", object_type.as_str(), state.revision),
            updated_at: RFC3339_EPOCH.to_string(),
        }),
        message: None,
    }
}

#[async_trait]
impl MailGateway for ScriptedGateway {
    async fn sync(
        &self,
        _account_id: &AccountId,
        _cursors: &[SyncCursor],
        _progress: Option<posthaste_domain_service::SyncProgressReporter>,
    ) -> Result<SyncBatch, GatewayError> {
        let state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("gateway state poisoned".to_string()))?;
        Ok(SyncBatch {
            mailboxes: state.mailboxes.clone(),
            messages: state.messages.values().cloned().collect(),
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            absence_deleted_imap_message_locations: Vec::new(),
            absence_deleted_message_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: true,
            replace_all_messages: true,
            cursors: vec![
                SyncCursor {
                    object_type: SyncObject::Mailbox,
                    state: format!("mailbox-{}", state.revision),
                    updated_at: RFC3339_EPOCH.to_string(),
                },
                SyncCursor {
                    object_type: SyncObject::Message,
                    state: format!("message-{}", state.revision),
                    updated_at: RFC3339_EPOCH.to_string(),
                },
            ],
        })
    }

    async fn fetch_message_body(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError> {
        Err(GatewayError::Rejected(
            "unused in automation tests".to_string(),
        ))
    }

    async fn download_blob(
        &self,
        _account_id: &AccountId,
        _blob_id: &BlobId,
    ) -> Result<Vec<u8>, GatewayError> {
        Err(GatewayError::Rejected(
            "unused in automation tests".to_string(),
        ))
    }

    async fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        _expected_state: Option<&str>,
        command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("gateway state poisoned".to_string()))?;
        let message = state
            .messages
            .get_mut(message_id.as_str())
            .ok_or_else(|| GatewayError::Rejected("unknown message".to_string()))?;
        for keyword in &command.remove {
            message.keywords.retain(|candidate| candidate != keyword);
        }
        for keyword in &command.add {
            if !message
                .keywords
                .iter()
                .any(|candidate| candidate == keyword)
            {
                message.keywords.push(keyword.clone());
            }
        }
        state.mutations.push(RecordedMutation::SetKeywords {
            account_id: account_id.to_string(),
            message_id: message_id.to_string(),
            add: command.add.clone(),
            remove: command.remove.clone(),
        });
        Ok(mutation_outcome(&mut state, SyncObject::Message))
    }

    async fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        _expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("gateway state poisoned".to_string()))?;
        let message = state
            .messages
            .get_mut(message_id.as_str())
            .ok_or_else(|| GatewayError::Rejected("unknown message".to_string()))?;
        message.mailbox_ids = mailbox_ids.to_vec();
        state.mutations.push(RecordedMutation::ReplaceMailboxes {
            account_id: account_id.to_string(),
            message_id: message_id.to_string(),
            mailbox_ids: mailbox_ids.iter().map(ToString::to_string).collect(),
        });
        Ok(mutation_outcome(&mut state, SyncObject::Message))
    }

    async fn destroy_message(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        _expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError> {
        Err(GatewayError::Rejected(
            "unused in automation tests".to_string(),
        ))
    }

    async fn set_mailbox_role(
        &self,
        _account_id: &AccountId,
        _mailbox_id: &MailboxId,
        _expected_state: Option<&str>,
        _role: Option<&str>,
        _clear_role_from: Option<&MailboxId>,
    ) -> Result<MutationOutcome, GatewayError> {
        Err(GatewayError::Rejected(
            "unused in automation tests".to_string(),
        ))
    }

    async fn create_mailbox(
        &self,
        _account_id: &AccountId,
        _name: &str,
    ) -> Result<MailboxId, GatewayError> {
        Err(GatewayError::Rejected(
            "unused in automation tests".to_string(),
        ))
    }

    async fn destroy_mailbox(
        &self,
        _account_id: &AccountId,
        _mailbox_id: &MailboxId,
        _remove_emails: bool,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::Rejected(
            "unused in automation tests".to_string(),
        ))
    }

    async fn fetch_identity(&self, _account_id: &AccountId) -> Result<Identity, GatewayError> {
        Err(GatewayError::Rejected(
            "unused in automation tests".to_string(),
        ))
    }

    async fn fetch_reply_context(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<ReplyContext, GatewayError> {
        Err(GatewayError::Rejected(
            "unused in automation tests".to_string(),
        ))
    }

    async fn send_message(
        &self,
        _account_id: &AccountId,
        _request: &SendMessageRequest,
        _idempotency_key: &str,
    ) -> Result<posthaste_domain_model::SendFiling, GatewayError> {
        Err(GatewayError::Rejected(
            "unused in automation tests".to_string(),
        ))
    }

    fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
        Vec::new()
    }
}
