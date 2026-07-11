use std::collections::HashMap;
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use posthaste_authority_server::oauth::OAuthTokenSet;
use posthaste_authority_server::{build_authority_server, from_api_bridge_for_migration};
use posthaste_client_link::RuntimeLink;
use posthaste_contract_core::RuntimeCaller;
use posthaste_contract_core::{
    AccountTransportMutation, ClientMutationId, CreateAccountMutation, MailListViewState,
    MailPresentationRequest, MailQueryRequest, MutationNotification, MutationRequest,
    MutationSettlementState, RuntimeErrorCode, RuntimeFrame, RuntimeLifecycle, RuntimeLinkSeq,
    SecretWriteMode, SecretWriteMutation, ViewDescriptor,
};
use posthaste_domain_model::{
    AccountDriver, AccountId, EventFilter, ImapTransportSettings, MailboxId, MailboxRecord,
    MessageId, MessageRecord, MessageSortField, ProviderAuthKind, ProviderHint, SecretRef,
    SecretStoreError, SmtpTransportSettings, SortDirection, SyncBatch,
    SyncCursor, SyncObject, SyncTrigger, ThreadId, TransportSecurity, EVENT_TOPIC_ACCOUNT_DELETED,
    EVENT_TOPIC_MESSAGE_UPDATED,
};
use posthaste_domain_service::SecretStore;
use posthaste_engine::MockJmapGateway;
use posthaste_runtime::{RuntimeBuildConfig, RuntimeBuildError};
use posthaste_runtime_api::RuntimeAccountApi;
use tempfile::TempDir;
use tokio::sync::Notify;

// This integration-test binary is a separate compilation unit from the crate's
// `src/`, so it cannot see the private `posthaste_authority_server::test_support`
// module — it carries its own small copy of the same RAII tempdir guard (P6).
// See `crates/posthaste-authority-server/src/test_support.rs` for the shared
// version used by the crate's own `#[cfg(test)]` modules.

/// RAII guard for a disposable temp directory. Removed on drop, including a
/// panicking unwind — keep the guard bound for as long as the directory needs
/// to exist. `Deref`/`AsRef` let it stand in for the `PathBuf` the call sites
/// used to bind.
struct TempDirGuard(TempDir);

impl Deref for TempDirGuard {
    type Target = Path;

    fn deref(&self) -> &Path {
        self.0.path()
    }
}

impl AsRef<Path> for TempDirGuard {
    fn as_ref(&self) -> &Path {
        self.0.path()
    }
}

fn temp_root() -> TempDirGuard {
    TempDirGuard(
        tempfile::Builder::new()
            .prefix("posthaste-authority-server-test-")
            .tempdir()
            .expect("temp dir should be created"),
    )
}

#[derive(Default)]
struct TestSecretStore {
    values: Mutex<HashMap<String, String>>,
}

impl TestSecretStore {
    fn value(&self, secret_ref: &SecretRef) -> Option<String> {
        self.values
            .lock()
            .expect("secret store mutex")
            .get(&secret_key(secret_ref))
            .cloned()
    }
}

impl SecretStore for TestSecretStore {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<String, SecretStoreError> {
        self.values
            .lock()
            .expect("secret store mutex")
            .get(&secret_key(secret_ref))
            .cloned()
            .ok_or_else(|| SecretStoreError::Unavailable("secret not found".to_string()))
    }

    fn save(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.values
            .lock()
            .expect("secret store mutex")
            .insert(secret_key(secret_ref), value.to_string());
        Ok(())
    }

    fn update(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.save(secret_ref, value)
    }

    fn delete(&self, secret_ref: &SecretRef) -> Result<(), SecretStoreError> {
        self.values
            .lock()
            .expect("secret store mutex")
            .remove(&secret_key(secret_ref));
        Ok(())
    }
}

fn secret_key(secret_ref: &SecretRef) -> String {
    format!("{:?}:{}", secret_ref.kind, secret_ref.key)
}

fn mock_account_mutation(account_id: &str) -> CreateAccountMutation {
    CreateAccountMutation {
        id: Some(account_id.to_string()),
        name: account_id.to_string(),
        driver: Some(AccountDriver::Mock),
        enabled: Some(false),
        full_name: None,
        signature: None,
        email_patterns: Vec::new(),
        appearance: None,
        transport: AccountTransportMutation::default(),
        secret: SecretWriteMutation::default(),
    }
}

fn seeded_message(message_id: &str, mailbox_id: &str) -> MessageRecord {
    MessageRecord {
        id: MessageId::from(message_id),
        source_thread_id: ThreadId::from(format!("thread-{message_id}")),
        subject: Some(format!("Subject {message_id}")),
        from_name: Some("Alice".to_string()),
        from_email: Some("alice@example.com".to_string()),
        preview: Some("Preview".to_string()),
        received_at: "2026-03-31T10:00:00Z".to_string(),
        size: 42,
        mailbox_ids: vec![MailboxId::from(mailbox_id)],
        keywords: vec!["$seen".to_string()],
        rfc_message_id: Some(format!("<{message_id}@example.test>")),
        ..Default::default()
    }
}

fn seed_message_batch(
    build: &posthaste_authority_server::AuthorityServerBuild,
    account_id: &AccountId,
) {
    build
        .api_bridge
        .store
        .apply_sync_batch(
            &posthaste_domain_service::BaseWrite::legacy("test base seed"),
            account_id,
            &SyncBatch {
                mailboxes: vec![MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 2,
                }],
                messages: vec![
                    seeded_message("message-1", "inbox"),
                    seeded_message("message-2", "inbox"),
                ],
                imap_mailbox_states: Vec::new(),
                imap_message_locations: Vec::new(),
                deleted_imap_message_locations: Vec::new(),
                deleted_mailbox_ids: Vec::new(),
                absence_deleted_imap_message_locations: Vec::new(),
                absence_deleted_message_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: false,
                replace_all_messages: false,
                cursors: vec![SyncCursor {
                    object_type: SyncObject::Message,
                    state: "state-1".to_string(),
                    updated_at: "2026-03-31T10:00:00Z".to_string(),
                }],
            },
        )
        .expect("message batch should apply");
}

fn seed_single_message_batch(
    build: &posthaste_authority_server::AuthorityServerBuild,
    account_id: &AccountId,
    message_id: &str,
    mailbox_id: &str,
) {
    build
        .api_bridge
        .store
        .apply_sync_batch(
            &posthaste_domain_service::BaseWrite::legacy("test base seed"),
            account_id,
            &SyncBatch {
                mailboxes: vec![MailboxRecord {
                    id: MailboxId::from(mailbox_id),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 1,
                }],
                messages: vec![seeded_message(message_id, mailbox_id)],
                imap_mailbox_states: Vec::new(),
                imap_message_locations: Vec::new(),
                deleted_imap_message_locations: Vec::new(),
                deleted_mailbox_ids: Vec::new(),
                absence_deleted_imap_message_locations: Vec::new(),
                absence_deleted_message_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: false,
                replace_all_messages: false,
                cursors: vec![SyncCursor {
                    object_type: SyncObject::Message,
                    state: "message-1".to_string(),
                    updated_at: "2026-03-31T10:00:00Z".to_string(),
                }],
            },
        )
        .expect("message batch should apply");
}

/// Seed a message in `mb-inbox` alongside a Snoozed mailbox (`mb-snooze`, role
/// "snooze") so `message.snooze` can resolve the snooze-role mailbox.
fn seed_message_with_snooze_mailbox(
    build: &posthaste_authority_server::AuthorityServerBuild,
    account_id: &AccountId,
    message_id: &str,
) {
    build
        .api_bridge
        .store
        .apply_sync_batch(
            &posthaste_domain_service::BaseWrite::legacy("test base seed"),
            account_id,
            &SyncBatch {
                mailboxes: vec![
                    MailboxRecord {
                        id: MailboxId::from("mb-inbox"),
                        name: "Inbox".to_string(),
                        role: Some("inbox".to_string()),
                        unread_emails: 0,
                        total_emails: 1,
                    },
                    MailboxRecord {
                        id: MailboxId::from("mb-snooze"),
                        name: "Snoozed".to_string(),
                        role: Some("snooze".to_string()),
                        unread_emails: 0,
                        total_emails: 0,
                    },
                ],
                messages: vec![seeded_message(message_id, "mb-inbox")],
                imap_mailbox_states: Vec::new(),
                imap_message_locations: Vec::new(),
                deleted_imap_message_locations: Vec::new(),
                deleted_mailbox_ids: Vec::new(),
                absence_deleted_imap_message_locations: Vec::new(),
                absence_deleted_message_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: false,
                replace_all_messages: false,
                cursors: vec![SyncCursor {
                    object_type: SyncObject::Message,
                    state: "message-1".to_string(),
                    updated_at: "2026-03-31T10:00:00Z".to_string(),
                }],
            },
        )
        .expect("message + snooze mailbox batch should apply");
}

/// Seed one message whose cached body is large (an attachment-shaped email with
/// a big inline body). Used to prove the mutation settlement payload stays
/// bounded regardless of body size.
fn seed_heavy_body_message_batch(
    build: &posthaste_authority_server::AuthorityServerBuild,
    account_id: &AccountId,
    message_id: &str,
    mailbox_id: &str,
    body_bytes: usize,
) {
    let mut message = seeded_message(message_id, mailbox_id);
    let filler = "x".repeat(body_bytes);
    message.body_html = Some(format!("<p>{filler}</p>"));
    message.body_text = Some(filler);
    build
        .api_bridge
        .store
        .apply_sync_batch(
            &posthaste_domain_service::BaseWrite::legacy("test base seed"),
            account_id,
            &SyncBatch {
                mailboxes: vec![MailboxRecord {
                    id: MailboxId::from(mailbox_id),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 1,
                }],
                messages: vec![message],
                imap_mailbox_states: Vec::new(),
                imap_message_locations: Vec::new(),
                deleted_imap_message_locations: Vec::new(),
                deleted_mailbox_ids: Vec::new(),
                absence_deleted_imap_message_locations: Vec::new(),
                absence_deleted_message_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: false,
                replace_all_messages: false,
                cursors: vec![SyncCursor {
                    object_type: SyncObject::Message,
                    state: "message-heavy".to_string(),
                    updated_at: "2026-03-31T10:00:00Z".to_string(),
                }],
            },
        )
        .expect("message batch should apply");
}

fn mail_list_descriptor(query: &str) -> ViewDescriptor {
    mail_list_descriptor_with_limit(query, 10)
}

fn mail_list_descriptor_with_limit(query: &str, limit: usize) -> ViewDescriptor {
    let request = MailQueryRequest {
        query: query.to_string(),
        presentation: MailPresentationRequest::Messages {
            limit: Some(limit),
            cursor: None,
            sort_field: MessageSortField::Date,
            sort_direction: SortDirection::Desc,
        },
        visibility: None,
    };
    ViewDescriptor {
        family: "mailList".to_string(),
        payload: serde_json::to_value(request).expect("request should serialize"),
        ..Default::default()
    }
}

fn mail_list_state(snapshot: &posthaste_contract_core::ViewSnapshot) -> MailListViewState {
    serde_json::from_value(snapshot.data.clone()).expect("snapshot data should be mail list state")
}

fn message_detail_descriptor(source_id: &str, message_id: &str) -> ViewDescriptor {
    ViewDescriptor {
        family: "messageDetail".to_string(),
        payload: serde_json::json!({ "sourceId": source_id, "messageId": message_id }),
        ..Default::default()
    }
}

fn scoped_test_caller(source_id: &str) -> RuntimeCaller {
    let mut caller = RuntimeCaller::test();
    caller.account_scope = Some(vec![source_id.to_string()]);
    caller
}

fn imap_smtp_account_mutation(
    account_id: &str,
    name: &str,
    password: &str,
) -> CreateAccountMutation {
    CreateAccountMutation {
        id: Some(account_id.to_string()),
        name: name.to_string(),
        full_name: None,
        signature: None,
        email_patterns: vec![format!("{account_id}@example.com")],
        driver: Some(AccountDriver::ImapSmtp),
        enabled: Some(false),
        appearance: None,
        transport: AccountTransportMutation {
            provider: Some(ProviderHint::Generic),
            auth: Some(ProviderAuthKind::Password),
            base_url: None,
            username: Some(format!("{account_id}@example.com")),
            imap: Some(ImapTransportSettings {
                host: "imap.example.com".to_string(),
                port: 993,
                security: TransportSecurity::Tls,
            }),
            smtp: Some(SmtpTransportSettings {
                host: "smtp.example.com".to_string(),
                port: 465,
                security: TransportSecurity::Tls,
            }),
        },
        secret: SecretWriteMutation {
            mode: SecretWriteMode::Replace,
            password: Some(password.to_string()),
        },
    }
}

