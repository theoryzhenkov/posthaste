//! Shared helpers for `posthaste-testkit` integration tests.
//!
//! Each file in `tests/` is its own test binary, so shared helpers live here
//! and are pulled in via `#[path = "common/mod.rs"] mod common;`.
//!
//! The mock-IMAP suites drive the surviving stack directly: a [`Harness`]
//! (`MailService` + store) with an `ImapSmtp` account pointed at a
//! [`GmailImapFixture`], pushed and pulled through a real
//! [`LiveImapSmtpGateway`] — the exact gateway the app assembles.

// Each test binary compiles this module independently and uses a different
// subset of the helpers.
#![allow(dead_code)]

use std::sync::Arc;

use posthaste_domain_model::{
    AccountDriver, AccountId, MailboxId, MessageSummary, ProviderAuthKind, SyncTrigger,
    TransportSecurity,
};
use posthaste_domain_service::StaticSecretResolver;
use posthaste_imap::{ImapConnectionConfig, LiveImapSmtpGateway, SmtpConnectionConfig};
use posthaste_testkit::{GmailImapFixture, Harness};

/// Save an `ImapSmtp` account against the fixture, connect a real
/// [`LiveImapSmtpGateway`] to it, and run the initial full-snapshot sync
/// (which also lands the fixture's mailboxes). Returns the connected gateway.
pub async fn connect_account(
    harness: &Harness,
    fixture: &GmailImapFixture,
    id: &str,
) -> LiveImapSmtpGateway {
    harness.save_account(id, id, AccountDriver::ImapSmtp, fixture.imap_transport());
    let gateway = LiveImapSmtpGateway::connect(
        ImapConnectionConfig {
            host: "127.0.0.1".to_string(),
            port: fixture.port(),
            security: TransportSecurity::Plain,
            username: fixture.username(),
            secret: fixture.password(),
            auth: ProviderAuthKind::Password,
        },
        SmtpConnectionConfig {
            host: "127.0.0.1".to_string(),
            port: fixture.smtp_port(),
            security: TransportSecurity::Plain,
            sender_name: Some("Gmail Dev".to_string()),
            sender_email: fixture.username(),
            username: fixture.username(),
            secret: fixture.password(),
            auth: ProviderAuthKind::Password,
            provider: fixture.provider(),
        },
        Some(harness.store.clone()),
        Arc::new(StaticSecretResolver::new(fixture.password())),
    )
    .await
    .expect("IMAP gateway should connect to the mock");
    sync(harness, &AccountId::from(id), &gateway).await;
    gateway
}

/// One explicit sync cycle through the live gateway.
pub async fn sync(harness: &Harness, account: &AccountId, gateway: &LiveImapSmtpGateway) {
    harness
        .service
        .sync_account(account, SyncTrigger::Manual, gateway, None)
        .await
        .expect("sync should succeed");
}

/// Flush the outbox through the live gateway and assert every operation
/// settled applied — the direct-drive equivalent of the retired runtime
/// settlement's `assert_confirmed` (nothing pending, failed, or parked
/// afterwards).
pub async fn flush_settled(harness: &Harness, account: &AccountId, gateway: &LiveImapSmtpGateway) {
    harness
        .service
        .flush_account(account, gateway)
        .await
        .expect("flush should succeed");
    let pending = harness
        .service
        .list_pending_operations(account)
        .expect("pending ops should list");
    assert!(
        pending.is_empty(),
        "every flushed operation must settle applied, not linger: {:?}",
        pending
            .iter()
            .map(|op| (op.kind, op.state, op.last_error.clone()))
            .collect::<Vec<_>>()
    );
}

/// Resolve the fixture-served mailbox's synced `MailboxId` by its IMAP name.
/// A live-synced mailbox's id is namespaced (`imap:mailbox:<hex>`), never the
/// bare name, so lookups must go through the projection.
pub fn mailbox_id_by_name(harness: &Harness, account: &AccountId, name: &str) -> MailboxId {
    harness
        .service
        .list_mailboxes(account)
        .expect("mailboxes should list")
        .into_iter()
        .find(|m| m.name.eq_ignore_ascii_case(name))
        .unwrap_or_else(|| panic!("mailbox {name} should be discovered"))
        .id
}

/// The messages currently projected into `mailbox` for `account`.
pub fn messages_in(
    harness: &Harness,
    account: &AccountId,
    mailbox: &MailboxId,
) -> Vec<MessageSummary> {
    harness
        .service
        .list_messages(account, None)
        .expect("messages should list")
        .into_iter()
        .filter(|m| m.mailbox_ids.contains(mailbox))
        .collect()
}

/// The synced seeded message's id, read from the INBOX projection's single row.
pub fn seeded_message_id(
    harness: &Harness,
    account: &AccountId,
    inbox: &MailboxId,
) -> posthaste_domain_model::MessageId {
    let rows = messages_in(harness, account, inbox);
    assert_eq!(rows.len(), 1, "the seeded message should be in INBOX");
    rows[0].id.clone()
}
