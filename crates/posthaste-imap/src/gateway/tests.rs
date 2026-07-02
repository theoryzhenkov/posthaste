use super::*;

#[tokio::test]
async fn fetch_identity_uses_configured_sender_identity() {
    let gateway = LiveImapSmtpGateway {
        config: test_config(),
        smtp_config: test_smtp_config(),
        discovery: DiscoveredImapAccount {
            capabilities: posthaste_domain_service::ImapCapabilities::default(),
            mailboxes: Vec::new(),
        },
        store: None,
        secret_resolver: Arc::new(posthaste_domain_service::StaticSecretResolver::new("secret")),
    };

    let identity = gateway
        .fetch_identity(&AccountId::from("primary"))
        .await
        .expect("identity");

    assert_eq!(identity.email, "alice@example.test");
    assert_eq!(identity.name, "Alice Example");
}

#[test]
fn names_imap_sync_plans_for_logs() {
    assert_eq!(
        imap_sync_plan_name(&ImapMailboxSyncPlan::FullSnapshot {
            reason: posthaste_domain_service::ImapFullSyncReason::InitialSync,
        }),
        "full_snapshot"
    );
    assert_eq!(
        imap_sync_plan_name(&ImapMailboxSyncPlan::FetchNewByUid {
            after_uid: ImapUid(42),
        }),
        "fetch_new_by_uid"
    );
    assert_eq!(
        imap_sync_plan_name(&ImapMailboxSyncPlan::CondstoreDelta {
            since_modseq: posthaste_domain_service::ImapModSeq(9),
            after_uid: None,
        }),
        "condstore_delta"
    );
    assert_eq!(
        imap_sync_plan_name(&ImapMailboxSyncPlan::QresyncDelta {
            uid_validity: ImapUidValidity(1),
            since_modseq: posthaste_domain_service::ImapModSeq(9),
            after_uid: None,
        }),
        "qresync_delta"
    );
}

#[test]
fn mailbox_status_does_not_skip_without_modseq_even_when_uidnext_and_count_match() {
    let state = imap_mailbox_state(Some(ImapUid(42)));
    let status = ImapMailboxStatus {
        messages: Some(5),
        uid_next: Some(ImapUid(43)),
        uid_validity: Some(ImapUidValidity(7)),
        highest_modseq: None,
    };

    assert!(!mailbox_status_proves_unchanged(&state, 5, &status));
}

#[test]
fn mailbox_status_does_not_skip_when_uidnext_advanced() {
    let state = imap_mailbox_state(Some(ImapUid(42)));
    let status = ImapMailboxStatus {
        messages: Some(5),
        uid_next: Some(ImapUid(44)),
        uid_validity: Some(ImapUidValidity(7)),
        highest_modseq: None,
    };

    assert!(!mailbox_status_proves_unchanged(&state, 5, &status));
}

#[test]
fn mailbox_status_does_not_skip_empty_mailbox_without_modseq() {
    let state = imap_mailbox_state(None);
    let status = ImapMailboxStatus {
        messages: Some(0),
        uid_next: Some(ImapUid(1)),
        uid_validity: Some(ImapUidValidity(7)),
        highest_modseq: None,
    };

    assert!(!mailbox_status_proves_unchanged(&state, 0, &status));
}

#[test]
fn mailbox_status_requires_matching_modseq_when_available() {
    let mut state = imap_mailbox_state(Some(ImapUid(42)));
    state.highest_modseq = Some(posthaste_domain_service::ImapModSeq(100));
    let unchanged = ImapMailboxStatus {
        messages: Some(5),
        uid_next: Some(ImapUid(43)),
        uid_validity: Some(ImapUidValidity(7)),
        highest_modseq: Some(posthaste_domain_service::ImapModSeq(100)),
    };
    let changed = ImapMailboxStatus {
        highest_modseq: Some(posthaste_domain_service::ImapModSeq(101)),
        ..unchanged
    };

    assert!(mailbox_status_proves_unchanged(&state, 5, &unchanged));
    assert!(!mailbox_status_proves_unchanged(&state, 5, &changed));
}