// spec: docs/eph/PLAN-L2-bundled-app-test-plan#authority-runtime-handle-test-first
// spec: docs/runtime/internals/L2#runtime-builder-transport-free
// spec: docs/authority-server/L2#runtime-build-before-adapters
/// NS1 test helper: simulate the provider flagging a message — a BASE write
/// (sync-plane, legacy-witnessed) returning the sync-apply events for the test
/// to broadcast. Replaces the deleted store-level `set_keywords` seam.
trait FlagInBase {
    fn flag_message_in_base(
        &self,
        account_id: &AccountId,
        message_id: &str,
    ) -> Result<posthaste_domain_model::CommandResult, posthaste_domain_model::StoreError>;
}

impl FlagInBase for std::sync::Arc<dyn posthaste_domain_service::MailStore> {
    fn flag_message_in_base(
        &self,
        account_id: &AccountId,
        message_id: &str,
    ) -> Result<posthaste_domain_model::CommandResult, posthaste_domain_model::StoreError> {
        let id = MessageId::from(message_id);
        let mut row = self
            .read_base_message_record(account_id, &id)?
            .ok_or_else(|| posthaste_domain_model::StoreError::NotFound(id.to_string()))?;
        if !row.keywords.iter().any(|keyword| keyword == "$flagged") {
            row.keywords.push("$flagged".to_string());
        }
        let events = self.apply_sync_batch(
            &posthaste_domain_service::BaseWrite::legacy("test base seed"),
            account_id,
            &posthaste_domain_model::SyncBatch {
                messages: vec![row],
                ..Default::default()
            },
        )?;
        Ok(posthaste_domain_model::CommandResult {
            detail: None,
            events,
        })
    }
}

#[tokio::test]
async fn build_from_empty_roots_reports_ready_status_without_http_or_tauri() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));

    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build from empty roots");

    assert_eq!(build.runtime_status.lifecycle, RuntimeLifecycle::Ready);
    assert!(build.runtime_status.store.config_loaded);
    assert!(build.runtime_status.store.state_store_open);
    assert!(build.runtime_status.store.cache_root_ready);
    assert_eq!(build.runtime_status.account_count, 0);
    assert!(root.join("config/app.toml").exists());
    assert!(root.join("state/mail.sqlite").exists());
    assert!(root.join("cache").is_dir());

    let handle = build.handle.clone();
    let status = handle
        .runtime_status(RuntimeCaller::test())
        .await
        .expect("runtime status should be readable through RuntimeCore");
    assert_eq!(status, build.runtime_status);

    build
        .shutdown
        .shutdown()
        .await
        .expect("shutdown should succeed for first-slice runtime");
    let stopped_status = handle
        .runtime_status(RuntimeCaller::test())
        .await
        .expect("runtime status should remain readable after shutdown");
    assert_eq!(stopped_status.lifecycle, RuntimeLifecycle::Stopped);
}

// spec: docs/runtime/internals/L3#authority-build-order
#[tokio::test]
async fn stopped_runtime_rejects_reads_but_keeps_status_readable() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let handle = build.handle.clone();

    build
        .shutdown
        .shutdown()
        .await
        .expect("shutdown should succeed");

    let status = handle
        .runtime_status(RuntimeCaller::test())
        .await
        .expect("runtime status should remain readable after shutdown");
    assert_eq!(status.lifecycle, RuntimeLifecycle::Stopped);
    let error = handle
        .list_accounts(RuntimeCaller::test())
        .await
        .expect_err("reads should be rejected after shutdown");
    assert_eq!(error.envelope().code, RuntimeErrorCode::RuntimeNotReady);
    assert_eq!(error.envelope().message, "runtime is stopped");
    assert_eq!(error.envelope().details["lifecycle"], "stopped");
}

// spec: docs/runtime/internals/L3#authority-build-order
#[tokio::test]
async fn stopped_runtime_rejects_mutations_before_mutation_service_lookup() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let handle = build.handle.clone();

    build
        .shutdown
        .shutdown()
        .await
        .expect("shutdown should succeed");

    let error = handle
        .create_account(RuntimeCaller::test(), mock_account_mutation("after-stop"))
        .await
        .expect_err("mutations should be rejected after shutdown");
    assert_eq!(error.envelope().code, RuntimeErrorCode::RuntimeNotReady);
    assert_eq!(error.envelope().message, "runtime is stopped");
}

// spec: docs/runtime/internals/L3#authority-build-order
#[tokio::test]
async fn active_migration_handle_without_mutations_reports_missing_mutation_service() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let handle =
        from_api_bridge_for_migration(build.api_bridge.clone(), build.runtime_status.account_count);

    let status = handle
        .runtime_status(RuntimeCaller::test())
        .await
        .expect("migration handle should report status");
    assert_eq!(status.lifecycle, RuntimeLifecycle::Ready);
    let error = handle
        .create_account(
            RuntimeCaller::test(),
            mock_account_mutation("missing-mutations"),
        )
        .await
        .expect_err("active migration handle should report missing mutation service");
    assert_eq!(error.envelope().code, RuntimeErrorCode::RuntimeNotReady);
    assert_eq!(
        error.envelope().message,
        "account mutation runtime is not available"
    );
}

// spec: docs/runtime/internals/L3#authority-build-order
#[tokio::test]
async fn authority_builder_handle_supports_account_mutations() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));

    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");

    let created = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            CreateAccountMutation {
                id: Some("acct-builder".to_string()),
                name: "Builder Account".to_string(),
                driver: Some(posthaste_domain_model::AccountDriver::Mock),
                enabled: Some(false),
                full_name: None,
                signature: None,
                email_patterns: Vec::new(),
                appearance: None,
                transport: AccountTransportMutation::default(),
                secret: SecretWriteMutation::default(),
            },
        )
        .await
        .expect("builder handle should support account mutations");

    assert_eq!(created.id.as_str(), "acct-builder");
    assert_eq!(created.name, "Builder Account");
}

// spec: docs/authority-server/L3#account-mutations-runtime-backed
#[tokio::test]
async fn mail_list_view_replaces_snapshot_after_keyword_event() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mock_account_mutation("view-account"))
        .await
        .expect("account should create");
    seed_message_batch(&build, &account.id);

    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");
    let snapshot = build
        .handle
        .open_link_view(
            RuntimeCaller::test(),
            link.link_id.clone(),
            mail_list_descriptor("in:view-account/inbox"),
        )
        .await
        .expect("mail list view should open");
    assert_eq!(snapshot.revision.get(), 1);
    let state = mail_list_state(&snapshot);
    assert_eq!(state.rows.len(), 2);

    let mut subscription = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            link.link_id.clone(),
            Some(RuntimeLinkSeq::new(1)),
        )
        .await
        .expect("runtime stream should subscribe");
    // The link's collapse-on-first-subscribe (D50) always re-serves the open
    // view's current state as catch-up (there is no cheaper "already caught up"
    // signal at the link level the way the retired per-view protocol had it).
    let RuntimeFrame::ViewSnapshot { snapshot, .. } = subscription
        .catch_up
        .into_iter()
        .next()
        .expect("first subscribe should catch up with the open view's snapshot")
    else {
        panic!("catch-up should be a view snapshot");
    };
    assert_eq!(snapshot.revision.get(), 1);

    let result = build
        .api_bridge
        .store
        .flag_message_in_base(&account.id, "message-1")
        .expect("keyword command should write");
    for event in result.events {
        build
            .api_bridge
            .event_sender
            .send(event)
            .expect("event should broadcast");
    }

    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let frame = subscription
                .live
                .next()
                .await
                .expect("runtime stream should remain open");
            if matches!(frame, RuntimeFrame::ViewReplace { .. }) {
                break frame;
            }
        }
    })
    .await
    .expect("view frame should arrive");
    let RuntimeFrame::ViewReplace { snapshot, .. } = frame else {
        panic!("expected replace frame");
    };
    assert_eq!(snapshot.revision.get(), 2);
    let state = mail_list_state(&snapshot);
    let row = state
        .rows
        .iter()
        .find(|row| row.projection["id"] == "message-1")
        .expect("updated row should remain in window");
    assert_eq!(row.projection["isFlagged"], true);
    assert_eq!(
        row.projection["keywords"],
        serde_json::json!(["$flagged", "$seen"])
    );

    let catch_up = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            link.link_id.clone(),
            Some(RuntimeLinkSeq::new(0)),
        )
        .await
        .expect("behind subscriber should get collapsed catch-up")
        .catch_up;
    let RuntimeFrame::ViewSnapshot { snapshot, .. } = catch_up
        .into_iter()
        .next()
        .expect("behind subscriber should get a snapshot frame")
    else {
        panic!("catch-up should be a fresh snapshot");
    };
    assert_eq!(snapshot.revision.get(), 2);
}

#[tokio::test]
async fn runtime_link_ids_are_not_predictable_counters() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");

    let first = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("first link should open");
    let second = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("second link should open");

    for link_id in [&first.link_id, &second.link_id] {
        let raw = link_id.as_str();
        let suffix = raw
            .strip_prefix("link-")
            .expect("link id should carry the link prefix");
        assert_eq!(suffix.len(), 32);
        assert!(suffix.chars().all(|ch| ch.is_ascii_hexdigit()));
    }
    assert_ne!(first.link_id.as_str(), "link-1");
    assert_ne!(second.link_id.as_str(), "link-2");
}

#[tokio::test]
async fn runtime_link_stream_carries_keyword_view_replace_frames() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let account = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            mock_account_mutation("runtime-link-account"),
        )
        .await
        .expect("account should create");
    seed_message_batch(&build, &account.id);

    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");
    let snapshot = build
        .handle
        .open_link_view(
            RuntimeCaller::test(),
            link.link_id.clone(),
            mail_list_descriptor("in:runtime-link-account/inbox"),
        )
        .await
        .expect("link view should open");
    let mut subscription = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            link.link_id.clone(),
            Some(RuntimeLinkSeq::new(0)),
        )
        .await
        .expect("runtime stream should subscribe");
    assert_eq!(subscription.catch_up.len(), 1);
    let RuntimeFrame::ViewSnapshot {
        link_seq,
        view_id,
        revision,
        ..
    } = &subscription.catch_up[0]
    else {
        panic!("expected collapsed view snapshot");
    };
    assert_eq!(link_seq.get(), 1);
    assert_eq!(view_id, &snapshot.view_id);
    assert_eq!(revision.get(), 1);

    let result = build
        .api_bridge
        .store
        .flag_message_in_base(&account.id, "message-1")
        .expect("keyword command should write");
    for event in result.events {
        build
            .api_bridge
            .event_sender
            .send(event)
            .expect("event should broadcast");
    }

    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let frame = subscription
                .live
                .next()
                .await
                .expect("runtime stream should remain open");
            if matches!(frame, RuntimeFrame::ViewReplace { .. }) {
                break frame;
            }
        }
    })
    .await
    .expect("runtime replace frame should arrive");
    let RuntimeFrame::ViewReplace {
        link_seq,
        view_id,
        revision,
        snapshot,
    } = frame
    else {
        panic!("expected runtime view replace frame");
    };
    assert!(link_seq.get() >= 2);
    assert_eq!(view_id, snapshot.view_id);
    assert_eq!(revision.get(), 2);
    let state = mail_list_state(&snapshot);
    let row = state
        .rows
        .iter()
        .find(|row| row.projection["id"] == "message-1")
        .expect("updated row should remain in window");
    assert_eq!(row.projection["isFlagged"], true);
}

