use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use posthaste_authority_runtime::oauth::OAuthTokenSet;
use posthaste_authority_runtime::{build_authority_runtime, from_api_bridge_for_migration};
use posthaste_domain_service::{
    AccountDriver, AccountId, EventFilter, ImapTransportSettings, MailboxId, MailboxRecord,
    MessageId, MessageRecord, MessageSortField, ProviderAuthKind, ProviderHint, SecretRef,
    SecretStore, SecretStoreError, SetKeywordsCommand, SmtpTransportSettings, SortDirection,
    SyncBatch, SyncCursor, SyncObject, SyncTrigger, ThreadId, TransportSecurity,
    EVENT_TOPIC_ACCOUNT_DELETED, EVENT_TOPIC_MESSAGE_UPDATED,
};
use posthaste_engine::MockJmapGateway;
use posthaste_client_link::RuntimeLinkOps;
use posthaste_runtime::{RuntimeBuildConfig, RuntimeBuildError};
use posthaste_contract_core::RuntimeCaller;
use posthaste_runtime_api::RuntimeAccountApi;
use posthaste_contract_core::{
    AccountTransportMutation, ClientMutationId, CreateAccountMutation, MailListViewState,
    MailPresentationRequest, MailQueryRequest, MutationNotification, MutationRequest,
    MutationSettlementState, RuntimeErrorCode, RuntimeFrame, RuntimeLifecycle, RuntimeSessionSeq,
    SecretWriteMode, SecretWriteMutation, ViewDescriptor, ViewFrame, ViewRevision,
};
use tokio::sync::Notify;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("posthaste-authority-runtime-test-{now}-{seq}"))
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
    build: &posthaste_authority_runtime::AuthorityRuntimeBuild,
    account_id: &AccountId,
) {
    build
        .api_bridge
        .store
        .apply_sync_batch(
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
    build: &posthaste_authority_runtime::AuthorityRuntimeBuild,
    account_id: &AccountId,
    message_id: &str,
    mailbox_id: &str,
) {
    build
        .api_bridge
        .store
        .apply_sync_batch(
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
    build: &posthaste_authority_runtime::AuthorityRuntimeBuild,
    account_id: &AccountId,
    message_id: &str,
) {
    build
        .api_bridge
        .store
        .apply_sync_batch(
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
    build: &posthaste_authority_runtime::AuthorityRuntimeBuild,
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
// spec: docs/backend/L2#runtime-build-before-adapters
#[tokio::test]
async fn build_from_empty_roots_reports_ready_status_without_http_or_tauri() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));

    let build = build_authority_runtime(config)
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
    let build = build_authority_runtime(config)
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
    let build = build_authority_runtime(config)
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
    let build = build_authority_runtime(config)
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

    let build = build_authority_runtime(config)
        .await
        .expect("authority runtime should build");

    let created = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            CreateAccountMutation {
                id: Some("acct-builder".to_string()),
                name: "Builder Account".to_string(),
                driver: Some(posthaste_domain_service::AccountDriver::Mock),
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

// spec: docs/backend/L3#account-mutations-runtime-backed
#[tokio::test]
async fn mail_list_view_replaces_snapshot_after_keyword_event() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_runtime(config)
        .await
        .expect("authority runtime should build");
    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mock_account_mutation("view-account"))
        .await
        .expect("account should create");
    seed_message_batch(&build, &account.id);

    let snapshot = build
        .handle
        .open_view(
            RuntimeCaller::test(),
            mail_list_descriptor("in:view-account/inbox"),
        )
        .await
        .expect("mail list view should open");
    assert_eq!(snapshot.revision.get(), 1);
    let state = mail_list_state(&snapshot);
    assert_eq!(state.rows.len(), 2);

    let mut subscription = build
        .handle
        .subscribe_view(
            RuntimeCaller::test(),
            snapshot.view_id.clone(),
            Some(snapshot.revision),
        )
        .await
        .expect("view should subscribe");
    assert!(subscription.catch_up.is_none());

    let result = build
        .api_bridge
        .store
        .set_keywords(
            &account.id,
            &MessageId::from("message-1"),
            None,
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .expect("keyword command should write");
    for event in result.events {
        build
            .api_bridge
            .event_sender
            .send(event)
            .expect("event should broadcast");
    }

    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), subscription.live.next())
        .await
        .expect("view frame should arrive")
        .expect("view stream should remain open");
    let ViewFrame::Replace { snapshot } = frame else {
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
        .subscribe_view(
            RuntimeCaller::test(),
            snapshot.view_id.clone(),
            Some(ViewRevision::new(1)),
        )
        .await
        .expect("behind subscriber should get collapsed catch-up")
        .catch_up
        .expect("behind subscriber should get snapshot");
    let ViewFrame::Snapshot { snapshot } = catch_up else {
        panic!("catch-up should be a fresh snapshot");
    };
    assert_eq!(snapshot.revision.get(), 2);
}

#[tokio::test]
async fn runtime_session_ids_are_not_predictable_counters() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_runtime(config)
        .await
        .expect("authority runtime should build");

    let first = build
        .handle
        .open_session(RuntimeCaller::test())
        .await
        .expect("first session should open");
    let second = build
        .handle
        .open_session(RuntimeCaller::test())
        .await
        .expect("second session should open");

    for session_id in [&first.session_id, &second.session_id] {
        let raw = session_id.as_str();
        let suffix = raw
            .strip_prefix("session-")
            .expect("session id should carry the session prefix");
        assert_eq!(suffix.len(), 32);
        assert!(suffix.chars().all(|ch| ch.is_ascii_hexdigit()));
    }
    assert_ne!(first.session_id.as_str(), "session-1");
    assert_ne!(second.session_id.as_str(), "session-2");
}

