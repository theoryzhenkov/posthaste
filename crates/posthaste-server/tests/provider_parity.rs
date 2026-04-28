use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, BlobId, FetchedBody,
    GatewayError, Identity, MailGateway, MailService, MailboxId, MailboxRecord, MessageId,
    MessageRecord, MutationOutcome, PushTransport, ReplyContext, SendMessageRequest,
    SetKeywordsCommand, SyncBatch, SyncCursor, SyncObject, SyncTrigger, ThreadId, RFC3339_EPOCH,
};
use posthaste_imap::{
    imap_body_from_raw_mime, imap_full_sync_batch, imap_header_message_record,
    imap_mailbox_state_from_header_snapshot, map_imap_mailbox, ImapFetchedHeader,
    ImapMailboxHeaderSnapshot,
};
use posthaste_store::DatabaseStore;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("posthaste-provider-parity-test-{now}-{seq}"))
}

struct Harness {
    service: MailService,
}

impl Harness {
    fn new() -> Self {
        let root = temp_root();
        let config_root = root.join("config");
        let state_root = root.join("state");
        let config_repo =
            TomlConfigRepository::open(&config_root).expect("config repository should open");
        config_repo
            .initialize_defaults()
            .expect("config defaults should initialize");
        let database_store = Arc::new(
            DatabaseStore::open(state_root.join("mail.sqlite"), &state_root)
                .expect("database store should open"),
        );
        let config = Arc::new(config_repo);
        Self {
            service: MailService::new(database_store, config),
        }
    }

    fn save_account(&self, id: &str, name: &str, driver: AccountDriver) {
        self.service
            .save_source(&AccountSettings {
                id: AccountId::from(id),
                name: name.to_string(),
                full_name: None,
                email_patterns: Vec::new(),
                driver,
                enabled: true,
                appearance: None,
                transport: AccountTransportSettings::default(),
                created_at: RFC3339_EPOCH.to_string(),
                updated_at: RFC3339_EPOCH.to_string(),
            })
            .expect("account should save");
    }
}

#[derive(Clone)]
struct StaticGateway {
    batch: Arc<Mutex<Option<SyncBatch>>>,
    body: FetchedBody,
    blob: Vec<u8>,
}

impl StaticGateway {
    fn new(batch: SyncBatch, body: FetchedBody, blob: Vec<u8>) -> Self {
        Self {
            batch: Arc::new(Mutex::new(Some(batch))),
            body,
            blob,
        }
    }
}

#[async_trait]
impl MailGateway for StaticGateway {
    async fn sync(
        &self,
        _account_id: &AccountId,
        _cursors: &[SyncCursor],
        _progress: Option<posthaste_domain::SyncProgressReporter>,
    ) -> Result<SyncBatch, GatewayError> {
        Ok(self
            .batch
            .lock()
            .expect("batch lock poisoned")
            .take()
            .expect("sync should be called once"))
    }