#[tokio::test]
async fn runtime_mutation_streams_settlement_frames() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let mut mutation = mock_account_mutation("runtime-mutation-account");
    mutation.enabled = Some(true);
    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mutation)
        .await
        .expect("account should create");
    build
        .account_supervisor
        .sync_account(&account.id)
        .await
        .expect("mock account runtime should sync");
    seed_single_message_batch(&build, &account.id, "em-001", "mb-inbox");
    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");
    let mut subscription = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            link.link_id.clone(),
            Some(RuntimeLinkSeq::new(0)),
        )
        .await
        .expect("runtime stream should subscribe");
    assert!(subscription.catch_up.is_empty());

    let receipt = build
        .handle
        .forward_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                link_id: Some(link.link_id.clone()),
                operation: serde_json::from_value(serde_json::json!({
                    "name": "message.setKeywords",
                    "args": serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "em-001",
                    "command": {"add": ["$flagged"], "remove": []}
                }),
                }))
                .expect("typed operation parses"),
                client_mutation_id: ClientMutationId::new("client-1"),
                context: None,
            },
        )
        .await
        .expect("mutation should run");
    let mutation_id = receipt
        .runtime_mutation_id
        .clone()
        .expect("runtime mutation id should be assigned");
    assert_eq!(receipt.client_mutation_id.as_str(), "client-1");
    assert_eq!(receipt.name, "message.setKeywords");
    assert_eq!(receipt.state, MutationSettlementState::Confirmed);
    assert_eq!(receipt.output["events"].as_array().unwrap().len(), 1);

    // The runtime emits a single terminal verdict, keyed by the client mutation
    // id, with no non-terminal `Accepted` frame (the mutation.notification
    // model: only terminal outcomes are emitted).
    let notification = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let frame = subscription
                .live
                .next()
                .await
                .expect("runtime stream should remain open");
            if let RuntimeFrame::MutationNotification {
                client_mutation_id,
                notification,
                ..
            } = frame
            {
                break (client_mutation_id, notification);
            }
        }
    })
    .await
    .expect("a mutation notification should arrive");
    assert_eq!(notification.0.as_str(), "client-1");
    assert_eq!(notification.1, MutationNotification::Confirmed);

    let duplicate = build
        .handle
        .forward_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                link_id: Some(link.link_id),
                operation: serde_json::from_value(serde_json::json!({
                    "name": "message.setKeywords",
                    "args": serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "em-001",
                    "command": {"add": ["$flagged"], "remove": []}
                }),
                }))
                .expect("typed operation parses"),
                client_mutation_id: ClientMutationId::new("client-1"),
                context: None,
            },
        )
        .await
        .expect("duplicate mutation should return existing receipt");
    assert_eq!(duplicate.runtime_mutation_id, Some(mutation_id));
    assert_eq!(duplicate.state, MutationSettlementState::Confirmed);
    // Wire-contract pin (v0.5.0 field bug): the re-observed receipt carries the
    // command result exactly like the first — `output.events` as an array.
    assert!(
        duplicate.output["events"].is_array(),
        "a re-observed confirmed receipt must carry output.events as an array, got {}",
        duplicate.output
    );
}

/// Cost contract: a state-assertion mutation acknowledges the change; it must
/// NOT shuttle the message body. The settlement payload (`receipt.output`,
/// serialized onto the link stream) must stay bounded regardless of how
/// large the message's cached body is — otherwise archive/delete/keyword ops on
/// attachment-shaped messages pay a load + serialize + transfer tax for data the
/// client discards. This is the regression that shipped; the bound makes it
/// catchable. See docs/replication/client-link/L3 §5 / mutation-pipeline cost notes.
#[tokio::test]
async fn message_mutation_settlement_payload_excludes_the_message_body() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let mut mutation = mock_account_mutation("settlement-size-account");
    mutation.enabled = Some(true);
    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mutation)
        .await
        .expect("account should create");
    build
        .account_supervisor
        .sync_account(&account.id)
        .await
        .expect("mock account runtime should sync");

    // A 256 KiB cached body — the shape of an attachment-bearing email.
    let body_bytes = 256 * 1024;
    seed_heavy_body_message_batch(&build, &account.id, "em-001", "mb-inbox", body_bytes);
    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");

    let receipt = build
        .handle
        .forward_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                link_id: Some(link.link_id.clone()),
                operation: serde_json::from_value(serde_json::json!({
                    "name": "message.setKeywords",
                    "args": serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "em-001",
                    "command": {"add": ["$flagged"], "remove": []}
                }),
                }))
                .expect("typed operation parses"),
                client_mutation_id: ClientMutationId::new("client-1"),
                context: None,
            },
        )
        .await
        .expect("mutation should run");
    assert_eq!(receipt.state, MutationSettlementState::Confirmed);

    let output_bytes = serde_json::to_string(&receipt.output)
        .expect("settlement output should serialize")
        .len();
    assert!(
        output_bytes < 8 * 1024,
        "settlement payload was {output_bytes} bytes for a {body_bytes}-byte body; \
         a state-assertion settlement must acknowledge the change, not carry the body"
    );
}

// spec: docs/runtime/L1#view-operation
#[tokio::test]
async fn message_detail_view_replaces_snapshot_after_keyword_event() {
    // The messageDetail view family serves the overlay-folded detail and pushes
    // a replacement when its own message changes — the read surface the renderer
    // subscribes to instead of patching a local cache.
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let account = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            mock_account_mutation("detail-account"),
        )
        .await
        .expect("account should create");
    seed_message_batch(&build, &account.id);

    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");
    let snapshot = build
        .handle
        .open_link_view(
            RuntimeCaller::test(),
            link.link_id.clone(),
            message_detail_descriptor(account.id.as_str(), "message-1"),
        )
        .await
        .expect("message detail view should open");
    assert_eq!(snapshot.revision.get(), 1);
    assert_eq!(snapshot.data["id"], "message-1");
    assert_eq!(snapshot.data["isFlagged"], false);

    let mut subscription = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            link.link_id.clone(),
            Some(RuntimeLinkSeq::new(1)),
        )
        .await
        .expect("runtime stream should subscribe");
    // The link's collapse-on-first-subscribe (D50) always re-serves the open
    // view's current state as catch-up.
    let RuntimeFrame::ViewSnapshot { snapshot, .. } = subscription
        .catch_up
        .into_iter()
        .next()
        .expect("first subscribe should catch up with the open view's snapshot")
    else {
        panic!("catch-up should be a view snapshot");
    };
    assert_eq!(snapshot.revision.get(), 1);

    let result = build
        .api_bridge
        .store
        .flag_message_in_base(&account.id, "message-1")
        .expect("keyword command should write");
    for event in result.events {
        build
            .api_bridge
            .event_sender
            .send(event)
            .expect("event should broadcast");
    }

    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let frame = subscription
                .live
                .next()
                .await
                .expect("runtime stream should remain open");
            if matches!(frame, RuntimeFrame::ViewReplace { .. }) {
                break frame;
            }
        }
    })
    .await
    .expect("view frame should arrive");
    let RuntimeFrame::ViewReplace { snapshot, .. } = frame else {
        panic!("expected replace frame");
    };
    assert_eq!(snapshot.revision.get(), 2);
    assert_eq!(snapshot.data["id"], "message-1");
    assert_eq!(snapshot.data["isFlagged"], true);
}

// spec: docs/runtime/L1#view-operation
#[tokio::test]
async fn conversation_view_replaces_snapshot_after_keyword_event() {
    // The conversation view family serves the overlay-folded conversation and
    // recomputes when a message changes.
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mock_account_mutation("conv-account"))
        .await
        .expect("account should create");
    seed_message_batch(&build, &account.id);

    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");

    // Resolve message-1's conversation id from a list snapshot.
    let list = build
        .handle
        .open_link_view(
            RuntimeCaller::test(),
            link.link_id.clone(),
            mail_list_descriptor("in:conv-account/inbox"),
        )
        .await
        .expect("mail list view should open");
    let list_state = mail_list_state(&list);
    let row = list_state
        .rows
        .iter()
        .find(|row| row.projection["id"] == "message-1")
        .expect("message-1 row");
    let conversation_id = row.projection["conversationId"]
        .as_str()
        .expect("row carries a conversation id")
        .to_string();

    let snapshot = build
        .handle
        .open_link_view(
            RuntimeCaller::test(),
            link.link_id.clone(),
            ViewDescriptor {
                family: "conversation".to_string(),
                payload: serde_json::json!({ "conversationId": conversation_id }),
                ..Default::default()
            },
        )
        .await
        .expect("conversation view should open");
    assert_eq!(snapshot.revision.get(), 1);
    let message = snapshot.data["messages"]
        .as_array()
        .expect("conversation messages")
        .iter()
        .find(|message| message["id"] == "message-1")
        .expect("message-1 in conversation");
    assert_eq!(message["isFlagged"], false);

    let mut subscription = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            link.link_id.clone(),
            Some(RuntimeLinkSeq::new(2)),
        )
        .await
        .expect("runtime stream should subscribe");
    // The link's collapse-on-first-subscribe (D50) always re-serves every open
    // view's current state as catch-up — both the mail-list and conversation
    // views opened above.
    assert_eq!(subscription.catch_up.len(), 2);

    let result = build
        .api_bridge
        .store
        .flag_message_in_base(&account.id, "message-1")
        .expect("keyword command should write");
    for event in result.events {
        build
            .api_bridge
            .event_sender
            .send(event)
            .expect("event should broadcast");
    }

    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let frame = subscription
                .live
                .next()
                .await
                .expect("runtime stream should remain open");
            if matches!(&frame, RuntimeFrame::ViewReplace { view_id, .. } if view_id == &snapshot.view_id)
            {
                break frame;
            }
        }
    })
    .await
    .expect("view frame should arrive");
    let RuntimeFrame::ViewReplace { snapshot, .. } = frame else {
        panic!("expected replace frame");
    };
    let message = snapshot.data["messages"]
        .as_array()
        .expect("conversation messages")
        .iter()
        .find(|message| message["id"] == "message-1")
        .expect("message-1 in conversation");
    assert_eq!(message["isFlagged"], true);
}

// spec: docs/runtime/mutations/L1#mutation-pipeline-and-catalog
#[tokio::test]
async fn runtime_set_read_state_mutation_routes_through_the_catalog() {
    // A catalog mutation beyond setKeywords routes to its handle action and
    // settles confirmed (read/flag/move/destroy all share this path).
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let mut mutation = mock_account_mutation("runtime-readstate-account");
    mutation.enabled = Some(true);
    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mutation)
        .await
        .expect("account should create");
    build
        .account_supervisor
        .sync_account(&account.id)
        .await
        .expect("mock account runtime should sync");
    seed_single_message_batch(&build, &account.id, "em-001", "mb-inbox");
    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");

    let receipt = build
        .handle
        .forward_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                link_id: Some(link.link_id.clone()),
                operation: serde_json::from_value(serde_json::json!({
                    "name": "message.setReadState",
                    "args": serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "em-001",
                    "read": true
                }),
                }))
                .expect("typed operation parses"),
                client_mutation_id: ClientMutationId::new("read-1"),
                context: None,
            },
        )
        .await
        .expect("setReadState mutation should run");
    assert_eq!(receipt.name, "message.setReadState");
    assert_eq!(receipt.state, MutationSettlementState::Confirmed);
    assert_eq!(receipt.output["events"].as_array().unwrap().len(), 1);

    // Post-M5 the operation vocabulary is typed: an unknown mutation name can
    // no longer be *constructed* — it is rejected at the wire parse (the serde
    // deserialization of `MutationRequest` IS the operation parse, D8).
    let unknown = serde_json::from_value::<MutationRequest>(serde_json::json!({
        "linkId": link.link_id.as_str(),
        "name": "message.nonsense",
        "args": {},
        "clientMutationId": "bad-1",
    }));
    assert!(
        unknown.is_err(),
        "unknown mutation names are rejected at the wire parse"
    );
}

#[tokio::test]
async fn runtime_link_view_extends_its_window_in_place() {
    // A windowed mailList view grows in place: extend re-queries the larger
    // window, keeps the same view id, and broadcasts a ViewReplace.
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let account = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            mock_account_mutation("runtime-extend-account"),
        )
        .await
        .expect("account should create");
    seed_message_batch(&build, &account.id);
    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");

    // Open with a one-row window so the second seeded message is past it.
    let snapshot = build
        .handle
        .open_link_view(
            RuntimeCaller::test(),
            link.link_id.clone(),
            mail_list_descriptor_with_limit("in:runtime-extend-account/inbox", 1),
        )
        .await
        .expect("link view should open");
    let opened = mail_list_state(&snapshot);
    assert_eq!(opened.rows.len(), 1);
    assert!(opened.continuation.has_after, "more rows past the window");

    let mut subscription = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            link.link_id.clone(),
            Some(RuntimeLinkSeq::new(0)),
        )
        .await
        .expect("runtime stream should subscribe");

    let extended = build
        .handle
        .extend_link_view(
            RuntimeCaller::test(),
            link.link_id.clone(),
            snapshot.view_id.clone(),
            5,
        )
        .await
        .expect("view should extend");
    assert_eq!(
        extended.view_id, snapshot.view_id,
        "extend keeps the same view id"
    );
    assert_eq!(extended.revision.get(), snapshot.revision.get() + 1);
    let grown = mail_list_state(&extended);
    assert_eq!(grown.rows.len(), 2, "window grew to include the second row");
    assert!(
        !grown.continuation.has_after,
        "no rows past the full window"
    );

    // The extend is broadcast as a ViewReplace to subscribers too.
    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let frame = subscription
                .live
                .next()
                .await
                .expect("runtime stream should remain open");
            if matches!(frame, RuntimeFrame::ViewReplace { .. }) {
                break frame;
            }
        }
    })
    .await
    .expect("runtime replace frame should arrive");
    let RuntimeFrame::ViewReplace {
        view_id, snapshot, ..
    } = frame
    else {
        panic!("expected a view replace frame");
    };
    assert_eq!(view_id, snapshot.view_id);
    assert_eq!(mail_list_state(&snapshot).rows.len(), 2);

    // Extending a non-windowed view family is rejected.
    let detail = build
        .handle
        .open_link_view(
            RuntimeCaller::test(),
            link.link_id.clone(),
            ViewDescriptor {
                family: "messageDetail".to_string(),
                payload: serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "message-1",
                }),
                ..Default::default()
            },
        )
        .await
        .expect("detail view should open");
    let rejected = build
        .handle
        .extend_link_view(
            RuntimeCaller::test(),
            link.link_id.clone(),
            detail.view_id,
            5,
        )
        .await;
    assert!(rejected.is_err(), "non-windowed views reject extension");
}