#[tokio::test]
async fn runtime_session_stream_carries_keyword_view_replace_frames() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_runtime(config)
        .await
        .expect("authority runtime should build");
    let account = build
        .handle
        .create_account(
            RuntimeCaller::test(),
            mock_account_mutation("runtime-session-account"),
        )
        .await
        .expect("account should create");
    seed_message_batch(&build, &account.id);

    let session = build
        .handle
        .open_session(RuntimeCaller::test())
        .await
        .expect("session should open");
    let snapshot = build
        .handle
        .open_session_view(
            RuntimeCaller::test(),
            session.session_id.clone(),
            mail_list_descriptor("in:runtime-session-account/inbox"),
        )
        .await
        .expect("session view should open");
    let mut subscription = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            session.session_id.clone(),
            Some(RuntimeSessionSeq::new(0)),
        )
        .await
        .expect("runtime stream should subscribe");
    assert_eq!(subscription.catch_up.len(), 1);
    let RuntimeFrame::ViewSnapshot {
        session_seq,
        view_id,
        revision,
        ..
    } = &subscription.catch_up[0]
    else {
        panic!("expected collapsed view snapshot");
    };
    assert_eq!(session_seq.get(), 1);
    assert_eq!(view_id, &snapshot.view_id);
    assert_eq!(revision.get(), 1);

    let result = build
        .api_bridge
        .store
        .set_keywords(
            &account.id,
            &MessageId::from("message-1"),
            None,
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
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
        session_seq,
        view_id,
        revision,
        snapshot,
    } = frame
    else {
        panic!("expected runtime view replace frame");
    };
    assert!(session_seq.get() >= 2);
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
    let build = build_authority_runtime(config)
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
    let session = build
        .handle
        .open_session(RuntimeCaller::test())
        .await
        .expect("session should open");
    let mut subscription = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            session.session_id.clone(),
            Some(RuntimeSessionSeq::new(0)),
        )
        .await
        .expect("runtime stream should subscribe");
    assert!(subscription.catch_up.is_empty());

    let receipt = build
        .handle
        .run_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                session_id: Some(session.session_id.clone()),
                name: "message.setKeywords".to_string(),
                args: serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "em-001",
                    "command": {"add": ["$flagged"], "remove": []}
                }),
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
        .run_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                session_id: Some(session.session_id),
                name: "message.setKeywords".to_string(),
                args: serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "em-001",
                    "command": {"add": ["$flagged"], "remove": []}
                }),
                client_mutation_id: ClientMutationId::new("client-1"),
                context: None,
            },
        )
        .await
        .expect("duplicate mutation should return existing receipt");
    assert_eq!(duplicate.runtime_mutation_id, Some(mutation_id));
    assert_eq!(duplicate.state, MutationSettlementState::Confirmed);
}

