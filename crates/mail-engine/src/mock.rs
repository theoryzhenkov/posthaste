use std::sync::Mutex;

use async_trait::async_trait;
use mail_domain::{
    AccountId, FetchedBody, GatewayError, Identity, MailGateway, MailboxId, MailboxRecord,
    MessageId, MessageRecord, Recipient, ReplyContext, SendMessageRequest, SetKeywordsCommand,
    SyncBatch, SyncCursor, SyncObject,
};

pub struct MockJmapGateway {
    state: Mutex<MockState>,
}

struct MockState {
    revision: u64,
    mailboxes: Vec<MailboxRecord>,
    messages: Vec<MessageRecord>,
}

impl Default for MockJmapGateway {
    fn default() -> Self {
        Self {
            state: Mutex::new(MockState {
                revision: 1,
                mailboxes: sample_mailboxes(),
                messages: sample_messages(),
            }),
        }
    }
}

#[async_trait]
impl MailGateway for MockJmapGateway {
    async fn sync(
        &self,
        _account_id: &AccountId,
        _cursors: &[SyncCursor],
    ) -> Result<SyncBatch, GatewayError> {
        let state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        Ok(SyncBatch {
            mailboxes: state.mailboxes.clone(),
            messages: state.messages.clone(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
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
        })
    }

    async fn set_keywords(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        command: &SetKeywordsCommand,
    ) -> Result<(), GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        ensure_expected_state(&state, expected_state)?;
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
        Ok(())
    }

    async fn replace_mailboxes(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<(), GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        ensure_expected_state(&state, expected_state)?;
        let message = state
            .messages
            .iter_mut()
            .find(|message| &message.id == message_id)
            .ok_or_else(|| GatewayError::Rejected("unknown message".to_string()))?;
        message.mailbox_ids = mailbox_ids.to_vec();
        bump_revision(&mut state);
        Ok(())
    }

    async fn destroy_message(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
    ) -> Result<(), GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::Rejected("mock state poisoned".to_string()))?;
        ensure_expected_state(&state, expected_state)?;
        state.messages.retain(|message| &message.id != message_id);
        bump_revision(&mut state);
        Ok(())
    }

    async fn fetch_identity(&self, _account_id: &AccountId) -> Result<Identity, GatewayError> {
        Ok(Identity {
            id: "mock-identity".to_string(),
            name: "Mock Sender".to_string(),
            email: "mock@example.com".to_string(),
        })
    }

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
            in_reply_to: Some(format!("<{}@mock>", message.id.as_str())),
            references: Some(format!("<{}@mock>", message.id.as_str())),
        })
    }

    async fn send_message(
        &self,
        _account_id: &AccountId,
        _request: &SendMessageRequest,
    ) -> Result<(), GatewayError> {
        Ok(())
    }
}

fn ensure_expected_state(
    state: &MockState,
    expected_state: Option<&str>,
) -> Result<(), GatewayError> {
    if let Some(expected_state) = expected_state {
        let current = format!("message-{}", state.revision);
        if expected_state != current {
            return Err(GatewayError::StateMismatch);
        }
    }
    Ok(())
}

fn bump_revision(state: &mut MockState) {
    state.revision += 1;
}

fn sample_mailboxes() -> Vec<MailboxRecord> {
    vec![
        MailboxRecord {
            id: MailboxId::from("mb-inbox"),
            name: "Inbox".to_string(),
            role: Some("inbox".to_string()),
            unread_emails: 2,
            total_emails: 3,
        },
        MailboxRecord {
            id: MailboxId::from("mb-archive"),
            name: "Archive".to_string(),
            role: Some("archive".to_string()),
            unread_emails: 0,
            total_emails: 0,
        },
        MailboxRecord {
            id: MailboxId::from("mb-trash"),
            name: "Trash".to_string(),
            role: Some("trash".to_string()),
            unread_emails: 0,
            total_emails: 0,
        },
    ]
}

fn sample_messages() -> Vec<MessageRecord> {
    vec![
        MessageRecord {
            id: MessageId::from("em-001"),
            thread_id: mail_domain::ThreadId::from("th-roadmap"),
            remote_blob_id: None,
            subject: Some("Q2 planning priorities".to_string()),
            from_name: Some("Alice Chen".to_string()),
            from_email: Some("alice@example.com".to_string()),
            preview: Some("Roadmap draft attached.".to_string()),
            received_at: "2026-03-31T09:00:00Z".to_string(),
            has_attachment: true,
            size: 48120,
            mailbox_ids: vec![MailboxId::from("mb-inbox")],
            keywords: vec!["$seen".to_string(), "$flagged".to_string()],
            body_html: Some("<p>Roadmap draft attached.</p>".to_string()),
            body_text: Some("Roadmap draft attached.".to_string()),
            raw_mime: Some("From: Alice <alice@example.com>\r\nSubject: Q2 planning priorities\r\n\r\nRoadmap draft attached.\r\n".to_string()),
        },
        MessageRecord {
            id: MessageId::from("em-002"),
            thread_id: mail_domain::ThreadId::from("th-roadmap"),
            remote_blob_id: None,
            subject: Some("Re: Q2 planning priorities".to_string()),
            from_name: Some("Marcus Johnson".to_string()),
            from_email: Some("marcus@example.com".to_string()),
            preview: Some("Looks good; one question on staffing.".to_string()),
            received_at: "2026-03-31T09:30:00Z".to_string(),
            has_attachment: false,
            size: 4120,
            mailbox_ids: vec![MailboxId::from("mb-inbox"), MailboxId::from("mb-archive")],
            keywords: Vec::new(),
            body_html: Some("<p>Looks good; one question on staffing.</p>".to_string()),
            body_text: Some("Looks good; one question on staffing.".to_string()),
            raw_mime: Some("From: Marcus <marcus@example.com>\r\nSubject: Re: Q2 planning priorities\r\n\r\nLooks good; one question on staffing.\r\n".to_string()),
        },
        MessageRecord {
            id: MessageId::from("em-003"),
            thread_id: mail_domain::ThreadId::from("th-invoice"),
            remote_blob_id: None,
            subject: Some("Invoice #2026-0312".to_string()),
            from_name: Some("Cloudflare Billing".to_string()),
            from_email: Some("billing@cloudflare.com".to_string()),
            preview: Some("Your March invoice is ready.".to_string()),
            received_at: "2026-03-30T15:00:00Z".to_string(),
            has_attachment: true,
            size: 52010,
            mailbox_ids: vec![MailboxId::from("mb-inbox")],
            keywords: vec!["$seen".to_string()],
            body_html: Some("<p>Your March invoice is ready.</p>".to_string()),
            body_text: Some("Your March invoice is ready.".to_string()),
            raw_mime: Some("From: Cloudflare Billing <billing@cloudflare.com>\r\nSubject: Invoice #2026-0312\r\n\r\nYour March invoice is ready.\r\n".to_string()),
        },
    ]
}