#[tokio::test]
async fn runtime_account_status_view_serves_and_recomputes() {
    // The accountStatus view serves the folded account list and recomputes +
    // broadcasts a ViewReplace when the account set changes.
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let first = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            mock_account_mutation("acct-status-1"),
        )
        .await
        .expect("first account should create");

    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");
    let snapshot = build
        .handle
        .open_link_view(
            RuntimeCaller::test(),
            link.link_id.clone(),
            ViewDescriptor {
                family: "accountStatus".to_string(),
                payload: serde_json::Value::Null,
                ..Default::default()
            },
        )
        .await
        .expect("account status view should open");
    let items = snapshot.data.as_array().expect("account list payload");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], first.id.as_str());
    assert!(
        items[0].get("runtime").is_some(),
        "overview folds runtime status"
    );

    let mut subscription = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            link.link_id.clone(),
            Some(RuntimeLinkSeq::new(0)),
        )
        .await
        .expect("runtime stream should subscribe");

    // Adding a second account recomputes the all-accounts view.
    build
        .handle
        .create_account(
            RuntimeCaller::test(),
            mock_account_mutation("acct-status-2"),
        )
        .await
        .expect("second account should create");

    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let frame = subscription
                .live
                .next()
                .await
                .expect("runtime stream should remain open");
            if matches!(frame, RuntimeFrame::ViewReplace { .. }) {
                break frame;
            }
        }
    })
    .await
    .expect("account status replace frame should arrive");
    let RuntimeFrame::ViewReplace {
        view_id, snapshot, ..
    } = frame
    else {
        panic!("expected a view replace frame");
    };
    assert_eq!(view_id, snapshot.view_id);
    assert_eq!(
        snapshot.data.as_array().expect("account list").len(),
        2,
        "the view now serves both accounts"
    );
}

#[tokio::test]
async fn mail_list_view_enforces_caller_account_scope() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let account = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            mock_account_mutation("view-account-scope"),
        )
        .await
        .expect("account should create");
    seed_message_batch(&build, &account.id);

    // Link-scoped views bind the caller's account scope to the link at
    // `open_link` time (`link_scope`); a mismatched caller on a later
    // `open_link_view`/`subscribe_runtime_frames` call is rejected as
    // `Unauthorized` (`ensure_caller_matches_link`), not `InvalidDescriptor`.
    let link = build
        .handle
        .open_link(scoped_test_caller("view-account-scope"))
        .await
        .expect("link should open");

    let denied = build
        .handle
        .open_link_view(
            scoped_test_caller("other-account"),
            link.link_id.clone(),
            mail_list_descriptor("in:view-account-scope/inbox"),
        )
        .await
        .expect_err("out-of-scope view should be rejected");
    assert_eq!(denied.envelope().code, RuntimeErrorCode::Unauthorized);

    build
        .handle
        .open_link_view(
            scoped_test_caller("view-account-scope"),
            link.link_id.clone(),
            mail_list_descriptor("in:view-account-scope/inbox"),
        )
        .await
        .expect("matching account scope should open");
    let subscription = build
        .handle
        .subscribe_runtime_frames(
            scoped_test_caller("other-account"),
            link.link_id.clone(),
            Some(RuntimeLinkSeq::new(0)),
        )
        .await;
    let Err(error) = subscription else {
        panic!("out-of-scope subscription should be rejected");
    };
    assert_eq!(error.envelope().code, RuntimeErrorCode::Unauthorized);
}

#[tokio::test]
async fn mail_list_view_fans_out_keyword_replaces_to_all_subscribers() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let account = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            mock_account_mutation("view-account-fanout"),
        )
        .await
        .expect("account should create");
    seed_message_batch(&build, &account.id);

    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");
    build
        .handle
        .open_link_view(
            RuntimeCaller::test(),
            link.link_id.clone(),
            mail_list_descriptor("in:view-account-fanout/inbox"),
        )
        .await
        .expect("mail list view should open");
    // Two independent frame-stream subscribers on the SAME link (fan-out is
    // per-subscriber, not per-link): the runtime stream is multiplexed over
    // the link, not the view, so both attach to the link's frame stream.
    let mut first = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            link.link_id.clone(),
            Some(RuntimeLinkSeq::new(1)),
        )
        .await
        .expect("first subscriber should open");
    let mut second = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            link.link_id.clone(),
            Some(RuntimeLinkSeq::new(1)),
        )
        .await
        .expect("second subscriber should open");

    let result = build
        .api_bridge
        .store
        .flag_message_in_base(&account.id, "message-1")
        .expect("keyword command should write");
    for event in result.events {
        build
            .api_bridge
            .event_sender
            .send(event)
            .expect("event should broadcast");
    }

    for subscription in [&mut first, &mut second] {
        let frame = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let frame = subscription
                    .live
                    .next()
                    .await
                    .expect("runtime stream should remain open");
                if matches!(frame, RuntimeFrame::ViewReplace { .. }) {
                    break frame;
                }
            }
        })
        .await
        .expect("view frame should arrive");
        let RuntimeFrame::ViewReplace { snapshot, .. } = frame else {
            panic!("expected replace frame");
        };
        assert_eq!(snapshot.revision.get(), 2);
    }
}

#[tokio::test]
async fn mail_list_view_keeps_open_view_fresh_without_active_subscribers() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let account = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            mock_account_mutation("view-account-reconnect"),
        )
        .await
        .expect("account should create");
    seed_message_batch(&build, &account.id);

    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");
    build
        .handle
        .open_link_view(
            RuntimeCaller::test(),
            link.link_id.clone(),
            mail_list_descriptor("in:view-account-reconnect/inbox"),
        )
        .await
        .expect("mail list view should open");

    let result = build
        .api_bridge
        .store
        .flag_message_in_base(&account.id, "message-1")
        .expect("keyword command should write");
    for event in result.events {
        build
            .api_bridge
            .event_sender
            .send(event)
            .expect("event should broadcast");
    }

    let catch_up = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let subscription = build
                .handle
                .subscribe_runtime_frames(
                    RuntimeCaller::test(),
                    link.link_id.clone(),
                    Some(RuntimeLinkSeq::new(0)),
                )
                .await
                .expect("subscription should open");
            if let Some(RuntimeFrame::ViewSnapshot { snapshot, .. }) =
                subscription.catch_up.into_iter().next()
            {
                if snapshot.revision.get() == 2 {
                    break snapshot;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("open view should refresh while disconnected");
    let state = mail_list_state(&catch_up);
    let row = state
        .rows
        .iter()
        .find(|row| row.projection["id"] == "message-1")
        .expect("updated row should remain in window");
    assert_eq!(row.projection["isFlagged"], true);
}

#[tokio::test]
async fn mail_list_view_replaces_snapshot_when_keyword_event_changes_membership() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let account = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            mock_account_mutation("view-account-flagged"),
        )
        .await
        .expect("account should create");
    seed_message_batch(&build, &account.id);

    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");
    let snapshot = build
        .handle
        .open_link_view(
            RuntimeCaller::test(),
            link.link_id.clone(),
            mail_list_descriptor("in:view-account-flagged/inbox is:flagged"),
        )
        .await
        .expect("flagged view should open");
    assert!(mail_list_state(&snapshot).rows.is_empty());
    let mut subscription = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            link.link_id.clone(),
            Some(RuntimeLinkSeq::new(1)),
        )
        .await
        .expect("runtime stream should subscribe");

    let result = build
        .api_bridge
        .store
        .flag_message_in_base(&account.id, "message-1")
        .expect("keyword command should write");
    for event in result.events {
        build
            .api_bridge
            .event_sender
            .send(event)
            .expect("event should broadcast");
    }

    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let frame = subscription
                .live
                .next()
                .await
                .expect("runtime stream should remain open");
            if matches!(frame, RuntimeFrame::ViewReplace { .. }) {
                break frame;
            }
        }
    })
    .await
    .expect("view frame should arrive");
    let RuntimeFrame::ViewReplace { snapshot, .. } = frame else {
        panic!("expected replace frame");
    };
    let state = mail_list_state(&snapshot);
    assert_eq!(state.rows.len(), 1);
    assert_eq!(state.rows[0].projection["id"], "message-1");
}

#[tokio::test]
async fn mail_list_view_ignores_keyword_events_for_messages_outside_window() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let account = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            mock_account_mutation("view-account-2"),
        )
        .await
        .expect("account should create");
    seed_message_batch(&build, &account.id);

    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");
    let snapshot = build
        .handle
        .open_link_view(
            RuntimeCaller::test(),
            link.link_id.clone(),
            mail_list_descriptor("in:view-account-2/archive"),
        )
        .await
        .expect("unaffected view should open");
    assert!(mail_list_state(&snapshot).rows.is_empty());
    let mut subscription = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            link.link_id.clone(),
            Some(RuntimeLinkSeq::new(1)),
        )
        .await
        .expect("runtime stream should subscribe");

    let result = build
        .api_bridge
        .store
        .flag_message_in_base(&account.id, "message-1")
        .expect("keyword command should write");
    for event in result.events {
        build
            .api_bridge
            .event_sender
            .send(event)
            .expect("event should broadcast");
    }

    // The link's stream also carries the flat `Notification` firehose frame for
    // every domain event on the account, unrelated to view membership — only a
    // `ViewReplace`/`ViewDelta` on the unaffected view itself would indicate a
    // (wrong) recompute, so filter those out rather than asserting silence.
    let no_view_frame = tokio::time::timeout(std::time::Duration::from_millis(50), async {
        loop {
            let frame = subscription.live.next().await?;
            if matches!(
                frame,
                RuntimeFrame::ViewReplace { .. } | RuntimeFrame::ViewDelta { .. }
            ) {
                return Some(frame);
            }
        }
    })
    .await;
    assert!(
        no_view_frame.is_err(),
        "unaffected view should not receive a replace/delta frame"
    );
}

#[tokio::test]
async fn create_account_duplicate_id_conflicts_without_overwriting_config_or_secret() {
    let root = temp_root();
    let secret_store = Arc::new(TestSecretStore::default());
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(secret_store.clone());

    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let created = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            imap_smtp_account_mutation("duplicate", "Original", "old-password"),
        )
        .await
        .expect("initial account should be created");
    let secret_ref = build
        .api_bridge
        .service
        .get_source(&created.id)
        .expect("source lookup should succeed")
        .expect("created account should exist")
        .transport
        .secret_ref
        .expect("created account should have a secret");

    let error = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            imap_smtp_account_mutation("duplicate", "Replacement", "new-password"),
        )
        .await
        .expect_err("duplicate account id should conflict");

    assert_eq!(error.envelope().code, RuntimeErrorCode::Conflict);
    let account = build
        .api_bridge
        .service
        .get_source(&created.id)
        .expect("source lookup should succeed")
        .expect("original account should remain configured");
    assert_eq!(account.name, "Original");
    assert_eq!(
        secret_store.value(&secret_ref).as_deref(),
        Some("old-password")
    );
}