/// Cost contract: a state-assertion mutation acknowledges the change; it must
/// NOT shuttle the message body. The settlement payload (`receipt.output`,
/// serialized onto the session stream) must stay bounded regardless of how
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
    let build = build_authority_runtime(config)
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
    let session = build
        .handle
        .open_session(RuntimeCaller::test())
        .await
        .expect("session should open");

    let receipt = build
        .handle
        .run_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                session_id: Some(session.session_id.clone()),
                name: "message.setKeywords".to_string(),
                args: serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "em-001",
                    "command": {"add": ["$flagged"], "remove": []}
                }),
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
    let build = build_authority_runtime(config)
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

    let snapshot = build
        .handle
        .open_view(
            RuntimeCaller::test(),
            message_detail_descriptor(account.id.as_str(), "message-1"),
        )
        .await
        .expect("message detail view should open");
    assert_eq!(snapshot.revision.get(), 1);
    assert_eq!(snapshot.data["id"], "message-1");
    assert_eq!(snapshot.data["isFlagged"], false);

    let mut subscription = build
        .handle
        .subscribe_view(
            RuntimeCaller::test(),
            snapshot.view_id.clone(),
            Some(snapshot.revision),
        )
        .await
        .expect("view should subscribe");
    assert!(subscription.catch_up.is_none());

    let result = build
        .api_bridge
        .store
        .set_keywords(
            &account.id,
            &MessageId::from("message-1"),
            None,
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .expect("keyword command should write");
    for event in result.events {
        build
            .api_bridge
            .event_sender
            .send(event)
            .expect("event should broadcast");
    }

    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), subscription.live.next())
        .await
        .expect("view frame should arrive")
        .expect("view stream should remain open");
    let ViewFrame::Replace { snapshot } = frame else {
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
    let build = build_authority_runtime(config)
        .await
        .expect("authority runtime should build");
    let account = build
        .handle
        .create_account(RuntimeCaller::test(), mock_account_mutation("conv-account"))
        .await
        .expect("account should create");
    seed_message_batch(&build, &account.id);

    // Resolve message-1's conversation id from a list snapshot.
    let list = build
        .handle
        .open_view(
            RuntimeCaller::test(),
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
        .open_view(
            RuntimeCaller::test(),
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
        .subscribe_view(
            RuntimeCaller::test(),
            snapshot.view_id.clone(),
            Some(snapshot.revision),
        )
        .await
        .expect("view should subscribe");
    assert!(subscription.catch_up.is_none());

    let result = build
        .api_bridge
        .store
        .set_keywords(
            &account.id,
            &MessageId::from("message-1"),
            None,
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .expect("keyword command should write");
    for event in result.events {
        build
            .api_bridge
            .event_sender
            .send(event)
            .expect("event should broadcast");
    }

    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), subscription.live.next())
        .await
        .expect("view frame should arrive")
        .expect("view stream should remain open");
    let ViewFrame::Replace { snapshot } = frame else {
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
    let build = build_authority_runtime(config)
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
    let session = build
        .handle
        .open_session(RuntimeCaller::test())
        .await
        .expect("session should open");

    let receipt = build
        .handle
        .run_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                session_id: Some(session.session_id.clone()),
                name: "message.setReadState".to_string(),
                args: serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "em-001",
                    "read": true
                }),
                client_mutation_id: ClientMutationId::new("read-1"),
                context: None,
            },
        )
        .await
        .expect("setReadState mutation should run");
    assert_eq!(receipt.name, "message.setReadState");
    assert_eq!(receipt.state, MutationSettlementState::Confirmed);
    assert_eq!(receipt.output["events"].as_array().unwrap().len(), 1);

    let unknown = build
        .handle
        .run_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                session_id: Some(session.session_id),
                name: "message.nonsense".to_string(),
                args: serde_json::json!({}),
                client_mutation_id: ClientMutationId::new("bad-1"),
                context: None,
            },
        )
        .await;
    assert!(unknown.is_err(), "unknown mutation names are rejected");
}