#[test]
fn detects_missing_uid_locations_from_current_uid_listing() {
    let mailbox_id = MailboxId::from("imap:mailbox:inbox");
    let kept = ImapMessageLocation {
        message_id: MessageId::from("message-kept"),
        mailbox_id: mailbox_id.clone(),
        uid_validity: ImapUidValidity(7),
        uid: ImapUid(10),
        modseq: None,
        updated_at: "2026-04-27T00:00:00Z".to_string(),
    };
    let missing = ImapMessageLocation {
        message_id: MessageId::from("message-missing"),
        uid: ImapUid(11),
        ..kept.clone()
    };

    let deleted = missing_location_identities_from_uids(&[kept, missing], &[ImapUid(10)]);

    assert_eq!(deleted, vec![(mailbox_id, ImapUidValidity(7), ImapUid(11))]);
}

#[test]
fn accumulator_builds_partial_delta_batch_from_explicit_deleted_uids() {
    let account_id = AccountId::from("primary");
    let mailbox_id = MailboxId::from("imap:mailbox:inbox");
    let message_id = MessageId::from("message-missing");
    let local_location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: mailbox_id.clone(),
        uid_validity: ImapUidValidity(7),
        uid: ImapUid(11),
        modseq: None,
        updated_at: "2026-04-27T00:00:00Z".to_string(),
    };
    let mut accumulator = SyncBatchAccumulator::default();
    accumulator.add_local_locations(std::slice::from_ref(&local_location));
    accumulator.add_deleted_uid_identities(vec![(mailbox_id, ImapUidValidity(7), ImapUid(11))]);

    let batch = accumulator.into_sync_batch(
        &account_id,
        DiscoveredImapAccount {
            capabilities: ImapCapabilities::default(),
            mailboxes: Vec::new(),
        },
        false,
        true,
        false,
        "2026-04-27T00:00:00Z".to_string(),
    );

    assert!(!batch.replace_all_messages);
    assert_eq!(
        batch.deleted_imap_message_locations,
        vec![local_location.key()]
    );
    assert_eq!(batch.deleted_message_ids, vec![message_id]);
    assert!(batch.messages.is_empty());
    assert!(batch.imap_message_locations.is_empty());
}

#[tokio::test]
async fn fetch_body_reports_clear_unsupported_error() {
    let gateway = LiveImapSmtpGateway {
        config: test_config(),
        smtp_config: test_smtp_config(),
        discovery: DiscoveredImapAccount {
            capabilities: posthaste_domain_service::ImapCapabilities::default(),
            mailboxes: Vec::new(),
        },
        store: None,
        secret_resolver: Arc::new(posthaste_domain_service::StaticSecretResolver::new("secret")),
    };

    let error = gateway
        .fetch_message_body(&AccountId::from("primary"), &MessageId::from("message"))
        .await
        .expect_err("body fetch is not implemented");

    assert!(matches!(error, GatewayError::Rejected(message) if message.contains("discovery")));
}

#[test]
fn simple_move_uses_uid_move_when_server_supports_move() {
    let delta = crate::ImapMailboxReplacementDelta {
        add: vec![MailboxId::from("archive")],
        remove: vec![MailboxId::from("inbox")],
    };

    let planned = simple_imap_move_mailboxes(&ImapCapabilities::from_tokens(["MOVE"]), &delta)
        .map(|(source, target)| (source.clone(), target.clone()));

    assert_eq!(
        planned,
        Some((MailboxId::from("inbox"), MailboxId::from("archive")))
    );
}