    async fn fetch_message_body(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError> {
        Ok(self.body.clone())
    }

    async fn download_blob(
        &self,
        _account_id: &AccountId,
        _blob_id: &BlobId,
    ) -> Result<Vec<u8>, GatewayError> {
        Ok(self.blob.clone())
    }

    async fn set_keywords(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        _expected_state: Option<&str>,
        _command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError> {
        Err(GatewayError::Rejected("unused".to_string()))
    }

    async fn replace_mailboxes(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        _expected_state: Option<&str>,
        _mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError> {
        Err(GatewayError::Rejected("unused".to_string()))
    }

    async fn destroy_message(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
        _expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError> {
        Err(GatewayError::Rejected("unused".to_string()))
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
        Ok(Identity {
            id: "identity".to_string(),
            name: "Alice".to_string(),
            email: "alice@example.test".to_string(),
        })
    }

    async fn fetch_reply_context(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<ReplyContext, GatewayError> {
        Err(GatewayError::Rejected("unused".to_string()))
    }

    async fn send_message(
        &self,
        _account_id: &AccountId,
        _request: &SendMessageRequest,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::Rejected("unused".to_string()))
    }

    fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
        Vec::new()
    }
}

#[tokio::test]
async fn imap_and_jmap_sync_and_lazy_body_project_equivalent_message_details() {
    let harness = Harness::new();
    harness.save_account("jmap", "JMAP", AccountDriver::Jmap);
    harness.save_account("imap", "IMAP", AccountDriver::ImapSmtp);
    let jmap_body = parity_body();
    let imap_body = imap_body_from_raw_mime(&MessageId::from("unused"), parity_raw_mime())
        .expect("IMAP body should parse");
    let jmap_gateway = StaticGateway::new(jmap_sync_batch(), jmap_body, parity_attachment_blob());
    let imap_batch = imap_sync_batch();
    let imap_message_id = imap_batch.messages[0].id.clone();
    let imap_gateway = StaticGateway::new(imap_batch, imap_body, parity_attachment_blob());

    harness
        .service
        .sync_account(
            &AccountId::from("jmap"),
            SyncTrigger::Manual,
            &jmap_gateway,
            None,
        )
        .await
        .expect("JMAP sync should apply");
    harness
        .service
        .sync_account(
            &AccountId::from("imap"),
            SyncTrigger::Manual,
            &imap_gateway,
            None,
        )
        .await
        .expect("IMAP sync should apply");

    let jmap_detail = harness
        .service
        .get_message_detail(
            &AccountId::from("jmap"),
            &MessageId::from("jmap-message-1"),
            Some(&jmap_gateway),
        )
        .await
        .expect("JMAP body should fetch")
        .detail
        .expect("JMAP detail");
    let imap_detail = harness
        .service
        .get_message_detail(
            &AccountId::from("imap"),
            &imap_message_id,
            Some(&imap_gateway),
        )
        .await
        .expect("IMAP body should fetch")
        .detail
        .expect("IMAP detail");

    assert_eq!(jmap_detail.summary.subject, imap_detail.summary.subject);
    assert_eq!(
        jmap_detail.summary.from_email,
        imap_detail.summary.from_email
    );
    assert_eq!(jmap_detail.summary.is_read, imap_detail.summary.is_read);
    assert_eq!(
        jmap_detail.summary.is_flagged,
        imap_detail.summary.is_flagged
    );
    assert_eq!(jmap_detail.body_text, imap_detail.body_text);
    assert_eq!(jmap_detail.body_html, imap_detail.body_html);
    assert_eq!(jmap_detail.attachments.len(), imap_detail.attachments.len());
    assert_eq!(
        jmap_detail.attachments[0].filename,
        imap_detail.attachments[0].filename
    );
    assert_eq!(
        jmap_detail.attachments[0].mime_type,
        imap_detail.attachments[0].mime_type
    );

    let jmap_blob = harness
        .service
        .download_blob(
            &AccountId::from("jmap"),
            &jmap_detail.attachments[0].blob_id,
            &jmap_gateway,
        )
        .await
        .expect("JMAP blob should download");
    let imap_blob = harness
        .service
        .download_blob(
            &AccountId::from("imap"),
            &imap_detail.attachments[0].blob_id,
            &imap_gateway,
        )
        .await
        .expect("IMAP blob should download");
    assert_eq!(jmap_blob, imap_blob);
}

fn jmap_sync_batch() -> SyncBatch {
    SyncBatch {
        mailboxes: vec![MailboxRecord {
            id: MailboxId::from("inbox"),
            name: "Inbox".to_string(),
            role: Some("inbox".to_string()),
            unread_emails: 0,
            total_emails: 1,
        }],
        messages: vec![MessageRecord {
            id: MessageId::from("jmap-message-1"),
            source_thread_id: ThreadId::from("thread-1"),
            remote_blob_id: None,
            subject: Some("Parity subject".to_string()),
            from_name: Some("Alice".to_string()),
            from_email: Some("alice@example.test".to_string()),
            preview: None,
            received_at: "2026-04-25T12:00:00Z".to_string(),
            has_attachment: true,
            size: 512,
            mailbox_ids: vec![MailboxId::from("inbox")],
            keywords: vec!["$flagged".to_string(), "$seen".to_string()],
            body_html: None,
            body_text: None,
            raw_mime: None,
            rfc_message_id: Some("<parity@example.test>".to_string()),
            in_reply_to: None,
            references: Vec::new(),
        }],
        imap_mailbox_states: Vec::new(),
        imap_message_locations: Vec::new(),
        deleted_mailbox_ids: Vec::new(),
        deleted_message_ids: Vec::new(),
        replace_all_mailboxes: true,
        replace_all_messages: true,
        cursors: vec![
            SyncCursor {
                object_type: SyncObject::Mailbox,
                state: "jmap-mailbox-state".to_string(),
                updated_at: "2026-04-25T12:00:00Z".to_string(),
            },
            SyncCursor {
                object_type: SyncObject::Message,
                state: "jmap-message-state".to_string(),
                updated_at: "2026-04-25T12:00:00Z".to_string(),
            },
        ],
    }
}

fn imap_sync_batch() -> SyncBatch {
    let selected = posthaste_domain::ImapSelectedMailbox {
        mailbox_id: posthaste_imap::imap_mailbox_id("INBOX"),
        mailbox_name: "INBOX".to_string(),
        uid_validity: posthaste_domain::ImapUidValidity(9),
        uid_next: None,
        highest_modseq: None,
    };
    let mapped = imap_header_message_record(
        &selected,
        ImapFetchedHeader {
            mailbox_id: selected.mailbox_id.clone(),
            uid: posthaste_domain::ImapUid(42),
            modseq: Some(posthaste_domain::ImapModSeq(777)),
            flags: vec!["\\Seen".to_string(), "\\Flagged".to_string()],
            rfc822_size: 512,
            has_attachment: true,
            headers: concat!(
                "From: Alice <alice@example.test>\r\n",
                "Date: Sat, 25 Apr 2026 12:00:00 +0000\r\n",
                "Subject: Parity subject\r\n",
                "Message-ID: <parity@example.test>\r\n",
                "\r\n",
            )
            .as_bytes()
            .to_vec(),
            updated_at: "2026-04-25T12:00:00Z".to_string(),
        },
    )
    .expect("IMAP header should map");
    let snapshot = ImapMailboxHeaderSnapshot {
        selected,
        headers: vec![mapped.clone()],
    };

    imap_full_sync_batch(
        &AccountId::from("imap"),
        posthaste_imap::DiscoveredImapAccount {
            capabilities: posthaste_domain::ImapCapabilities::default(),
            mailboxes: vec![map_imap_mailbox("INBOX", ["\\Inbox"])],
        },
        vec![mapped],
        vec![imap_mailbox_state_from_header_snapshot(
            &snapshot,
            "2026-04-25T12:00:00Z".to_string(),
        )],
        "2026-04-25T12:00:00Z".to_string(),
    )
}

fn parity_body() -> FetchedBody {
    FetchedBody {
        body_html: Some("<p>HTML body</p>".to_string()),
        body_text: Some("Plain body".to_string()),
        raw_mime: None,
        attachments: vec![posthaste_domain::MessageAttachment {
            id: "attachment-1".to_string(),
            blob_id: BlobId::from("jmap-blob-1"),
            part_id: Some("1".to_string()),
            filename: Some("notes.txt".to_string()),
            mime_type: "text/plain".to_string(),
            size: 13,
            disposition: Some("attachment".to_string()),
            cid: None,
            is_inline: false,
        }],
    }
}

fn parity_raw_mime() -> Vec<u8> {
    concat!(
        "From: Alice <alice@example.test>\r\n",
        "Subject: Parity subject\r\n",
        "MIME-Version: 1.0\r\n",
        "Content-Type: multipart/mixed; boundary=\"outer\"\r\n",
        "\r\n",
        "--outer\r\n",
        "Content-Type: multipart/alternative; boundary=\"inner\"\r\n",
        "\r\n",
        "--inner\r\n",
        "Content-Type: text/plain; charset=utf-8\r\n",
        "\r\n",
        "Plain body\r\n",
        "--inner\r\n",
        "Content-Type: text/html; charset=utf-8\r\n",
        "\r\n",
        "<p>HTML body</p>\r\n",
        "--inner--\r\n",
        "--outer\r\n",
        "Content-Type: text/plain; name=\"notes.txt\"\r\n",
        "Content-Disposition: attachment; filename=\"notes.txt\"\r\n",
        "\r\n",
        "attached text\r\n",
        "--outer--\r\n",
    )
    .as_bytes()
    .to_vec()
}

fn parity_attachment_blob() -> Vec<u8> {
    b"attached text".to_vec()
}