// spec: docs/authority-server/L3#account-assets-runtime-backed
// spec: docs/runtime/internals/L3#account-resource-linkage-runtime-owned
#[tokio::test]
async fn delete_account_removes_secret_config_and_publishes_event_through_runtime() {
    let root = temp_root();
    let secret_store = Arc::new(TestSecretStore::default());
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(secret_store.clone());

    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let created = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            CreateAccountMutation {
                id: Some("delete-me".to_string()),
                name: "Delete Me".to_string(),
                full_name: None,
                signature: None,
                email_patterns: vec!["delete-me@example.com".to_string()],
                driver: Some(AccountDriver::ImapSmtp),
                enabled: Some(false),
                appearance: None,
                transport: AccountTransportMutation {
                    provider: Some(ProviderHint::Generic),
                    auth: Some(ProviderAuthKind::Password),
                    base_url: None,
                    username: Some("delete-me@example.com".to_string()),
                    imap: Some(ImapTransportSettings {
                        host: "imap.example.com".to_string(),
                        port: 993,
                        security: TransportSecurity::Tls,
                    }),
                    smtp: Some(SmtpTransportSettings {
                        host: "smtp.example.com".to_string(),
                        port: 465,
                        security: TransportSecurity::Tls,
                    }),
                },
                secret: SecretWriteMutation {
                    mode: SecretWriteMode::Replace,
                    password: Some("secret".to_string()),
                },
            },
        )
        .await
        .expect("account should be created");
    let secret_ref = build
        .api_bridge
        .service
        .get_source(&created.id)
        .expect("source lookup should succeed")
        .expect("created account should exist")
        .transport
        .secret_ref
        .expect("created account should have a secret");
    assert!(secret_store.value(&secret_ref).is_some());

    build
        .handle
        .delete_account(RuntimeCaller::test(), created.id.clone())
        .await
        .expect("runtime should delete account");

    assert!(secret_store.value(&secret_ref).is_none());
    assert!(build
        .api_bridge
        .service
        .get_source(&created.id)
        .expect("source lookup should succeed")
        .is_none());
    let subscription = build
        .handle
        .subscribe_events(
            RuntimeCaller::test(),
            EventFilter {
                account_id: Some(created.id.clone()),
                topic: Some(EVENT_TOPIC_ACCOUNT_DELETED.to_string()),
                mailbox_id: None,
                after_seq: Some(0),
            },
        )
        .await
        .expect("runtime should replay delete events");
    assert_eq!(subscription.replay.len(), 1);
}

// spec: docs/eph/RFC-L2-provider-reliability#d100
#[tokio::test]
async fn deleting_an_account_gcs_its_message_and_mailbox_rows() {
    // D100(b) / D2: account deletion must GC the account's synced store rows,
    // not just its config + secret — otherwise messages/mailboxes are orphaned
    // in SQLite forever.
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");

    let mut mutation = mock_account_mutation("gc-account");
    mutation.enabled = Some(true);
    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mutation)
        .await
        .expect("account should create");

    // A sync seeds the mock provider's sample mailboxes/messages into the store.
    build
        .account_supervisor
        .sync_account(&account.id)
        .await
        .expect("sync should seed store rows");
    assert!(
        !build
            .api_bridge
            .service
            .list_messages(&account.id, None)
            .expect("messages should list")
            .is_empty(),
        "the account should have message rows before deletion"
    );
    assert!(
        !build
            .api_bridge
            .service
            .list_mailboxes(&account.id)
            .expect("mailboxes should list")
            .is_empty(),
        "the account should have mailbox rows before deletion"
    );

    build
        .handle
        .delete_account(RuntimeCaller::test(), account.id.clone())
        .await
        .expect("runtime should delete account");

    assert!(
        build
            .api_bridge
            .service
            .list_messages(&account.id, None)
            .expect("messages should list")
            .is_empty(),
        "message rows must be GC'd when the account is deleted"
    );
    assert!(
        build
            .api_bridge
            .service
            .list_mailboxes(&account.id)
            .expect("mailboxes should list")
            .is_empty(),
        "mailbox rows must be GC'd when the account is deleted"
    );
}

// spec: docs/eph/RFC-L2-provider-reliability#d100
#[tokio::test]
async fn deleting_an_account_during_an_in_flight_sync_commits_no_rows() {
    // D100(a) / D1: the runtime must be stopped-and-drained BEFORE the config +
    // secret are removed and the store rows are GC'd, so an in-flight sync
    // cannot commit NEW rows for the just-deleted account (undoing the purge).
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");

    let mut mutation = mock_account_mutation("delete-midsync-account");
    mutation.enabled = Some(true);
    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mutation)
        .await
        .expect("account should create");

    // Baseline sync establishes the connection and seeds rows.
    build
        .account_supervisor
        .sync_account(&account.id)
        .await
        .expect("baseline sync should run");
    assert!(
        !build
            .api_bridge
            .service
            .list_messages(&account.id, None)
            .expect("messages should list")
            .is_empty(),
        "the baseline sync should have seeded rows"
    );

    // Gate the next provider pull at entry, then fire a sync that blocks
    // in-flight there.
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let _gate = MockJmapGateway::gate_sync_at_entry(&account.id, entered.clone(), release.clone());
    build
        .account_supervisor
        .trigger_account_sync(&account.id, SyncTrigger::Manual)
        .await
        .expect("trigger should enqueue a cycle");
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .expect("the sync should reach the gated provider pull and block in-flight");

    // Delete while that sync is in flight. The fixed ordering stops-and-drains
    // the runtime first (cancelling the gated sync), then purges — so the
    // in-flight sync is cancelled before it can commit anything.
    build
        .handle
        .delete_account(RuntimeCaller::test(), account.id.clone())
        .await
        .expect("delete during an in-flight sync should succeed");

    // Release the (now-cancelled) gate. Under the wrong ordering the sync could
    // still be live here and re-commit rows for the deleted account.
    release.notify_one();
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(
        build
            .api_bridge
            .service
            .list_messages(&account.id, None)
            .expect("messages should list")
            .is_empty(),
        "a delete racing an in-flight sync must leave no message rows for the account"
    );
    assert!(
        build
            .api_bridge
            .service
            .list_mailboxes(&account.id)
            .expect("mailboxes should list")
            .is_empty(),
        "a delete racing an in-flight sync must leave no mailbox rows for the account"
    );
}

// spec: docs/authority-server/L3#account-mutations-runtime-backed
// spec: docs/runtime/internals/L3#account-mutation-contract-pattern
#[tokio::test]
async fn oauth_token_persistence_writes_secret_and_patches_account_through_runtime() {
    let root = temp_root();
    let secret_store = Arc::new(TestSecretStore::default());
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(secret_store.clone());

    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let created = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            CreateAccountMutation {
                id: Some("oauth-existing".to_string()),
                name: "OAuth Existing".to_string(),
                full_name: None,
                signature: None,
                email_patterns: vec!["user@example.com".to_string()],
                driver: Some(AccountDriver::ImapSmtp),
                enabled: Some(false),
                appearance: None,
                transport: AccountTransportMutation {
                    provider: Some(ProviderHint::Gmail),
                    auth: Some(ProviderAuthKind::Password),
                    base_url: None,
                    username: Some("user@example.com".to_string()),
                    imap: Some(ImapTransportSettings {
                        host: "imap.example.com".to_string(),
                        port: 993,
                        security: TransportSecurity::Tls,
                    }),
                    smtp: Some(SmtpTransportSettings {
                        host: "smtp.example.com".to_string(),
                        port: 465,
                        security: TransportSecurity::Tls,
                    }),
                },
                secret: SecretWriteMutation {
                    mode: SecretWriteMode::Replace,
                    password: Some("old-password".to_string()),
                },
            },
        )
        .await
        .expect("existing account should be created");

    let secret_ref = build
        .api_bridge
        .service
        .get_source(&created.id)
        .expect("source lookup should succeed")
        .expect("created account should exist")
        .transport
        .secret_ref
        .expect("created account should have a secret");
    build
        .account_mutations
        .persist_oauth_token_set(
            created.id.clone(),
            OAuthTokenSet {
                r#type: "oauth2".to_string(),
                provider: ProviderHint::Gmail,
                client_id: "client-id".to_string(),
                client_secret: Some("client-secret".to_string()),
                access_token: "access-token".to_string(),
                refresh_token: Some("refresh-token".to_string()),
                expires_at: Some("2026-04-27T10:00:00Z".to_string()),
                scopes: vec!["email".to_string()],
            },
        )
        .await
        .expect("OAuth token set should be persisted by runtime");

    let stored = secret_store
        .value(&secret_ref)
        .expect("OAuth token set should be written to existing managed secret");
    let decoded =
        OAuthTokenSet::decode(&stored).expect("stored secret should be an OAuth token set");
    assert_eq!(decoded.access_token, "access-token");

    let account = build
        .api_bridge
        .service
        .get_source(&created.id)
        .expect("source lookup should succeed")
        .expect("account should exist after OAuth patch");
    assert_eq!(account.transport.auth, ProviderAuthKind::OAuth2);
}

// spec: docs/runtime/internals/L3#event-subscription-runtime-backed
#[tokio::test]
async fn event_subscription_replays_backlog_then_filters_live_events() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));

    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let filter = EventFilter {
        account_id: Some(AccountId::from("primary")),
        topic: Some(EVENT_TOPIC_MESSAGE_UPDATED.to_string()),
        mailbox_id: Some(MailboxId::from("inbox")),
        after_seq: Some(0),
    };
    let replayed = build
        .api_bridge
        .store
        .append_event(
            &AccountId::from("primary"),
            EVENT_TOPIC_MESSAGE_UPDATED,
            Some(&MailboxId::from("inbox")),
            None,
            serde_json::json!({"kind": "replayed"}),
        )
        .expect("backlog event should append");
    build
        .api_bridge
        .store
        .append_event(
            &AccountId::from("secondary"),
            EVENT_TOPIC_MESSAGE_UPDATED,
            Some(&MailboxId::from("inbox")),
            None,
            serde_json::json!({"kind": "ignored"}),
        )
        .expect("non-matching backlog event should append");

    let mut subscription = build
        .handle
        .subscribe_events(RuntimeCaller::test(), filter)
        .await
        .expect("runtime should subscribe to filtered events");

    assert_eq!(subscription.replay.len(), 1);
    assert_eq!(subscription.replay[0].seq, replayed.seq);

    let ignored_live = build
        .api_bridge
        .store
        .append_event(
            &AccountId::from("primary"),
            EVENT_TOPIC_MESSAGE_UPDATED,
            Some(&MailboxId::from("archive")),
            None,
            serde_json::json!({"kind": "ignored-live"}),
        )
        .expect("non-matching live event should append");
    build
        .api_bridge
        .event_sender
        .send(ignored_live)
        .expect("ignored live event should broadcast");
    let matching_live = build
        .api_bridge
        .store
        .append_event(
            &AccountId::from("primary"),
            EVENT_TOPIC_MESSAGE_UPDATED,
            Some(&MailboxId::from("inbox")),
            None,
            serde_json::json!({"kind": "live"}),
        )
        .expect("matching live event should append");
    build
        .api_bridge
        .event_sender
        .send(matching_live.clone())
        .expect("matching live event should broadcast");

    let received = subscription
        .live
        .next()
        .await
        .expect("matching live event should pass runtime filter");
    assert_eq!(received.seq, matching_live.seq);
}

#[tokio::test]
async fn runtime_link_stream_carries_scoped_domain_event_notifications() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));

    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let caller = RuntimeCaller {
        account_scope: Some(vec!["primary".to_string()]),
        ..RuntimeCaller::test()
    };
    let link = build
        .handle
        .open_link(caller.clone())
        .await
        .expect("runtime link should open");
    let mut subscription = build
        .handle
        .subscribe_runtime_frames(caller, link.link_id, Some(RuntimeLinkSeq::new(0)))
        .await
        .expect("runtime frames should subscribe");

    let ignored = build
        .api_bridge
        .store
        .append_event(
            &AccountId::from("secondary"),
            EVENT_TOPIC_MESSAGE_UPDATED,
            Some(&MailboxId::from("inbox")),
            None,
            serde_json::json!({"kind": "ignored"}),
        )
        .expect("ignored event should append");
    build
        .api_bridge
        .event_sender
        .send(ignored)
        .expect("ignored event should broadcast");
    let matching = build
        .api_bridge
        .store
        .append_event(
            &AccountId::from("primary"),
            EVENT_TOPIC_MESSAGE_UPDATED,
            Some(&MailboxId::from("inbox")),
            None,
            serde_json::json!({"kind": "live"}),
        )
        .expect("matching event should append");
    build
        .api_bridge
        .event_sender
        .send(matching.clone())
        .expect("matching event should broadcast");

    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), subscription.live.next())
        .await
        .expect("notification frame should arrive")
        .expect("runtime stream should remain open");
    let RuntimeFrame::Notification {
        link_seq,
        kind,
        payload,
    } = frame
    else {
        panic!("expected notification frame");
    };
    assert_eq!(link_seq.get(), 1);
    assert_eq!(kind, EVENT_TOPIC_MESSAGE_UPDATED);
    assert_eq!(payload["seq"], matching.seq);
    assert_eq!(payload["accountId"], "primary");
    assert_eq!(payload["payload"]["kind"], "live");
}