#[test]
fn simple_move_falls_back_when_move_is_unavailable() {
    let delta = crate::ImapMailboxReplacementDelta {
        add: vec![MailboxId::from("archive")],
        remove: vec![MailboxId::from("inbox")],
    };

    let planned = simple_imap_move_mailboxes(&ImapCapabilities::from_tokens(["IMAP4rev1"]), &delta);

    assert!(planned.is_none());
}

#[test]
fn simple_move_does_not_apply_to_copy_or_multi_mailbox_changes() {
    let copy_delta = crate::ImapMailboxReplacementDelta {
        add: vec![MailboxId::from("archive")],
        remove: Vec::new(),
    };
    let multi_delta = crate::ImapMailboxReplacementDelta {
        add: vec![MailboxId::from("archive"), MailboxId::from("project")],
        remove: vec![MailboxId::from("inbox")],
    };
    let capabilities = ImapCapabilities::from_tokens(["MOVE", "UIDPLUS"]);

    assert!(simple_imap_move_mailboxes(&capabilities, &copy_delta).is_none());
    assert!(simple_imap_move_mailboxes(&capabilities, &multi_delta).is_none());
}

fn test_config() -> ImapConnectionConfig {
    ImapConnectionConfig {
        host: "imap.example.test".to_string(),
        port: 993,
        security: posthaste_domain_service::TransportSecurity::Tls,
        username: "alice@example.test".to_string(),
        secret: "secret".to_string(),
        auth: posthaste_domain_service::ProviderAuthKind::Password,
    }
}

fn test_smtp_config() -> SmtpConnectionConfig {
    SmtpConnectionConfig {
        host: "smtp.example.test".to_string(),
        port: 587,
        security: posthaste_domain_service::TransportSecurity::StartTls,
        sender_name: Some("Alice Example".to_string()),
        sender_email: "alice@example.test".to_string(),
        username: "alice-login".to_string(),
        secret: "secret".to_string(),
        auth: posthaste_domain_service::ProviderAuthKind::Password,
        provider: posthaste_domain_service::ProviderHint::Generic,
    }
}

fn imap_mailbox_state(highest_uid: Option<ImapUid>) -> ImapMailboxSyncState {
    ImapMailboxSyncState {
        mailbox_id: MailboxId::from("imap:mailbox:inbox"),
        mailbox_name: "INBOX".to_string(),
        uid_validity: ImapUidValidity(7),
        highest_uid,
        highest_modseq: None,
        updated_at: "2026-04-27T00:00:00Z".to_string(),
    }
}

#[derive(Debug)]
struct CountingResolver {
    count: std::sync::atomic::AtomicUsize,
    secret: String,
}

impl CountingResolver {
    fn new(secret: impl Into<String>) -> Self {
        Self {
            count: std::sync::atomic::AtomicUsize::new(0),
            secret: secret.into(),
        }
    }

    fn call_count(&self) -> usize {
        self.count.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl posthaste_domain_service::SecretResolver for CountingResolver {
    async fn resolve_secret(&self) -> Result<String, posthaste_domain_service::GatewayError> {
        self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(self.secret.clone())
    }
}

#[tokio::test]
async fn gateway_resolves_fresh_secret_before_each_connection() {
    let resolver = Arc::new(CountingResolver::new("fresh-secret"));
    let gateway = LiveImapSmtpGateway {
        config: test_config(),
        smtp_config: test_smtp_config(),
        discovery: DiscoveredImapAccount {
            capabilities: ImapCapabilities::default(),
            mailboxes: Vec::new(),
        },
        store: None,
        secret_resolver: resolver.clone(),
    };

    let imap_config = gateway.resolve_imap_config().await.expect("resolve imap");
    assert_eq!(imap_config.secret, "fresh-secret");
    assert_eq!(resolver.call_count(), 1);

    let smtp_config = gateway.resolve_smtp_config().await.expect("resolve smtp");
    assert_eq!(smtp_config.secret, "fresh-secret");
    assert_eq!(resolver.call_count(), 2);
}
