use std::collections::BTreeSet;
use std::sync::Arc;

use posthaste_config::TomlConfigRepository;
use posthaste_domain_model::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, MailboxId, MailboxRecord,
    MessageRecord, SyncBatch, SyncCursor, SyncObject, RFC3339_EPOCH,
};
use posthaste_domain_service::SyncWriteStore;
use posthaste_store::DatabaseStore;

use crate::fixture::{Fixture, FixtureAccount, FixtureDriver, FixtureError, FixtureMessage};
use crate::guard::TempDirGuard;
use crate::paths::temp_root;

/// Disposable integration harness: a config repository, a SQLite store, and a
/// `MailService` bound to them, all rooted under a fresh temp directory.
///
/// The store and service are exposed so tests can drive mutations, flush, sync,
/// and read projections directly. The temp root is a [`TempDirGuard`] guard (P6):
/// it is removed on drop, including a panicking unwind, so a failing test
/// leaves nothing behind in `$TMPDIR`.
pub struct Harness {
    pub service: posthaste_domain_service::MailService,
    pub store: Arc<DatabaseStore>,
    /// Kept alive only for its Drop side effect (recursive removal, P6).
    _root: TempDirGuard,
}

impl Harness {
    /// Opens a fresh disposable config + store + service.
    pub fn new() -> Self {
        let root = temp_root("posthaste-testkit-harness");
        let config_root = root.join("config");
        let state_root = root.join("state");
        let config_repo =
            TomlConfigRepository::open(&config_root).expect("config repository should open");
        config_repo
            .initialize_defaults()
            .expect("config defaults should initialize");
        let store = Arc::new(
            DatabaseStore::open(state_root.join("mail.sqlite"), &state_root)
                .expect("database store should open"),
        );
        let config = Arc::new(config_repo);
        Self {
            service: posthaste_domain_service::MailService::new(store.clone(), config),
            store,
            _root: root,
        }
    }

    /// Saves a source account with the given driver and transport settings.
    pub fn save_account(
        &self,
        id: &str,
        name: &str,
        driver: AccountDriver,
        transport: AccountTransportSettings,
    ) {
        self.service
            .save_source(&AccountSettings {
                id: AccountId::from(id),
                name: name.to_string(),
                full_name: Some("Dev Account".to_string()),
                signature: None,
                email_patterns: vec!["dev@example.org".to_string()],
                driver,
                enabled: true,
                appearance: None,
                transport,
                created_at: RFC3339_EPOCH.to_string(),
                updated_at: RFC3339_EPOCH.to_string(),
            })
            .expect("account should save");
    }

    /// Seed `(message_id, mailbox_id)` pairs into an account via a direct store
    /// batch (bypasses sync — for unit/integration setup). Convenience wrapper
    /// around [`seed_messages_typed`](Self::seed_messages_typed) for specs with
    /// no field overrides.
    pub fn seed_messages(&self, account_id: &AccountId, messages: &[(&str, &str)]) {
        let typed: Vec<FixtureMessage> = messages
            .iter()
            .map(|(id, mailbox)| FixtureMessage {
                id: (*id).to_string(),
                mailbox: (*mailbox).to_string(),
                subject: None,
                from_name: None,
                from_email: None,
                preview: None,
                received_at: None,
                size: None,
                keywords: None,
                thread_id: None,
                rfc_message_id: None,
            })
            .collect();
        self.seed_messages_typed(account_id, typed);
    }

    /// Seed typed fixture messages into an account via a direct store batch
    /// (bypasses sync). Each message's declared fields override the
    /// `fixture::default_message` baseline.
    pub fn seed_messages_typed(&self, account_id: &AccountId, messages: Vec<FixtureMessage>) {
        let mailbox_ids: BTreeSet<&str> = messages.iter().map(|m| m.mailbox.as_str()).collect();
        // The mailbox INSERT in apply_sync_batch persists only
        // (account_id, id, name, role); unread_emails/total_emails are
        // SQL-trigger-maintained from message rows and read directly by
        // list_mailboxes, so the values set here are informational only.
        let mailboxes: Vec<MailboxRecord> = mailbox_ids
            .iter()
            .map(|mb| MailboxRecord {
                id: MailboxId::from(*mb),
                name: mb.to_string(),
                role: Some((*mb).to_string()),
                unread_emails: 0,
                total_emails: messages.iter().filter(|m| m.mailbox == *mb).count() as i64,
            })
            .collect();
        let msgs: Vec<MessageRecord> = messages
            .into_iter()
            .map(FixtureMessage::into_record)
            .collect();
        let batch = SyncBatch {
            mailboxes,
            messages: msgs,
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: Vec::new(),
            absence_deleted_imap_message_locations: Vec::new(),
            absence_deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: vec![SyncCursor {
                object_type: SyncObject::Message,
                state: "testkit-seed".to_string(),
                updated_at: "2026-03-31T10:00:00Z".to_string(),
            }],
        };
        self.store
            .apply_sync_batch(
                &posthaste_domain_service::BaseWrite::legacy("testkit fixture seed"),
                account_id,
                &batch,
            )
            .expect("seed batch should apply");
    }

    /// Load a declarative TOML [`Fixture`](crate::fixture::Fixture) from a
    /// string, creating each account (mock driver) and seeding its messages.
    /// Returns the created account ids in declaration order.
    pub fn load_fixture_toml(&self, toml: &str) -> Result<Vec<AccountId>, FixtureError> {
        let fixture = Fixture::parse(toml)?;
        let mut accounts = Vec::with_capacity(fixture.accounts.len());
        for account in fixture.accounts {
            let id = self.load_fixture_account(account)?;
            accounts.push(id);
        }
        Ok(accounts)
    }

    /// Load a declarative TOML fixture from a file. See
    /// [`load_fixture_toml`](Self::load_fixture_toml).
    pub fn load_fixture(
        &self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<Vec<AccountId>, FixtureError> {
        let contents = std::fs::read_to_string(path)?;
        self.load_fixture_toml(&contents)
    }

    fn load_fixture_account(&self, account: FixtureAccount) -> Result<AccountId, FixtureError> {
        match account.driver {
            FixtureDriver::Mock => {
                let id = AccountId::from(account.id.as_str());
                self.save_account(
                    &account.id,
                    &account.id,
                    AccountDriver::Mock,
                    AccountTransportSettings::default(),
                );
                if !account.messages.is_empty() {
                    self.seed_messages_typed(&id, account.messages);
                }
                Ok(id)
            }
            FixtureDriver::Jmap => Err(FixtureError::UnsupportedDriver { driver: "jmap" }),
        }
    }
}

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}