#[tokio::test]
async fn zero_event_channel_capacity_returns_typed_build_error() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()))
            .with_event_channel_capacity(0);

    let error = match build_authority_server(config).await {
        Ok(_) => panic!("zero-capacity event channel should be rejected before build side effects"),
        Err(error) => error,
    };

    assert!(matches!(error, RuntimeBuildError::InvalidConfig(_)));
}

/// An authority server link transport whose up-channel blocks until released — a test seam
/// for observing the runtime's pending-set overlay while a mutation is in flight.
/// Wraps the real transport pair (`AuthorityServerLinkHandle`, D33) and
/// implements both trait halves, intercepting only the Link half's up-channel.
struct DeferredTransport {
    inner: posthaste_authority_server_link::AuthorityServerLinkHandle,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait]
impl posthaste_authority_server_link::AuthorityServerLink for DeferredTransport {
    async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<posthaste_contract_core::MutationReceipt, posthaste_contract_core::RuntimeError>
    {
        // Signal that the forward has begun (so the pending set already holds the
        // mutation), then wait for the test to release it.
        self.entered.notify_one();
        self.release.notified().await;
        Ok(posthaste_contract_core::MutationReceipt {
            runtime_mutation_id: Some(posthaste_contract_core::RuntimeMutationId::new(
                "authority-server-deferred",
            )),
            client_mutation_id: mutation.client_mutation_id,
            name: mutation.operation.name().to_string(),
            state: MutationSettlementState::Confirmed,
            error: None,
            output: serde_json::json!({ "events": [] }),
        })
    }

    async fn subscribe(
        &self,
        coverage: posthaste_authority_server_link::LinkCoverage,
        after_seq: Option<u64>,
    ) -> Result<posthaste_authority_server_link::DownStream, posthaste_contract_core::RuntimeError>
    {
        self.inner.subscribe(coverage, after_seq).await
    }
}

#[async_trait::async_trait]
impl posthaste_authority_server_link::AuthorityServerApi for DeferredTransport {
    // Everything other than the gated up-channel delegates to the real authority server,
    // so setup (account creation) hits the live store the local reads observe.
    // (forward_mutation deliberately does *not* delegate: it confirms without
    // applying, so the test can prove the optimistic overlay reverts on retire.)
    async fn create_account(
        &self,
        mutation: CreateAccountMutation,
    ) -> Result<posthaste_domain_model::AccountOverview, posthaste_contract_core::RuntimeError>
    {
        self.inner.create_account(mutation).await
    }
}

fn flagged(state: &MailListViewState, message_id: &str) -> bool {
    state
        .rows
        .iter()
        .find(|row| row.projection["id"] == message_id)
        .expect("row should exist")
        .projection["isFlagged"]
        .as_bool()
        .expect("isFlagged should be a bool")
}

// spec: docs/replication/authority-server-link/L2#5-the-runtime-near-node-read-replica-pending-set
#[tokio::test]
async fn runtime_serves_optimistic_rows_from_its_pending_set_while_a_forward_is_in_flight() {
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let entered_for_transport = entered.clone();
    let release_for_transport = release.clone();

    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()))
            // Decorate the real in-process transport: gate the up-channel, delegate the
            // rest (so account-creation setup reaches the live authority server).
            .with_authority_server_transport_override(move |inner| {
                posthaste_authority_server_link::AuthorityServerLinkHandle::new(Arc::new(
                    DeferredTransport {
                        inner,
                        entered: entered_for_transport,
                        release: release_for_transport,
                    },
                ))
            });
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let account = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            mock_account_mutation("optimism-account"),
        )
        .await
        .expect("account should create");
    seed_message_batch(&build, &account.id);
    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");

    let descriptor = mail_list_descriptor("in:optimism-account/inbox");

    // Baseline: message-1 is not flagged.
    let baseline = build
        .handle
        .open_link_view(
            RuntimeCaller::test(),
            link.link_id.clone(),
            descriptor.clone(),
        )
        .await
        .expect("baseline view should open");
    assert!(!flagged(&mail_list_state(&baseline), "message-1"));

    // Forward a flag mutation whose up-channel blocks, leaving it in the pending set.
    let handle = build.handle.clone();
    let link_id = link.link_id.clone();
    let account_id = account.id.as_str().to_string();
    let task = tokio::spawn(async move {
        handle
            .forward_mutation(
                RuntimeCaller::test(),
                MutationRequest {
                    link_id: Some(link_id),
                    operation: serde_json::from_value(serde_json::json!({
                        "name": "message.setFlaggedState",
                        "args": serde_json::json!({
                        "sourceId": account_id,
                        "messageId": "message-1",
                        "flagged": true,
                    }),
                    }))
                    .expect("typed operation parses"),
                    client_mutation_id: ClientMutationId::new("client-flag"),
                    context: None,
                },
            )
            .await
            .expect("mutation should run")
    });

    // Wait for the forward to begin (the pending set now holds the flag).
    entered.notified().await;

    // While the forward is in flight, the runtime serves the row optimistically
    // flagged — folded from its pending set via the shared MailListReplica.
    let optimistic = build
        .handle
        .open_link_view(
            RuntimeCaller::test(),
            link.link_id.clone(),
            descriptor.clone(),
        )
        .await
        .expect("optimistic view should open");
    assert!(
        flagged(&mail_list_state(&optimistic), "message-1"),
        "the in-flight mutation should show optimistically"
    );

    // Release the forward; the mutation completes and the pending set retires.
    release.notify_one();
    task.await.expect("mutation task should join");

    // The deferred authority server never applied the change, so once the pending set retires
    // the served row reflects the (unchanged) authoritative store again.
    let settled = build
        .handle
        .open_link_view(RuntimeCaller::test(), link.link_id.clone(), descriptor)
        .await
        .expect("settled view should open");
    assert!(
        !flagged(&mail_list_state(&settled), "message-1"),
        "the overlay should retire once the forward completes"
    );
}

fn move_to_role_request(
    link_id: &posthaste_contract_core::RuntimeLinkId,
    client_mutation_id: &str,
) -> MutationRequest {
    MutationRequest {
        link_id: Some(link_id.clone()),
        operation: serde_json::from_value(serde_json::json!({
            "name": "message.moveToRole",
            "args": serde_json::json!({
                "sourceId": "field-bug-account",
                "messageId": "m-1",
                "role": "archive",
            }),
        }))
        .expect("typed operation parses"),
        client_mutation_id: ClientMutationId::new(client_mutation_id),
        context: None,
    }
}

/// THE v0.5.0 field-bug regression pin ("message.moveToRole did not return a
/// message command result"): a duplicate forward under the SAME
/// `clientMutationId` arriving while the FIRST dispatch is still in flight —
/// the near-end engine re-forwards after its request deadline while a slow
/// first apply is still running — must not resolve with a result-less receipt
/// (state `accepted`, `output: null`, the pre-fix answer the web client failed
/// to parse). The duplicate WAITS for the original dispatch to settle and
/// re-observes its terminal receipt, whose serialized wire shape carries
/// `output.events` as an array.
#[tokio::test]
async fn in_flight_duplicate_forward_re_observes_the_settled_receipt_with_events() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let entered_for_transport = entered.clone();
    let release_for_transport = release.clone();

    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()))
            .with_authority_server_transport_override(move |inner| {
                posthaste_authority_server_link::AuthorityServerLinkHandle::new(Arc::new(
                    DeferredTransport {
                        inner,
                        entered: entered_for_transport,
                        release: release_for_transport,
                    },
                ))
            });
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");

    // Dispatch #1: enters the (gated) up-channel and stays in flight.
    let first_handle = build.handle.clone();
    let first_request = move_to_role_request(&link.link_id, "dup-move-1");
    let first = tokio::spawn(async move {
        first_handle
            .forward_mutation(RuntimeCaller::test(), first_request)
            .await
    });
    entered.notified().await;

    // Dispatch #2: the SAME clientMutationId while #1 is still applying.
    let second_handle = build.handle.clone();
    let second_request = move_to_role_request(&link.link_id, "dup-move-1");
    let mut second = tokio::spawn(async move {
        second_handle
            .forward_mutation(RuntimeCaller::test(), second_request)
            .await
    });

    // The duplicate must WAIT for the original to settle. Pre-fix it resolved
    // immediately with the pending record's receipt: state `accepted`,
    // `output: null` — no command result for the client to parse.
    if let Ok(early) = tokio::time::timeout(Duration::from_millis(300), &mut second).await {
        let receipt = early
            .expect("duplicate task should not panic")
            .expect("duplicate forward should not error while the original is in flight");
        panic!(
            "the duplicate resolved before the original settled — a result-less \
             receipt (state {:?}, output {}) is the v0.5.0 field bug",
            receipt.state, receipt.output
        );
    }

    // Release the original; both dispatches settle on ITS outcome.
    release.notify_one();
    let first_receipt = first
        .await
        .expect("first task should join")
        .expect("first forward should settle");
    assert_eq!(first_receipt.state, MutationSettlementState::Confirmed);

    let duplicate_receipt = second
        .await
        .expect("duplicate task should join")
        .expect("the duplicate re-observes the settled outcome");
    assert_eq!(duplicate_receipt.state, MutationSettlementState::Confirmed);
    let wire = serde_json::to_value(&duplicate_receipt).expect("receipt serializes");
    assert!(
        wire["output"]["events"].is_array(),
        "a re-observed confirmed mail-command receipt must carry `output.events` \
         as an array on the wire (empty ok, never absent); got {wire}"
    );
}

/// An authority-server-link transport answering every forward with a
/// result-less receipt: state `accepted`, `output: null` — the shape a pre-fix
/// authority returns when a duplicate re-observes a still-pending (or
/// cancellation-orphaned) dedup record.
struct PendingEchoTransport {
    inner: posthaste_authority_server_link::AuthorityServerLinkHandle,
}

#[async_trait::async_trait]
impl posthaste_authority_server_link::AuthorityServerLink for PendingEchoTransport {
    async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<posthaste_contract_core::MutationReceipt, posthaste_contract_core::RuntimeError>
    {
        Ok(posthaste_contract_core::MutationReceipt {
            runtime_mutation_id: Some(posthaste_contract_core::RuntimeMutationId::new(
                "authority-pending-echo",
            )),
            client_mutation_id: mutation.client_mutation_id,
            name: mutation.operation.name().to_string(),
            state: MutationSettlementState::Accepted,
            error: None,
            output: serde_json::Value::Null,
        })
    }

    async fn subscribe(
        &self,
        coverage: posthaste_authority_server_link::LinkCoverage,
        after_seq: Option<u64>,
    ) -> Result<posthaste_authority_server_link::DownStream, posthaste_contract_core::RuntimeError>
    {
        self.inner.subscribe(coverage, after_seq).await
    }
}

#[async_trait::async_trait]
impl posthaste_authority_server_link::AuthorityServerApi for PendingEchoTransport {}

/// The wire-contract guard behind the same field bug, one hop down: an
/// authority answering a forward with a result-less receipt (`output: null`,
/// non-terminal) must NOT be promoted to a client-facing `Confirmed` receipt —
/// pre-fix the runtime settled `Confirmed` around the null output, and the
/// client threw "did not return a message command result" on a receipt that
/// claimed success. It settles `Failed` with a retryable conflict instead, so
/// the caller reverts its optimism and retries honestly.
#[tokio::test]
async fn a_result_less_authority_receipt_settles_failed_not_confirmed() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()))
            .with_authority_server_transport_override(move |inner| {
                posthaste_authority_server_link::AuthorityServerLinkHandle::new(Arc::new(
                    PendingEchoTransport { inner },
                ))
            });
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");

    let receipt = build
        .handle
        .forward_mutation(
            RuntimeCaller::test(),
            move_to_role_request(&link.link_id, "pending-echo-1"),
        )
        .await
        .expect("the forward settles with a terminal receipt");
    assert_eq!(
        receipt.state,
        MutationSettlementState::Failed,
        "a result-less authority receipt must never surface as Confirmed \
         (output {})",
        receipt.output
    );
    let error = receipt
        .error
        .expect("the failed settlement carries the retryable conflict");
    assert_eq!(error.code, RuntimeErrorCode::Conflict);
    assert!(
        error.terminality != posthaste_contract_core::Terminality::Permanent,
        "the guard's verdict is retryable — the caller may retry the mutation"
    );
}

