//! Update-chain continuity: an existing install's data must survive the swap
//! from the split-model desktop app (embedded `posthaste-server`) to this
//! backend, delivered as an in-place auto-update. Four seams carry that
//! guarantee, each pinned here:
//!
//! - filesystem roots — the same config/state directories resolve;
//! - keyring naming — stored provider credentials keep resolving;
//! - the store — an older-schema `mail.sqlite` migrates forward in place;
//! - the config repository — existing TOML loads unchanged.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use posthaste_client_backend::{keyring_entry_location, AppPaths, AppState, BuildOptions};
use posthaste_config::TomlConfigRepository;
use posthaste_domain_model::{
    now_iso8601, AccountDriver, AccountId, AccountSettings, AccountTransportSettings,
    MailQueryGroup, MailQueryGroupOperator, MailQueryRule, MessageSortField, SecretKind, SecretRef,
    SecretStoreError, SortDirection,
};
use posthaste_domain_service::{ConfigRepository, SecretStore};

// -- Environment control ---------------------------------------------------

/// Serializes tests that read or write process environment variables (the
/// path resolvers and the env-backed secret store are env-driven).
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Sets/unsets environment variables for one test, restoring the previous
/// values on drop.
struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn apply(vars: &[(&'static str, Option<&Path>)]) -> Self {
        let saved = vars
            .iter()
            .map(|(name, value)| {
                let previous = std::env::var(name).ok();
                match value {
                    Some(path) => std::env::set_var(name, path),
                    None => std::env::remove_var(name),
                }
                (*name, previous)
            })
            .collect();
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, previous) in &self.saved {
            match previous {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }
}

// -- 1. Path continuity ----------------------------------------------------

#[test]
fn default_roots_resolve_to_the_directories_existing_installs_use() {
    let _lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let xdg_config = dir.path().join("xdg-config");
    let xdg_data = dir.path().join("xdg-data");
    let _env = EnvGuard::apply(&[
        (posthaste_config::paths::CONFIG_ROOT_ENV, None),
        (posthaste_config::paths::STATE_ROOT_ENV, None),
        ("XDG_CONFIG_HOME", Some(&xdg_config)),
        ("XDG_DATA_HOME", Some(&xdg_data)),
    ]);

    let paths = AppPaths::resolve();

    // The backend and posthaste-config's canonical resolver — the same one
    // the embedded-server chain resolves through — must agree exactly.
    assert_eq!(paths.config_root, posthaste_config::paths::config_root());
    assert_eq!(paths.state_root, posthaste_config::paths::state_root());

    // The concrete layout is frozen: existing installs already keep their
    // data in `<XDG dir>/posthaste`, and the store file is `mail.sqlite`.
    // These literals are the on-disk contract; changing any of them orphans
    // every installed copy's data.
    assert_eq!(paths.config_root, xdg_config.join("posthaste"));
    assert_eq!(paths.state_root, xdg_data.join("posthaste"));
    assert_eq!(paths.db_path(), xdg_data.join("posthaste/mail.sqlite"));
}

#[test]
fn env_root_overrides_resolve_through_the_canonical_resolver() {
    let _lock = env_lock();
    let dir = tempfile::tempdir().unwrap();
    let config_root = dir.path().join("config-override");
    let state_root = dir.path().join("state-override");
    let _env = EnvGuard::apply(&[
        (posthaste_config::paths::CONFIG_ROOT_ENV, Some(&config_root)),
        (posthaste_config::paths::STATE_ROOT_ENV, Some(&state_root)),
    ]);

    let paths = AppPaths::resolve();
    assert_eq!(paths.config_root, config_root);
    assert_eq!(paths.state_root, state_root);
    assert_eq!(paths.config_root, posthaste_config::paths::config_root());
    assert_eq!(paths.state_root, posthaste_config::paths::state_root());
}

// -- 2. Keyring continuity ---------------------------------------------------

#[test]
fn keyring_naming_resolves_the_entries_existing_installs_wrote() {
    let secret_ref = SecretRef {
        kind: SecretKind::Os,
        key: "account:primary".to_string(),
    };
    let (service, account) = keyring_entry_location(&secret_ref);

    // The literals are the credential contract: every existing install's
    // keychain entries live under service "posthaste" with the secret-ref
    // key (as persisted in `sources/*.toml`) as the account name.
    assert_eq!(service, "posthaste");
    assert_eq!(account, "account:primary");
}

#[test]
fn env_secret_refs_resolve_by_their_key_verbatim() {
    let _lock = env_lock();
    let name: &'static str = "POSTHASTE_CONTINUITY_TEST_SECRET";
    std::env::set_var(name, "hunter2");
    let secret_ref = SecretRef {
        kind: SecretKind::Env,
        key: name.to_string(),
    };
    let resolved = posthaste_client_backend::SystemSecretStore.resolve(&secret_ref);
    std::env::remove_var(name);
    assert_eq!(resolved.expect("env secret resolves"), "hunter2");
}

// -- Shared assembly helpers -------------------------------------------------

/// In-memory secret store so tests never touch the OS keychain.
#[derive(Default)]
struct MemorySecretStore {
    secrets: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemorySecretStore {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<String, SecretStoreError> {
        self.secrets
            .lock()
            .unwrap()
            .get(&secret_ref.key)
            .cloned()
            .ok_or_else(|| SecretStoreError::Unavailable(secret_ref.key.clone()))
    }

    fn save(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.secrets
            .lock()
            .unwrap()
            .insert(secret_ref.key.clone(), value.to_string());
        Ok(())
    }

    fn update(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.save(secret_ref, value)
    }

    fn delete(&self, secret_ref: &SecretRef) -> Result<(), SecretStoreError> {
        self.secrets.lock().unwrap().remove(&secret_ref.key);
        Ok(())
    }
}

async fn assemble(paths: &AppPaths) -> AppState {
    let options = BuildOptions {
        poll_interval: Duration::from_secs(1),
        secret_store: Some(Arc::new(MemorySecretStore::default())),
        ..BuildOptions::at(paths.clone())
    };
    AppState::assemble(options).await.expect("assembles")
}

fn all_mail_rule() -> MailQueryRule {
    MailQueryRule {
        root: MailQueryGroup {
            operator: MailQueryGroupOperator::All,
            negated: false,
            nodes: Vec::new(),
        },
    }
}

fn message_count(state: &AppState) -> usize {
    state
        .service
        .query_message_page_by_rule(
            &all_mail_rule(),
            100,
            None,
            MessageSortField::Date,
            SortDirection::Desc,
        )
        .expect("message query evaluates")
        .items
        .len()
}

async fn wait_for_messages(state: &AppState) -> usize {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let count = message_count(state);
        if count > 0 {
            return count;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "mock account sync should seed messages"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn mock_account(id: &str) -> AccountSettings {
    let now = now_iso8601().expect("clock");
    AccountSettings {
        id: id.into(),
        name: format!("Mock {id}"),
        full_name: None,
        signature: None,
        email_patterns: Vec::new(),
        driver: AccountDriver::Mock,
        enabled: true,
        appearance: None,
        transport: AccountTransportSettings::default(),
        created_at: now.clone(),
        updated_at: now,
    }
}

// -- 3. Store continuity -----------------------------------------------------

fn user_version(db_path: &Path) -> i64 {
    rusqlite::Connection::open(db_path)
        .expect("raw sqlite open")
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version")
}

/// Rewind a current-shape database to the earliest (v0) schema an installed
/// copy can hold: the mailbox counter columns and their maintenance triggers
/// restored, `user_version` zeroed. Mirrors the fixture in posthaste-store's
/// schema_migrations tests.
fn downgrade_to_v0(db_path: &Path) {
    rusqlite::Connection::open(db_path)
        .expect("raw sqlite open")
        .execute_batch(
            "ALTER TABLE mailbox ADD COLUMN unread_emails INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE mailbox ADD COLUMN total_emails INTEGER NOT NULL DEFAULT 0;
             CREATE TRIGGER mailbox_counters_message_mailbox_ai
             AFTER INSERT ON message_mailbox BEGIN
                 UPDATE mailbox SET total_emails = total_emails + 1
                  WHERE account_id = new.account_id AND id = new.mailbox_id;
             END;
             CREATE TRIGGER mailbox_counters_message_mailbox_ad
             AFTER DELETE ON message_mailbox BEGIN
                 UPDATE mailbox SET total_emails = total_emails - 1
                  WHERE account_id = old.account_id AND id = old.mailbox_id;
             END;
             CREATE TRIGGER mailbox_counters_message_read_au
             AFTER UPDATE OF is_read ON message BEGIN
                 UPDATE mailbox SET unread_emails = unread_emails WHERE 0;
             END;
             PRAGMA user_version = 0;",
        )
        .expect("synthetic v0 downgrade");
}

/// Seed a non-tombstone `message_overlay` draft pin — the shape a pre-slice-3
/// database stranded — with no owning op and no base row, so the v5 migration
/// parks it as a recovered content op.
fn seed_stranded_draft_pin(db_path: &Path, account_id: &str, id: &str, subject: &str) {
    rusqlite::Connection::open(db_path)
        .expect("raw sqlite open")
        .execute(
            "INSERT INTO message_overlay (
                 account_id, id, thread_id, subject, from_name, from_email,
                 to_json, received_at, references_json, draft_id, tombstone
             ) VALUES (?1, ?2, ?2, ?3, 'Me', 'me@example.com',
                       '[{\"email\":\"ada@example.com\"}]',
                       '2026-05-01T09:00:00Z', '[]', ?2, 0)",
            rusqlite::params![account_id, id, subject],
        )
        .expect("seed stranded draft pin");
}

/// An older-schema database opens through `AppState::assemble`: the schema
/// migrates forward in place — no quarantine, no data loss — and accounts,
/// settings, and messages read back afterwards.
///
/// The reverse direction (a database stamped by a NEWER build) is refused by
/// the store's downgrade guard without touching the file — covered by
/// posthaste-store's schema_migrations test
/// `downgrade_guard_refuses_a_newer_database_without_quarantining_it`.
#[tokio::test]
async fn older_schema_database_migrates_in_place_through_assemble() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::with_roots(dir.path().join("config"), dir.path().join("state"));

    // Stand the install up once: a configured mock account whose sync seeds
    // real rows, then a clean shutdown.
    let config = TomlConfigRepository::open(&paths.config_root).expect("config opens");
    config
        .save_source(&mock_account("primary"))
        .expect("account saved");
    drop(config);
    let state = assemble(&paths).await;
    let seeded = wait_for_messages(&state).await;
    state.shutdown().await;

    let current_version = user_version(&paths.db_path());
    assert!(current_version > 0, "a fresh store stamps its version");

    // Stamp the database at the older schema shape and reopen through the
    // backend's assemble path.
    downgrade_to_v0(&paths.db_path());
    assert_eq!(user_version(&paths.db_path()), 0);

    // Seed one stranded pre-slice-3 draft pin (no owning op, no base row): the
    // v5 migration must recover it end-to-end as a parked content op, visible
    // through the same pendingOperations wire shape a live parked draft uses —
    // no wire change, no quarantine.
    seed_stranded_draft_pin(
        &paths.db_path(),
        "primary",
        "draft-stranded",
        "Recovered draft",
    );

    let state = assemble(&paths).await;
    assert!(
        state.repair.is_none(),
        "migration must run in place, never quarantine-and-rebuild"
    );
    let sources = state.service.list_sources().expect("accounts readable");
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id.as_str(), "primary");
    state.service.get_app_settings().expect("settings readable");
    assert_eq!(
        message_count(&state),
        seeded,
        "every pre-migration message survives"
    );
    // The stranded phantom came back as a parked (Failed) DraftCreate content
    // op, surfaced through the unchanged pendingOperations path.
    let pending = state
        .service
        .list_pending_operations(&AccountId::from("primary"))
        .expect("pending operations readable");
    assert!(
        pending
            .iter()
            .any(|op| op.id.as_str() == "recovered-primary-draft-stranded"),
        "the stranded draft is recovered as a parked content op: {pending:?}"
    );
    state.shutdown().await;

    assert_eq!(
        user_version(&paths.db_path()),
        current_version,
        "migrations restamp the current schema version"
    );
}

// -- 4. Config continuity ----------------------------------------------------

const EXISTING_APP_TOML: &str = r##"schema_version = 1
default_source_id = "primary"

[daemon]
bind = "127.0.0.1:3001"
poll_interval_seconds = 300

[logging]
level = "info"

[cache]
soft_cap_bytes = 1073741824
hard_cap_bytes = 2147483648
cache_bodies = true
cache_raw_messages = false
cache_attachments = true

[notifications]
new_mail = true
sound = false

[[tags]]
name = "travel"
fg = "#ffffff"
bg = "#336699"

[compose]
undo_send_delay_seconds = 20
"##;

const EXISTING_MOCK_SOURCE_TOML: &str = r#"id = "primary"
name = "Primary"
driver = "mock"
enabled = true
created_at = "2025-11-02T09:30:00Z"
updated_at = "2025-11-02T09:30:00Z"
"#;

const EXISTING_JMAP_SOURCE_TOML: &str = r#"id = "fastmail"
name = "My Fastmail"
full_name = "Example User"
email_patterns = ["user@example.com"]
driver = "jmap"
enabled = false
created_at = "2025-11-02T09:31:00Z"
updated_at = "2025-11-02T09:31:00Z"

[transport]
base_url = "https://api.fastmail.com/jmap/session"
username = "user@example.com"

[transport.secret_ref]
kind = "os"
key = "account:fastmail"
"#;

const EXISTING_SMART_MAILBOX_TOML: &str = r#"id = "projects"
name = "Projects"
kind = "user"

[rule]
operator = "any"

[[rule.nodes]]
type = "condition"
field = "subject"
operator = "contains"
value = "project"
"#;

fn write_existing_config(config_root: &Path) {
    std::fs::create_dir_all(config_root.join("sources")).unwrap();
    std::fs::create_dir_all(config_root.join("smart-mailboxes")).unwrap();
    std::fs::write(config_root.join("app.toml"), EXISTING_APP_TOML).unwrap();
    std::fs::write(
        config_root.join("sources/primary.toml"),
        EXISTING_MOCK_SOURCE_TOML,
    )
    .unwrap();
    std::fs::write(
        config_root.join("sources/fastmail.toml"),
        EXISTING_JMAP_SOURCE_TOML,
    )
    .unwrap();
    std::fs::write(
        config_root.join("smart-mailboxes/projects.toml"),
        EXISTING_SMART_MAILBOX_TOML,
    )
    .unwrap();
}

/// A config repository written by an earlier release loads through
/// `AppState::assemble` with every value intact — and assembly leaves the
/// files byte-for-byte untouched.
#[tokio::test]
async fn existing_config_toml_loads_unchanged_through_assemble() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::with_roots(dir.path().join("config"), dir.path().join("state"));
    write_existing_config(&paths.config_root);
    let files: Vec<PathBuf> = [
        "app.toml",
        "sources/primary.toml",
        "sources/fastmail.toml",
        "smart-mailboxes/projects.toml",
    ]
    .iter()
    .map(|name| paths.config_root.join(name))
    .collect();
    let before: Vec<String> = files
        .iter()
        .map(|path| std::fs::read_to_string(path).unwrap())
        .collect();

    let state = assemble(&paths).await;

    let sources = state.service.list_sources().expect("accounts load");
    assert_eq!(sources.len(), 2);
    let primary = sources
        .iter()
        .find(|source| source.id.as_str() == "primary")
        .expect("primary account");
    assert_eq!(primary.driver, AccountDriver::Mock);
    assert!(primary.enabled);
    let fastmail = sources
        .iter()
        .find(|source| source.id.as_str() == "fastmail")
        .expect("fastmail account");
    assert_eq!(fastmail.driver, AccountDriver::Jmap);
    assert!(!fastmail.enabled);
    assert_eq!(fastmail.name, "My Fastmail");
    assert_eq!(fastmail.full_name.as_deref(), Some("Example User"));
    assert_eq!(fastmail.email_patterns, vec!["user@example.com"]);
    assert_eq!(
        fastmail.transport.base_url.as_deref(),
        Some("https://api.fastmail.com/jmap/session")
    );
    assert_eq!(
        fastmail.transport.username.as_deref(),
        Some("user@example.com")
    );
    assert_eq!(
        fastmail.transport.secret_ref,
        Some(SecretRef {
            kind: SecretKind::Os,
            key: "account:fastmail".to_string(),
        })
    );

    let settings = state.service.get_app_settings().expect("settings load");
    assert_eq!(
        settings.default_account_id.as_ref().map(AccountId::as_str),
        Some("primary")
    );
    assert_eq!(settings.cache_policy.soft_cap_bytes, 1_073_741_824);
    assert_eq!(settings.cache_policy.hard_cap_bytes, 2_147_483_648);
    assert!(settings.cache_policy.cache_bodies);
    assert!(!settings.cache_policy.cache_raw_messages);
    assert!(settings.cache_policy.cache_attachments);
    let notifications = settings.notifications.expect("notifications load");
    assert_eq!(notifications.new_mail, Some(true));
    assert_eq!(notifications.sound, Some(false));
    assert_eq!(settings.tags.len(), 1);
    assert_eq!(settings.tags[0].name, "travel");
    assert_eq!(settings.tags[0].fg.as_deref(), Some("#ffffff"));
    assert_eq!(settings.tags[0].bg.as_deref(), Some("#336699"));
    let compose = settings.compose.expect("compose settings load");
    assert_eq!(compose.undo_send_delay_seconds, Some(20));

    let smart_mailboxes = state
        .config
        .list_smart_mailboxes()
        .expect("smart mailboxes load");
    assert!(
        smart_mailboxes
            .iter()
            .any(|mailbox| mailbox.id.as_str() == "projects" && mailbox.name == "Projects"),
        "user smart mailbox loads: {smart_mailboxes:?}"
    );

    state.shutdown().await;

    for (path, original) in files.iter().zip(&before) {
        assert_eq!(
            &std::fs::read_to_string(path).unwrap(),
            original,
            "assembly must not rewrite {}",
            path.display()
        );
    }
}