#[tokio::test]
async fn runtime_session_view_extends_its_window_in_place() {
    // A windowed mailList view grows in place: extend re-queries the larger
    // window, keeps the same view id, and broadcasts a ViewReplace.
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_runtime(config)
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
    let session = build
        .handle
        .open_session(RuntimeCaller::test())
        .await
        .expect("session should open");

    // Open with a one-row window so the second seeded message is past it.
    let snapshot = build
        .handle
        .open_session_view(
            RuntimeCaller::test(),
            session.session_id.clone(),
            mail_list_descriptor_with_limit("in:runtime-extend-account/inbox", 1),
        )
        .await
        .expect("session view should open");
    let opened = mail_list_state(&snapshot);
    assert_eq!(opened.rows.len(), 1);
    assert!(opened.continuation.has_after, "more rows past the window");

    let mut subscription = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            session.session_id.clone(),
            Some(RuntimeSessionSeq::new(0)),
        )
        .await
        .expect("runtime stream should subscribe");

    let extended = build
        .handle
        .extend_session_view(
            RuntimeCaller::test(),
            session.session_id.clone(),
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
        .open_session_view(
            RuntimeCaller::test(),
            session.session_id.clone(),
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
        .extend_session_view(
            RuntimeCaller::test(),
            session.session_id.clone(),
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
    let build = build_authority_runtime(config)
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

    let session = build
        .handle
        .open_session(RuntimeCaller::test())
        .await
        .expect("session should open");
    let snapshot = build
        .handle
        .open_session_view(
            RuntimeCaller::test(),
            session.session_id.clone(),
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
            session.session_id.clone(),
            Some(RuntimeSessionSeq::new(0)),
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
    let build = build_authority_runtime(config)
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

    let denied = build
        .handle
        .open_view(
            scoped_test_caller("other-account"),
            mail_list_descriptor("in:view-account-scope/inbox"),
        )
        .await
        .expect_err("out-of-scope view should be rejected");
    assert_eq!(denied.envelope().code, RuntimeErrorCode::InvalidDescriptor);

    let snapshot = build
        .handle
        .open_view(
            scoped_test_caller("view-account-scope"),
            mail_list_descriptor("in:view-account-scope/inbox"),
        )
        .await
        .expect("matching account scope should open");
    let subscription = build
        .handle
        .subscribe_view(
            scoped_test_caller("other-account"),
            snapshot.view_id,
            Some(snapshot.revision),
        )
        .await;
    let Err(error) = subscription else {
        panic!("out-of-scope subscription should be rejected");
    };
    assert_eq!(error.envelope().code, RuntimeErrorCode::InvalidDescriptor);
}

#[tokio::test]
async fn mail_list_view_fans_out_keyword_replaces_to_all_subscribers() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_runtime(config)
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

    let snapshot = build
        .handle
        .open_view(
            RuntimeCaller::test(),
            mail_list_descriptor("in:view-account-fanout/inbox"),
        )
        .await
        .expect("mail list view should open");
    let mut first = build
        .handle
        .subscribe_view(
            RuntimeCaller::test(),
            snapshot.view_id.clone(),
            Some(snapshot.revision),
        )
        .await
        .expect("first subscriber should open");
    let mut second = build
        .handle
        .subscribe_view(
            RuntimeCaller::test(),
            snapshot.view_id.clone(),
            Some(snapshot.revision),
        )
        .await
        .expect("second subscriber should open");

    let result = build
        .api_bridge
        .store
        .set_keywords(
            &account.id,
            &MessageId::from("message-1"),
            None,
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .expect("keyword command should write");
    for event in result.events {
        build
            .api_bridge
            .event_sender
            .send(event)
            .expect("event should broadcast");
    }

    for subscription in [&mut first, &mut second] {
        let frame =
            tokio::time::timeout(std::time::Duration::from_secs(2), subscription.live.next())
                .await
                .expect("view frame should arrive")
                .expect("view stream should remain open");
        let ViewFrame::Replace { snapshot } = frame else {
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
    let build = build_authority_runtime(config)
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

    let snapshot = build
        .handle
        .open_view(
            RuntimeCaller::test(),
            mail_list_descriptor("in:view-account-reconnect/inbox"),
        )
        .await
        .expect("mail list view should open");

    let result = build
        .api_bridge
        .store
        .set_keywords(
            &account.id,
            &MessageId::from("message-1"),
            None,
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
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
                .subscribe_view(
                    RuntimeCaller::test(),
                    snapshot.view_id.clone(),
                    Some(ViewRevision::new(1)),
                )
                .await
                .expect("subscription should open");
            if let Some(ViewFrame::Snapshot { snapshot }) = subscription.catch_up {
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
    let build = build_authority_runtime(config)
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

    let snapshot = build
        .handle
        .open_view(
            RuntimeCaller::test(),
            mail_list_descriptor("in:view-account-flagged/inbox is:flagged"),
        )
        .await
        .expect("flagged view should open");
    assert!(mail_list_state(&snapshot).rows.is_empty());
    let mut subscription = build
        .handle
        .subscribe_view(
            RuntimeCaller::test(),
            snapshot.view_id.clone(),
            Some(snapshot.revision),
        )
        .await
        .expect("view should subscribe");

    let result = build
        .api_bridge
        .store
        .set_keywords(
            &account.id,
            &MessageId::from("message-1"),
            None,
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .expect("keyword command should write");
    for event in result.events {
        build
            .api_bridge
            .event_sender
            .send(event)
            .expect("event should broadcast");
    }

    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), subscription.live.next())
        .await
        .expect("view frame should arrive")
        .expect("view stream should remain open");
    let ViewFrame::Replace { snapshot } = frame else {
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
    let build = build_authority_runtime(config)
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

    let snapshot = build
        .handle
        .open_view(
            RuntimeCaller::test(),
            mail_list_descriptor("in:view-account-2/archive"),
        )
        .await
        .expect("unaffected view should open");
    assert!(mail_list_state(&snapshot).rows.is_empty());
    let mut subscription = build
        .handle
        .subscribe_view(
            RuntimeCaller::test(),
            snapshot.view_id.clone(),
            Some(snapshot.revision),
        )
        .await
        .expect("view should subscribe");

    let result = build
        .api_bridge
        .store
        .set_keywords(
            &account.id,
            &MessageId::from("message-1"),
            None,
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .expect("keyword command should write");
    for event in result.events {
        build
            .api_bridge
            .event_sender
            .send(event)
            .expect("event should broadcast");
    }

    let no_frame = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        subscription.live.next(),
    )
    .await;
    assert!(
        no_frame.is_err(),
        "unaffected view should not receive a frame"
    );
}

#[tokio::test]
async fn create_account_duplicate_id_conflicts_without_overwriting_config_or_secret() {
    let root = temp_root();
    let secret_store = Arc::new(TestSecretStore::default());
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(secret_store.clone());

    let build = build_authority_runtime(config)
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

// spec: docs/backend/L3#account-assets-runtime-backed
// spec: docs/runtime/internals/L3#account-resource-linkage-runtime-owned
#[tokio::test]
async fn delete_account_removes_secret_config_and_publishes_event_through_runtime() {
    let root = temp_root();
    let secret_store = Arc::new(TestSecretStore::default());
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(secret_store.clone());

    let build = build_authority_runtime(config)
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

// spec: docs/backend/L3#account-mutations-runtime-backed
// spec: docs/runtime/internals/L3#account-mutation-contract-pattern
#[tokio::test]
async fn oauth_token_persistence_writes_secret_and_patches_account_through_runtime() {
    let root = temp_root();
    let secret_store = Arc::new(TestSecretStore::default());
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(secret_store.clone());

    let build = build_authority_runtime(config)
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

    let build = build_authority_runtime(config)
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
async fn runtime_session_stream_carries_scoped_domain_event_notifications() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));

    let build = build_authority_runtime(config)
        .await
        .expect("authority runtime should build");
    let caller = RuntimeCaller {
        account_scope: Some(vec!["primary".to_string()]),
        ..RuntimeCaller::test()
    };
    let session = build
        .handle
        .open_session(caller.clone())
        .await
        .expect("runtime session should open");
    let mut subscription = build
        .handle
        .subscribe_runtime_frames(caller, session.session_id, Some(RuntimeSessionSeq::new(0)))
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
        session_seq,
        kind,
        payload,
    } = frame
    else {
        panic!("expected notification frame");
    };
    assert_eq!(session_seq.get(), 1);
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

    let error = match build_authority_runtime(config).await {
        Ok(_) => panic!("zero-capacity event channel should be rejected before build side effects"),
        Err(error) => error,
    };

    assert!(matches!(error, RuntimeBuildError::InvalidConfig(_)));
}

/// A backend link transport whose up-channel blocks until released — a test seam
/// for observing the runtime's outbox overlay while a mutation is in flight.
struct DeferredTransport {
    inner: Arc<dyn posthaste_link_contract::BackendApi>,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait]
impl posthaste_link_contract::BackendApi for DeferredTransport {
    async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<posthaste_contract_core::MutationReceipt, posthaste_contract_core::RuntimeError>
    {
        // Signal that the forward has begun (so the outbox already holds the
        // mutation), then wait for the test to release it.
        self.entered.notify_one();
        self.release.notified().await;
        Ok(posthaste_contract_core::MutationReceipt {
            runtime_mutation_id: Some(posthaste_contract_core::RuntimeMutationId::new(
                "backend-deferred",
            )),
            client_mutation_id: mutation.client_mutation_id,
            name: mutation.name,
            state: MutationSettlementState::Confirmed,
            error: None,
            output: serde_json::json!({ "events": [] }),
        })
    }

    async fn subscribe(
        &self,
        coverage: posthaste_link_contract::LinkCoverage,
    ) -> Result<posthaste_link_contract::DownStream, posthaste_contract_core::RuntimeError> {
        self.inner.subscribe(coverage).await
    }

    // Everything other than the gated up-channel delegates to the real backend,
    // so setup (account creation) hits the live store the local reads observe.
    // (forward_mutation deliberately does *not* delegate: it confirms without
    // applying, so the test can prove the optimistic overlay reverts on retire.)
    async fn create_account(
        &self,
        mutation: CreateAccountMutation,
    ) -> Result<posthaste_domain_service::AccountOverview, posthaste_contract_core::RuntimeError> {
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

// spec: docs/replication/backend-link/L2#5-the-runtime-near-node-read-replica-outbox
#[tokio::test]
async fn runtime_serves_optimistic_rows_from_its_outbox_while_a_forward_is_in_flight() {
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let entered_for_transport = entered.clone();
    let release_for_transport = release.clone();

    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()))
            // Decorate the real in-process transport: gate the up-channel, delegate the
            // rest (so account-creation setup reaches the live backend).
            .with_backend_transport_override(move |inner| {
                Arc::new(DeferredTransport {
                    inner,
                    entered: entered_for_transport,
                    release: release_for_transport,
                })
            });
    let build = build_authority_runtime(config)
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
    let session = build
        .handle
        .open_session(RuntimeCaller::test())
        .await
        .expect("session should open");

    let descriptor = mail_list_descriptor("in:optimism-account/inbox");

    // Baseline: message-1 is not flagged.
    let baseline = build
        .handle
        .open_view(RuntimeCaller::test(), descriptor.clone())
        .await
        .expect("baseline view should open");
    assert!(!flagged(&mail_list_state(&baseline), "message-1"));

    // Forward a flag mutation whose up-channel blocks, leaving it in the outbox.
    let handle = build.handle.clone();
    let session_id = session.session_id.clone();
    let account_id = account.id.as_str().to_string();
    let task = tokio::spawn(async move {
        handle
            .run_mutation(
                RuntimeCaller::test(),
                MutationRequest {
                    session_id: Some(session_id),
                    name: "message.setFlaggedState".to_string(),
                    args: serde_json::json!({
                        "sourceId": account_id,
                        "messageId": "message-1",
                        "flagged": true,
                    }),
                    client_mutation_id: ClientMutationId::new("client-flag"),
                    context: None,
                },
            )
            .await
            .expect("mutation should run")
    });

    // Wait for the forward to begin (the outbox now holds the flag).
    entered.notified().await;

    // While the forward is in flight, the runtime serves the row optimistically
    // flagged — folded from its outbox via the shared MailListReplica.
    let optimistic = build
        .handle
        .open_view(RuntimeCaller::test(), descriptor.clone())
        .await
        .expect("optimistic view should open");
    assert!(
        flagged(&mail_list_state(&optimistic), "message-1"),
        "the in-flight mutation should show optimistically"
    );

    // Release the forward; the mutation completes and the outbox retires.
    release.notify_one();
    task.await.expect("mutation task should join");

    // The deferred backend never applied the change, so once the outbox retires
    // the served row reflects the (unchanged) authoritative store again.
    let settled = build
        .handle
        .open_view(RuntimeCaller::test(), descriptor)
        .await
        .expect("settled view should open");
    assert!(
        !flagged(&mail_list_state(&settled), "message-1"),
        "the overlay should retire once the forward completes"
    );
}

// spec: docs/L1-sync#sync-loop
#[tokio::test]
async fn rapid_mutation_burst_coalesces_provider_sync_triggers() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_runtime(config)
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

    // Slow down the mock provider sync so concurrent mutation triggers overlap
    // with an in-flight sync and exercise the coalescing path.
    MockJmapGateway::set_sync_delay_for_tests(50);

    let session = build
        .handle
        .open_session(RuntimeCaller::test())
        .await
        .expect("session should open");

    // Fire 15 rapid flag/unflag toggles concurrently. Under the old behavior
    // each toggle would enqueue a full provider sync; with coalescing they
    // collapse into at most one in-flight + one pending follow-up cycle.
    let handle = build.handle.clone();
    let session_id = session.session_id.clone();
    let account_id = account.id.clone();
    let mut burst = Vec::with_capacity(15);
    for i in 0..15 {
        let handle = handle.clone();
        let session_id = session_id.clone();
        let account_id = account_id.clone();
        let (add, remove) = if i % 2 == 0 {
            (vec!["$flagged"], Vec::<&str>::new())
        } else {
            (Vec::<&str>::new(), vec!["$flagged"])
        };
        burst.push(tokio::spawn(async move {
            handle
                .run_mutation(
                    RuntimeCaller::test(),
                    MutationRequest {
                        session_id: Some(session_id),
                        name: "message.setKeywords".to_string(),
                        args: serde_json::json!({
                            "sourceId": account_id.as_str(),
                            "messageId": "em-001",
                            "command": {"add": add, "remove": remove}
                        }),
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

    // Release the delay so the settle wait is fast and other tests are unaffected.
    MockJmapGateway::clear_sync_delay_for_tests();

    // Wait until no new sync cycle starts for a short interval, proving the
    // burst has drained. Mock syncs are fast; cap total wait at 2 seconds.
    let final_count = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let count = build.account_supervisor.sync_cycle_count(&account.id).await;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if build.account_supervisor.sync_cycle_count(&account.id).await == count {
                return count;
            }
        }
    })
    .await
    .expect("sync cycles should settle within timeout");

    let additional_cycles = final_count.saturating_sub(baseline);
    assert!(
        additional_cycles <= 2,
        "15 rapid mutations should produce at most 2 additional provider sync cycles, got {additional_cycles}"
    );
}

#[tokio::test]
async fn runtime_mutation_in_one_session_updates_view_in_another_session() {
    let root = temp_root();
    let config =
        RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
            .with_secret_store(Arc::new(TestSecretStore::default()));
    let build = build_authority_runtime(config)
        .await
        .expect("authority runtime should build");
    let mut mutation = mock_account_mutation("cross-session-mutation-account");
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

    let session_a = build
        .handle
        .open_session(RuntimeCaller::test())
        .await
        .expect("session A should open");
    let snapshot_a = build
        .handle
        .open_session_view(
            RuntimeCaller::test(),
            session_a.session_id.clone(),
            mail_list_descriptor("in:cross-session-mutation-account/mb-inbox"),
        )
        .await
        .expect("session A view should open");
    let mut subscription_a = build
        .handle
        .subscribe_runtime_frames(
            RuntimeCaller::test(),
            session_a.session_id.clone(),
            Some(RuntimeSessionSeq::new(0)),
        )
        .await
        .expect("session A stream should subscribe");
    assert_eq!(subscription_a.catch_up.len(), 1);

    let session_b = build
        .handle
        .open_session(RuntimeCaller::test())
        .await
        .expect("session B should open");

    let receipt = build
        .handle
        .run_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                session_id: Some(session_b.session_id.clone()),
                name: "message.setKeywords".to_string(),
                args: serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "xm-001",
                    "command": {"add": ["$flagged"], "remove": []}
                }),
                client_mutation_id: ClientMutationId::new("client-b"),
                context: None,
            },
        )
        .await
        .expect("session B mutation should run");

    assert_eq!(receipt.name, "message.setKeywords");
    assert_eq!(receipt.state, MutationSettlementState::Confirmed);

    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let frame = subscription_a
                .live
                .next()
                .await
                .expect("session A stream should remain open");
            if matches!(frame, RuntimeFrame::ViewReplace { .. }) {
                break frame;
            }
        }
    })
    .await
    .expect("session A should receive a view update after session B mutation");
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
    let build = build_authority_runtime(config)
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
    let build = build_authority_runtime(config)
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
    let session = build
        .handle
        .open_session(RuntimeCaller::test())
        .await
        .expect("session should open");

    // Snooze: move em-001 to the Snoozed mailbox + record a return time.
    let snooze_receipt = build
        .handle
        .run_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                session_id: Some(session.session_id.clone()),
                name: "message.snooze".to_string(),
                args: serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "em-001",
                    "until": 2_000_000_000,
                }),
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
        .run_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                session_id: Some(session.session_id.clone()),
                name: "message.applyDiff".to_string(),
                args: serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "em-001",
                    "diff": {
                        "keywords": {"added": [], "removed": []},
                        "mailboxes": {"added": ["mb-inbox"], "removed": ["mb-snooze"]}
                    }
                }),
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