// spec: docs/L1-sync#sync-loop
#[tokio::test]
async fn rapid_mutation_burst_coalesces_provider_sync_triggers() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");

    let mut mutation = mock_account_mutation("rapid-burst-account");
    mutation.enabled = Some(true);
    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mutation)
        .await
        .expect("account should create");

    // Establish a baseline after an explicit sync; this also ensures the
    // runtime task is running before the burst arrives.
    build
        .account_supervisor
        .sync_account(&account.id)
        .await
        .expect("mock account runtime should sync");
    let baseline = build.account_supervisor.sync_cycle_count(&account.id).await;
    assert!(
        baseline >= 1,
        "startup or explicit sync should execute at least one cycle"
    );

    seed_single_message_batch(&build, &account.id, "em-001", "mb-inbox");

    // Gate every subsequent provider pull at entry so the in-flight cycle is
    // held open deterministically — the burst below cannot race past it. This is
    // what turns the old probabilistic `<= 2` bound into an exact invariant: the
    // first trigger to win the atomic idle→active claim holds `active` for the
    // whole gated cycle, so every one of the other fourteen triggers provably
    // coalesces (rather than some slipping through the idle window as their own
    // cycle, the P5 flake).
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let _gate = MockJmapGateway::gate_sync_at_entry(&account.id, entered.clone(), release.clone());

    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");

    // Fire 15 rapid flag/unflag toggles concurrently. Under the old behavior
    // each toggle would enqueue a full provider sync; with the atomic claim they
    // collapse into exactly one in-flight cycle plus one pending follow-up.
    let handle = build.handle.clone();
    let link_id = link.link_id.clone();
    let account_id = account.id.clone();
    let mut burst = Vec::with_capacity(15);
    for i in 0..15 {
        let handle = handle.clone();
        let link_id = link_id.clone();
        let account_id = account_id.clone();
        let (add, remove) = if i % 2 == 0 {
            (vec!["$flagged"], Vec::<&str>::new())
        } else {
            (Vec::<&str>::new(), vec!["$flagged"])
        };
        burst.push(tokio::spawn(async move {
            handle
                .forward_mutation(
                    RuntimeCaller::test(),
                    MutationRequest {
                        link_id: Some(link_id),
                        operation: serde_json::from_value(serde_json::json!({
                            "name": "message.setKeywords",
                            "args": serde_json::json!({
                            "sourceId": account_id.as_str(),
                            "messageId": "em-001",
                            "command": {"add": add, "remove": remove}
                        }),
                        }))
                        .expect("typed operation parses"),
                        client_mutation_id: ClientMutationId::new(format!("burst-{i}")),
                        context: None,
                    },
                )
                .await
                .expect("burst mutation should run");
        }));
    }
    for task in burst {
        task.await.expect("burst task should not panic");
    }

    // Every trigger has now been issued. Exactly one won the atomic claim and
    // its cycle (cycle A) is entering the gated provider pull; the other
    // fourteen coalesced into a single pending follow-up. Confirm cycle A
    // reached the gate.
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .expect("exactly one burst trigger should have claimed and entered a cycle");

    // Release cycle A. On finishing it drains the single coalesced pending and
    // runs exactly one follow-up (cycle B), which re-enters the gate.
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .expect("the fourteen coalesced triggers should run exactly one follow-up cycle");
    // Release cycle B; it finds nothing pending and settles.
    release.notify_one();

    // Wait until no new sync cycle starts for a short interval, proving the
    // burst has fully drained.
    let final_count = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let count = build.account_supervisor.sync_cycle_count(&account.id).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            if build.account_supervisor.sync_cycle_count(&account.id).await == count {
                return count;
            }
        }
    })
    .await
    .expect("sync cycles should settle within timeout");

    let additional_cycles = final_count.saturating_sub(baseline);
    // D99: with the atomic idle-claim this is an INVARIANT, not a scheduling
    // accident. The gate holds cycle A's `active` claim open across the whole
    // burst, so all fifteen concurrent triggers resolve to exactly one claimed
    // cycle (A) plus one coalesced follow-up (B) — never the per-mutation storm
    // the old `<= 2` probabilistic bound was papering over.
    assert_eq!(
        additional_cycles, 2,
        "15 rapid mutations must coalesce into exactly one in-flight cycle plus one \
         pending follow-up (2 additional), got {additional_cycles}"
    );
}

#[tokio::test]
async fn runtime_mutation_in_one_session_updates_view_in_another_session() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let mut mutation = mock_account_mutation("cross-link-mutation-account");
    mutation.enabled = Some(true);
    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mutation)
        .await
        .expect("account should create");
    build
        .account_supervisor
        .sync_account(&account.id)
        .await
        .expect("mock account runtime should sync");
    seed_single_message_batch(&build, &account.id, "xm-001", "mb-inbox");

    let link_a = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link A should open");
    let snapshot_a = build
        .handle
        .open_link_view(
            RuntimeCaller::test(),
            link_a.link_id.clone(),
            mail_list_descriptor("in:cross-link-mutation-account/mb-inbox"),
        )
        .await
        .expect("link A view should open");
    let mut subscription_a = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            link_a.link_id.clone(),
            Some(RuntimeLinkSeq::new(0)),
        )
        .await
        .expect("link A stream should subscribe");
    assert_eq!(subscription_a.catch_up.len(), 1);

    let link_b = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link B should open");

    let receipt = build
        .handle
        .forward_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                link_id: Some(link_b.link_id.clone()),
                operation: serde_json::from_value(serde_json::json!({
                    "name": "message.setKeywords",
                    "args": serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "xm-001",
                    "command": {"add": ["$flagged"], "remove": []}
                }),
                }))
                .expect("typed operation parses"),
                client_mutation_id: ClientMutationId::new("client-b"),
                context: None,
            },
        )
        .await
        .expect("link B mutation should run");

    assert_eq!(receipt.name, "message.setKeywords");
    assert_eq!(receipt.state, MutationSettlementState::Confirmed);

    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let frame = subscription_a
                .live
                .next()
                .await
                .expect("link A stream should remain open");
            if matches!(frame, RuntimeFrame::ViewReplace { .. }) {
                break frame;
            }
        }
    })
    .await
    .expect("link A should receive a view update after link B mutation");
    let RuntimeFrame::ViewReplace {
        view_id: fid,
        snapshot,
        ..
    } = frame
    else {
        panic!("expected ViewReplace");
    };
    assert_eq!(fid, snapshot_a.view_id);
    let state = mail_list_state(&snapshot);
    let row = state
        .rows
        .iter()
        .find(|row| row.projection["id"] == "xm-001")
        .expect("updated row should be visible");
    assert_eq!(row.projection["isFlagged"], true);
}

/// A(1) client-liveness (AUDIT-L2-client-liveness §0, updated by
/// RFC-L2-count-unification): the OPTIMISTIC client echo for a mark-read
/// carries the store command's ENRICHED `message.updated` — the row-liveness
/// `projection` — instead of a bare `{changes:{keywords:true}}` event, and it
/// carries NO countDeltas (the delta channel is deleted). The client reacts to
/// the echo by invalidating its mailbox-count query; the refetch target — the
/// store's trigger-maintained canonical count — must already reflect the
/// mark-read when the echo is observable, which this test asserts.
#[tokio::test]
async fn mark_read_echo_carries_projection_and_no_count_deltas() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let mut mutation = mock_account_mutation("mark-read-echo-account");
    mutation.enabled = Some(true);
    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mutation)
        .await
        .expect("account should create");
    build
        .account_supervisor
        .sync_account(&account.id)
        .await
        .expect("mock account runtime should sync");

    // Seed one UNREAD message in a source mailbox (unread_emails = 1), so a
    // mark-read has a source count to move.
    let mut message = seeded_message("mr-001", "mb-inbox");
    message.keywords = Vec::new();
    build
        .api_bridge
        .store
        .apply_sync_batch(
           &posthaste_domain_service::BaseWrite::legacy("test base seed"),
            &account.id,
            &SyncBatch {
                // Counts are maintained by the store's mailbox-counter triggers
                // as the (unread) message is inserted; seed the mailbox at 0 and
                // let the trigger raise unread to 1.
                mailboxes: vec![MailboxRecord {
                    id: MailboxId::from("mb-inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 0,
                }],
                messages: vec![message],
                imap_mailbox_states: Vec::new(),
                imap_message_locations: Vec::new(),
                deleted_imap_message_locations: Vec::new(),
                deleted_mailbox_ids: Vec::new(),
                absence_deleted_imap_message_locations: Vec::new(),
                absence_deleted_message_ids: Vec::new(),
                deleted_message_ids: Vec::new(),
                replace_all_mailboxes: false,
                replace_all_messages: false,
                cursors: vec![SyncCursor {
                    object_type: SyncObject::Message,
                    state: "mr-state-1".to_string(),
                    updated_at: "2026-03-31T10:00:00Z".to_string(),
                }],
            },
        )
        .expect("unread message batch should apply");

    let caller = scoped_test_caller(account.id.as_str());
    let link = build
        .handle
        .open_link(caller.clone())
        .await
        .expect("link should open");
    let mut subscription = build
        .handle
        .subscribe_runtime_frames(
            caller.clone(),
            link.link_id.clone(),
            Some(RuntimeLinkSeq::new(0)),
        )
        .await
        .expect("runtime frames should subscribe");

    // Baseline the SOURCE mailbox's unread count directly from the store (the
    // mock account may seed sibling messages, so use a relative assertion). The
    // seeded mr-001 is unread, so mb-inbox has at least one unread.
    let unread_before = build
        .api_bridge
        .store
        .list_mailboxes(&account.id)
        .expect("list mailboxes")
        .into_iter()
        .find(|mailbox| mailbox.id == MailboxId::from("mb-inbox"))
        .expect("mb-inbox exists")
        .unread_emails;
    assert!(
        unread_before >= 1,
        "the seeded message leaves mb-inbox with an unread to clear",
    );

    // Mark the (unread) message read through the catalog mutation path — the
    // client echo path — by adding `$seen`.
    let receipt = build
        .handle
        .forward_mutation(
            caller,
            MutationRequest {
                link_id: Some(link.link_id.clone()),
                operation: serde_json::from_value(serde_json::json!({
                    "name": "message.setKeywords",
                    "args": serde_json::json!({
                        "sourceId": account.id.as_str(),
                        "messageId": "mr-001",
                        "command": {"add": ["$seen"], "remove": []}
                    }),
                }))
                .expect("typed operation parses"),
                client_mutation_id: ClientMutationId::new("mark-read"),
                context: None,
            },
        )
        .await
        .expect("mark-read mutation should run");
    assert_eq!(receipt.name, "message.setKeywords");

    // The optimistic echo — emitted before the follow-up sync — is a
    // `message.updated` Notification carrying the enriched projection. Break
    // on the first such frame (the echo).
    let payload = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let frame = subscription
                .live
                .next()
                .await
                .expect("runtime stream should remain open");
            if let RuntimeFrame::Notification { kind, payload, .. } = frame {
                if kind == EVENT_TOPIC_MESSAGE_UPDATED {
                    break payload;
                }
            }
        }
    })
    .await
    .expect("a message.updated echo should arrive on the optimistic path");

    let inner = &payload["payload"];
    assert_eq!(inner["messageId"], "mr-001");
    assert!(
        inner["projection"].is_object(),
        "the echo carries the message projection — the same enriched shape the sync path emits",
    );
    assert!(
        inner.get("countDeltas").is_none(),
        "the countDelta channel is deleted (RFC-L2-count-unification): the echo carries no counts",
    );
    assert_eq!(
        inner["projection"]["isRead"], true,
        "the echoed projection reflects the applied mark-read",
    );

    // The invalidation refetch target: by the time the echo is observable, the
    // canonical (trigger-maintained) source-mailbox unread count has already
    // dropped — a client that invalidates on this echo refetches the correct
    // value, no sync wait.
    let unread_after = build
        .api_bridge
        .store
        .list_mailboxes(&account.id)
        .expect("list mailboxes")
        .into_iter()
        .find(|mailbox| mailbox.id == MailboxId::from("mb-inbox"))
        .expect("mb-inbox exists")
        .unread_emails;
    assert_eq!(
        unread_after,
        unread_before - 1,
        "mark-read drops the canonical source-mailbox unread count by one",
    );
}

