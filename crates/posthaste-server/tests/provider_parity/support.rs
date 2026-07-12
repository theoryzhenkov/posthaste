use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use posthaste_config::TomlConfigRepository;
use posthaste_domain_model::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, BlobId, FetchedBody,
    GatewayError, Identity, MailboxId, MessageId, MutationOutcome, ReplyContext,
    SendMessageRequest, SetKeywordsCommand, SyncBatch, SyncCursor, RFC3339_EPOCH,
};
use posthaste_domain_service::{ImapMessageLocationStore, MailGateway, MailService, PushTransport};
use posthaste_store::DatabaseStore;
use posthaste_testkit::temp_root;

pub(super) struct Harness {
    // Held only to keep the temp directory alive for the harness's lifetime;
    // removed on drop.
    _root: posthaste_testkit::TempDirGuard,
    pub(super) service: MailService,
    pub(super) store: Arc<DatabaseStore>,
}

impl Harness {
    pub(super) fn new() -> Self {
        let root = temp_root("posthaste-provider-parity-test");
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
            _root: root,
            service: MailService::new(database_store.clone(), config),
            store: database_store,
        }
    }

    pub(super) fn save_account(&self, id: &str, name: &str, driver: AccountDriver) {
        self.service
            .save_source(&AccountSettings {
                id: AccountId::from(id),
                name: name.to_string(),
                full_name: None,
                signature: None,
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
pub(super) struct StaticGateway {
    batches: Arc<Mutex<VecDeque<SyncBatch>>>,
    body: FetchedBody,
    blob: Vec<u8>,
}

impl StaticGateway {
    pub(super) fn new(batch: SyncBatch, body: FetchedBody, blob: Vec<u8>) -> Self {
        Self::from_batches(vec![batch], body, blob)
    }

    pub(super) fn from_batches(batches: Vec<SyncBatch>, body: FetchedBody, blob: Vec<u8>) -> Self {
        Self {
            batches: Arc::new(Mutex::new(VecDeque::from(batches))),
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
        _progress: Option<posthaste_domain_service::SyncProgressReporter>,
    ) -> Result<SyncBatch, GatewayError> {
        Ok(self
            .batches
            .lock()
            .expect("batches lock poisoned")
            .pop_front()
            .expect("sync should have a queued batch"))
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

    async fn create_mailbox(
        &self,
        _account_id: &AccountId,
        _name: &str,
    ) -> Result<MailboxId, GatewayError> {
        Err(GatewayError::Rejected("unused".to_string()))
    }

    async fn destroy_mailbox(
        &self,
        _account_id: &AccountId,
        _mailbox_id: &MailboxId,
        _remove_emails: bool,
    ) -> Result<(), GatewayError> {
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
        _idempotency_key: &str,
    ) -> Result<posthaste_domain_model::SendFiling, GatewayError> {
        Err(GatewayError::Rejected("unused".to_string()))
    }

    fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
        Vec::new()
    }
}

pub(super) fn maybe_mailbox_roles_for_subject(
    harness: &Harness,
    account_id: &str,
    subject: &str,
) -> Option<Vec<String>> {
    let mailboxes = harness
        .service
        .list_mailboxes(&AccountId::from(account_id))
        .expect("mailboxes should list")
        .into_iter()
        .map(|mailbox| {
            (
                mailbox.id,
                mailbox
                    .role
                    .unwrap_or_else(|| mailbox.name.to_ascii_lowercase()),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let message = harness
        .service
        .list_messages(&AccountId::from(account_id), None)
        .expect("messages should list")
        .into_iter()
        .find(|message| message.subject.as_deref() == Some(subject))?;
    let mut roles = message
        .mailbox_ids
        .iter()
        .map(|mailbox_id| {
            mailboxes
                .get(mailbox_id)
                .cloned()
                .unwrap_or_else(|| mailbox_id.to_string())
        })
        .collect::<Vec<_>>();
    roles.sort();
    Some(roles)
}

pub(super) fn message_by_subject(
    harness: &Harness,
    account_id: &str,
    subject: &str,
) -> posthaste_domain_model::MessageSummary {
    harness
        .service
        .list_messages(&AccountId::from(account_id), None)
        .expect("messages should list")
        .into_iter()
        .find(|message| message.subject.as_deref() == Some(subject))
        .unwrap_or_else(|| panic!("message with subject {subject:?} should exist"))
}

pub(super) fn imap_location_count_for_subject(
    harness: &Harness,
    account_id: &str,
    subject: &str,
) -> usize {
    let message = message_by_subject(harness, account_id, subject);
    harness
        .store
        .list_imap_message_locations(&AccountId::from(account_id), &message.id)
        .expect("IMAP locations should list")
        .len()
}