/// A sync trigger that arrives while a provider sync is in flight must still run
/// a follow-up cycle once that sync finishes — it is coalesced, never dropped.
///
/// Regression for the lost-wakeup race in `SyncTriggerState`: when the coalesced
/// trigger was stranded, the follow-up sync never ran, so changes that sync
/// would have observed (e.g. another client's mailbox change pulled on the next
/// cycle) stayed invisible until the next poll — surfacing to the user as "views
/// don't regenerate until I switch views and back".
///
/// spec: docs/L1-sync#sync-loop
#[tokio::test]
async fn coalesced_sync_trigger_still_runs_a_follow_up_cycle() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");

    let mut mutation = mock_account_mutation("coalesce-followup-account");
    mutation.enabled = Some(true);
    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mutation)
        .await
        .expect("account should create");

    // Baseline sync (ungated) establishes the connection and seeds state.
    build
        .account_supervisor
        .sync_account(&account.id)
        .await
        .expect("baseline sync should run");

    // Gate every subsequent provider pull at entry so the test controls when a
    // cycle is in flight.
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let _gate = MockJmapGateway::gate_sync_at_entry(&account.id, entered.clone(), release.clone());

    let baseline = build.account_supervisor.sync_cycle_count(&account.id).await;

    // Trigger A: enqueues a cycle. It enters the (gated) pull and blocks.
    build
        .account_supervisor
        .trigger_account_sync(&account.id, SyncTrigger::Manual)
        .await
        .expect("first trigger should enqueue");
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .expect("cycle A should enter the provider pull");

    // Trigger B arrives while A is still in flight. It must coalesce into A's
    // pending follow-up (not enqueue a redundant cycle, and not be dropped).
    build
        .account_supervisor
        .trigger_account_sync(&account.id, SyncTrigger::Push)
        .await
        .expect("second trigger should coalesce");

    // Release A. On finishing it must drain the coalesced trigger and run a
    // second cycle, which enters the gated pull again.
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .expect("the coalesced trigger should run a follow-up cycle (not be stranded)");

    // Let the follow-up cycle finish.
    release.notify_one();

    // Wait for the cycle count to settle and assert exactly the two cycles ran:
    // A plus the single coalesced follow-up.
    let final_count = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let count = build.account_supervisor.sync_cycle_count(&account.id).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            if build.account_supervisor.sync_cycle_count(&account.id).await == count {
                return count;
            }
        }
    })
    .await
    .expect("sync cycles should settle");

    assert_eq!(
        final_count - baseline,
        2,
        "trigger A plus one coalesced follow-up should run exactly two cycles"
    );
}

/// Snooze → undo (via `message.applyDiff`) must clear the snooze row. The undo's
/// mailbox restore routes through `replace_mailboxes`, whose store invariant
/// (Slice 2) deletes the snooze row when a message leaves the Snoozed mailbox.
/// This locks the wiring so a future change to `applyDiff` can't silently leave
/// an orphaned row (the scheduler would re-fire forever).
///
/// @spec docs/eph/DESIGN-L2-snooze
#[tokio::test]
async fn snooze_then_undo_apply_diff_clears_the_snooze_row() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let mut mutation = mock_account_mutation("snooze-undo-account");
    mutation.enabled = Some(true);
    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mutation)
        .await
        .expect("account should create");
    build
        .account_supervisor
        .sync_account(&account.id)
        .await
        .expect("mock account runtime should sync");
    seed_message_with_snooze_mailbox(&build, &account.id, "em-001");
    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");

    // Snooze: move em-001 to the Snoozed mailbox + record a return time.
    let snooze_receipt = build
        .handle
        .forward_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                link_id: Some(link.link_id.clone()),
                operation: serde_json::from_value(serde_json::json!({
                    "name": "message.snooze",
                    "args": serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "em-001",
                    "until": 2_000_000_000,
                }),
                }))
                .expect("typed operation parses"),
                client_mutation_id: ClientMutationId::new("snooze-1"),
                context: None,
            },
        )
        .await
        .expect("snooze should run");
    assert_eq!(snooze_receipt.state, MutationSettlementState::Confirmed);
    assert_eq!(
        build
            .api_bridge
            .store
            .list_due_snoozes(&account.id, 2_000_000_001)
            .expect("list due snoozes")
            .len(),
        1,
        "the snooze return-time row is recorded"
    );

    // Undo via `message.applyDiff`: restore em-001 to mb-inbox (remove mb-snooze).
    let undo_receipt = build
        .handle
        .forward_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                link_id: Some(link.link_id.clone()),
                operation: serde_json::from_value(serde_json::json!({
                    "name": "message.applyDiff",
                    "args": serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "em-001",
                    "diff": {
                        "keywords": {"added": [], "removed": []},
                        "mailboxes": {"added": ["mb-inbox"], "removed": ["mb-snooze"]}
                    }
                }),
                }))
                .expect("typed operation parses"),
                client_mutation_id: ClientMutationId::new("undo-1"),
                context: None,
            },
        )
        .await
        .expect("applyDiff should run");
    assert_eq!(undo_receipt.state, MutationSettlementState::Confirmed);

    // The store invariant: the undo's mailbox replace cleared the snooze row.
    assert!(
        build
            .api_bridge
            .store
            .list_due_snoozes(&account.id, 2_000_000_001)
            .expect("list due snoozes")
            .is_empty(),
        "undoing the snooze (a mailbox replace) must clear the snooze row"
    );
}

/// The send end-to-end at the mutation layer (beta-blocker guard): compose →
/// Send with NO prior draft, exactly as the web client dispatches it
/// (`message.send` named mutation over the link). The send must EXECUTE: the
/// receipt defers (Accepted + `deferredOperationId`), the outbox op flushes to
/// the provider and settles, the settlement bridge routes a terminal
/// `Confirmed` notification, and nothing is left pending in the outbox.
#[tokio::test]
async fn send_mutation_without_prior_draft_flushes_settles_and_confirms() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let mut mutation = mock_account_mutation("send-regression-account");
    mutation.enabled = Some(true);
    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mutation)
        .await
        .expect("account should create");
    build
        .account_supervisor
        .sync_account(&account.id)
        .await
        .expect("mock account runtime should sync");
    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");
    let mut subscription = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            link.link_id.clone(),
            Some(RuntimeLinkSeq::new(0)),
        )
        .await
        .expect("runtime stream should subscribe");

    let receipt = build
        .handle
        .forward_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                link_id: Some(link.link_id.clone()),
                operation: serde_json::from_value(serde_json::json!({
                    "name": "message.send",
                    "args": serde_json::json!({
                        "sourceId": account.id.as_str(),
                        "messageId": "",
                        "request": {
                            "from": null,
                            "to": [{"name": null, "email": "self@example.com"}],
                            "cc": [], "bcc": [],
                            "subject": "send regression outgoing",
                            "body": "hello",
                            "inReplyTo": null,
                            "references": null
                        }
                    }),
                }))
                .expect("typed operation parses"),
                client_mutation_id: ClientMutationId::new("send-regression-1"),
                context: None,
            },
        )
        .await
        .expect("send mutation should run");
    // The send-bridge defers: no false Confirmed at enqueue, and the receipt
    // carries the outbox op id the bridge joins the flush settlement on.
    assert_eq!(receipt.state, MutationSettlementState::Accepted);
    assert!(
        receipt.output.get("deferredOperationId").is_some(),
        "the deferred receipt must carry the outbox op id"
    );

    // The enqueue-time nudge triggers the flush; the settlement bridge must
    // then route the terminal Confirmed notification for this mutation.
    let notification = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let frame = subscription
                .live
                .next()
                .await
                .expect("runtime stream should remain open");
            if let RuntimeFrame::MutationNotification {
                client_mutation_id,
                notification,
                ..
            } = frame
            {
                break (client_mutation_id, notification);
            }
        }
    })
    .await
    .expect("the send settlement must surface as a mutation notification");
    assert_eq!(notification.0.as_str(), "send-regression-1");
    assert_eq!(notification.1, MutationNotification::Confirmed);

    // The push executed and the op settled: nothing pending, failed, or parked.
    let pending = build
        .api_bridge
        .service
        .list_pending_operations(&account.id)
        .expect("list pending");
    assert!(
        pending.is_empty(),
        "the send must flush and settle, leaving no pending/failed op: {:?}",
        pending
            .iter()
            .map(|op| (op.kind, op.state, op.last_error.clone()))
            .collect::<Vec<_>>()
    );
}

/// RFC-L2-count-unification — the OWN-mutation echo contract the web client's
/// count invalidation depends on (apps/web/src/domain-cache/handlers.ts:
/// `payloadChangeFlag(event, 'keywords')`). A `message.setKeywords` forwarded
/// over the link (the exact client path: near-end `forward` →
/// `RuntimeLink::forward_mutation` → authority `set_keywords` →
/// `publish_events` → the link's notification forwarder) MUST come back on the
/// SAME link's frame stream as a `notification` frame whose payload is the
/// DomainEvent verbatim, `changes.keywords === true` at
/// `payload.payload.changes`. This test serializes the frame exactly as the
/// SSE edge does (`frame_to_sse` → `serde_json::to_value(frame)`) and pins the
/// wire shape the web fixtures mirror.
#[tokio::test]
async fn own_set_keywords_echo_arrives_on_the_link_stream_with_change_flags() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_server(config)
        .await
        .expect("authority runtime should build");
    let mut mutation = mock_account_mutation("echo-shape-account");
    mutation.enabled = Some(true);
    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mutation)
        .await
        .expect("account should create");
    build
        .account_supervisor
        .sync_account(&account.id)
        .await
        .expect("mock account runtime should sync");
    seed_single_message_batch(&build, &account.id, "em-echo", "mb-inbox");

    let link = build
        .handle
        .open_link(RuntimeCaller::test())
        .await
        .expect("link should open");
    let mut subscription = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            link.link_id.clone(),
            Some(RuntimeLinkSeq::new(0)),
        )
        .await
        .expect("runtime stream should subscribe");

    let receipt = build
        .handle
        .forward_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                link_id: Some(link.link_id.clone()),
                operation: serde_json::from_value(serde_json::json!({
                    "name": "message.setKeywords",
                    "args": serde_json::json!({
                        "sourceId": account.id.as_str(),
                        "messageId": "em-echo",
                        "command": {"add": ["$seen"], "remove": []}
                    }),
                }))
                .expect("typed operation parses"),
                client_mutation_id: ClientMutationId::new("client-echo-1"),
                context: None,
            },
        )
        .await
        .expect("mutation should run");

    // The RECEIPT carries the SAME events (the BUNDLED ECHO — `CommandAck {
    // detail, events }` serialized as `receipt.output`). The web client's
    // entity-store adapter dispatches these through `applyDomainEvent` on
    // settlement (`dispatchReceiptEchoEvents`) so the user's OWN mutation
    // reconciles mailbox/smart-mailbox counts even when the link-stream echo
    // is dropped (the stream's lag/stale-cursor collapse never replays missed
    // notification frames). This pins the receipt half of the echo contract.
    let receipt_events = receipt.output["events"]
        .as_array()
        .expect("receipt output carries the command's events");
    assert_eq!(receipt_events.len(), 1);
    let receipt_event = &receipt_events[0];
    assert!(receipt_event["seq"].is_i64());
    assert_eq!(receipt_event["accountId"], "echo-shape-account");
    assert_eq!(receipt_event["topic"], "message.updated");
    assert!(receipt_event["occurredAt"].is_string());
    assert_eq!(
        receipt_event["payload"]["changes"]["keywords"], true,
        "the web count gate reads event.payload.changes.keywords on the receipt echo too"
    );

    // Drain the ORIGINATING link's own stream: the echo must arrive here (no
    // own-echo suppression) as a `Notification` frame.
    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let frame = subscription
                .live
                .next()
                .await
                .expect("runtime stream should remain open");
            if matches!(frame, RuntimeFrame::Notification { .. }) {
                break frame;
            }
        }
    })
    .await
    .expect("the own-mutation echo notification frame must arrive on the originating link");

    // Serialize exactly as the SSE edge does (`frame_to_sse` uses the same serde
    // path), then pin the nesting the web client reads.
    // (The web fixtures in apps/web/test/daemonEventsRealFrameDispatch.test.tsx
    // and apps/web/test/harness/scenarios/receiptEchoCountReconciliation.test.ts
    // mirror this serialized value key-for-key — re-capture here if the shape
    // ever changes.)
    let wire = serde_json::to_value(&frame).expect("frame serializes");
    assert_eq!(wire["type"], "notification");
    assert_eq!(wire["kind"], "message.updated");
    let event = &wire["payload"];
    assert!(event["seq"].is_i64(), "event.seq must be a number");
    assert_eq!(event["accountId"], "echo-shape-account");
    assert_eq!(event["topic"], "message.updated");
    assert!(event["occurredAt"].is_string());
    assert_eq!(
        event["payload"]["changes"]["keywords"], true,
        "the web guard reads event.payload.changes.keywords"
    );
    assert_eq!(event["payload"]["messageId"], "em-echo");
}
