use std::collections::BTreeSet;
use std::sync::Arc;

use posthaste_domain_model::{
    AccountDriver, AccountId, ProviderAuthKind, ProviderHint, Recipient, ReplaceMailboxesCommand,
    SendMessageRequest, SetKeywordsCommand, TransportSecurity,
};
use posthaste_domain_service::StaticSecretResolver;
use posthaste_engine::LiveJmapGateway;
use posthaste_imap::{ImapConnectionConfig, LiveImapSmtpGateway, SmtpConnectionConfig};

use posthaste_testkit::{Harness, StalwartFixture};

use crate::helpers::*;

#[tokio::test]
// spec: docs/testing/L1#real-provider-parity
async fn stalwart_jmap_and_imap_sync_project_equivalent_fixture_messages() {
    if std::env::var("POSTHASTE_STALWART_INTEGRATION").as_deref() != Ok("1") {
        eprintln!("skipping Stalwart integration; set POSTHASTE_STALWART_INTEGRATION=1");
        return;
    }

    let stalwart = StalwartFixture::start();
    let harness = Harness::new();
    harness.save_account(
        "jmap-stalwart",
        "Stalwart JMAP",
        AccountDriver::Jmap,
        stalwart.jmap_transport(),
    );
    harness.save_account(
        "imap-stalwart",
        "Stalwart IMAP",
        AccountDriver::ImapSmtp,
        stalwart.imap_transport(),
    );
    let jmap_gateway =
        LiveJmapGateway::connect(&stalwart.http_url, Some("dev"), &stalwart.password)
            .await
            .expect("JMAP gateway should connect");
    let imap_gateway = LiveImapSmtpGateway::connect(
        ImapConnectionConfig {
            host: "127.0.0.1".to_string(),
            port: stalwart.imap_port,
            security: TransportSecurity::Plain,
            username: "dev".to_string(),
            secret: stalwart.password.clone(),
            auth: ProviderAuthKind::Password,
        },
        SmtpConnectionConfig {
            host: "127.0.0.1".to_string(),
            port: stalwart.smtp_port,
            security: TransportSecurity::Plain,
            sender_name: Some("Dev Account".to_string()),
            sender_email: stalwart.email(),
            username: "dev".to_string(),
            secret: stalwart.password.clone(),
            auth: ProviderAuthKind::Password,
            provider: ProviderHint::Generic,
        },
        Some(harness.store.clone()),
        Arc::new(StaticSecretResolver::new(stalwart.password.clone())),
    )
    .await
    .expect("IMAP gateway should connect");

    sync_pair(&harness, &jmap_gateway, &imap_gateway).await;

    let jmap_messages = normalized_messages(&harness, "jmap-stalwart");
    let imap_messages = normalized_messages(&harness, "imap-stalwart");

    // Cross-provider projections must match, modulo the ONE tracked `\Deleted`
    // Trash divergence (QUARANTINED_TRASH_SUBJECT). Assert that divergence
    // precisely so a future convergence trips here and forces removing the
    // quarantine; this runs before the message is deleted via JMAP below.
    assert!(
        maybe_message_by_subject(&harness, "jmap-stalwart", QUARANTINED_TRASH_SUBJECT).is_some(),
        "JMAP Email/query should surface the Trash fixture message"
    );
    assert!(
        maybe_message_by_subject(&harness, "imap-stalwart", QUARANTINED_TRASH_SUBJECT).is_none(),
        "IMAP UID SEARCH UNDELETED should hide the \\Deleted Trash fixture message; \
         if this fires the projection converged — drop QUARANTINED_TRASH_SUBJECT"
    );
    assert_eq!(
        cross_provider_messages(&harness, "jmap-stalwart"),
        cross_provider_messages(&harness, "imap-stalwart"),
    );
    assert!(
        jmap_messages.len() >= 8,
        "fixture should contain enough messages to exercise multiple mailbox roles"
    );

    let initial_imap_location_count = imap_location_count(&harness);
    sync_pair(&harness, &jmap_gateway, &imap_gateway).await;
    assert_eq!(
        jmap_messages,
        normalized_messages(&harness, "jmap-stalwart")
    );
    assert_eq!(
        imap_messages,
        normalized_messages(&harness, "imap-stalwart")
    );
    assert_eq!(initial_imap_location_count, imap_location_count(&harness));

    let target = jmap_message_by_subject(
        &harness,
        "jmap-stalwart",
        "Welcome to the Posthaste sandbox",
    );
    harness
        .service
        .set_keywords(
            &AccountId::from("jmap-stalwart"),
            &target.id,
            &SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: vec!["$seen".to_string()],
            },
        )
        .await
        .expect("JMAP flag mutation should apply locally");
    harness
        .service
        .flush_account(&AccountId::from("jmap-stalwart"), &jmap_gateway)
        .await
        .expect("JMAP flag mutation should flush");
    sync_pair(&harness, &jmap_gateway, &imap_gateway).await;

    let jmap_target = message_by_subject(
        &harness,
        "jmap-stalwart",
        "Welcome to the Posthaste sandbox",
    );
    let imap_target = message_by_subject(
        &harness,
        "imap-stalwart",
        "Welcome to the Posthaste sandbox",
    );
    assert!(!jmap_target.is_read);
    assert!(jmap_target.is_flagged);
    assert_eq!(jmap_target.is_read, imap_target.is_read);
    assert_eq!(jmap_target.is_flagged, imap_target.is_flagged);
    assert_eq!(
        cross_provider_messages(&harness, "jmap-stalwart"),
        cross_provider_messages(&harness, "imap-stalwart")
    );

    let archive_id = mailbox_id_by_label(&harness, "jmap-stalwart", "archive");
    harness
        .service
        .replace_mailboxes(
            &AccountId::from("jmap-stalwart"),
            &jmap_target.id,
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![archive_id],
            },
        )
        .await
        .expect("JMAP mailbox move should apply locally");
    harness
        .service
        .flush_account(&AccountId::from("jmap-stalwart"), &jmap_gateway)
        .await
        .expect("JMAP mailbox move should flush");
    sync_pair(&harness, &jmap_gateway, &imap_gateway).await;

    let jmap_labels = mailbox_labels_for_subject(
        &harness,
        "jmap-stalwart",
        "Welcome to the Posthaste sandbox",
    );
    let imap_labels = mailbox_labels_for_subject(
        &harness,
        "imap-stalwart",
        "Welcome to the Posthaste sandbox",
    );
    assert_eq!(jmap_labels, BTreeSet::from(["archive".to_string()]));
    assert_eq!(jmap_labels, imap_labels);
    assert_eq!(
        cross_provider_messages(&harness, "jmap-stalwart"),
        cross_provider_messages(&harness, "imap-stalwart")
    );

    let imap_move_target = message_by_subject(&harness, "imap-stalwart", "Invoice 2048 attached");
    let imap_archive_id = mailbox_id_by_label(&harness, "imap-stalwart", "archive");
    harness
        .service
        .replace_mailboxes(
            &AccountId::from("imap-stalwart"),
            &imap_move_target.id,
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![imap_archive_id],
            },
        )
        .await
        .expect("IMAP mailbox move should apply locally");
    harness
        .service
        .flush_account(&AccountId::from("imap-stalwart"), &imap_gateway)
        .await
        .expect("IMAP mailbox move should flush");
    sync_pair(&harness, &jmap_gateway, &imap_gateway).await;

    let jmap_imap_move_labels =
        mailbox_labels_for_subject(&harness, "jmap-stalwart", "Invoice 2048 attached");
    let imap_move_labels =
        mailbox_labels_for_subject(&harness, "imap-stalwart", "Invoice 2048 attached");
    assert_eq!(
        jmap_imap_move_labels,
        BTreeSet::from(["archive".to_string()])
    );
    assert_eq!(jmap_imap_move_labels, imap_move_labels);
    assert_eq!(
        cross_provider_messages(&harness, "jmap-stalwart"),
        cross_provider_messages(&harness, "imap-stalwart")
    );

    let deleted = jmap_message_by_subject(
        &harness,
        "jmap-stalwart",
        "Build failure on obsolete branch",
    );
    harness
        .service
        .destroy_message(&AccountId::from("jmap-stalwart"), &deleted.id)
        .await
        .expect("JMAP delete should apply locally");
    harness
        .service
        .flush_account(&AccountId::from("jmap-stalwart"), &jmap_gateway)
        .await
        .expect("JMAP delete should flush");
    sync_pair(&harness, &jmap_gateway, &imap_gateway).await;

    assert!(maybe_message_by_subject(
        &harness,
        "jmap-stalwart",
        "Build failure on obsolete branch"
    )
    .is_none());
    assert!(maybe_message_by_subject(
        &harness,
        "imap-stalwart",
        "Build failure on obsolete branch"
    )
    .is_none());
    assert_eq!(
        cross_provider_messages(&harness, "jmap-stalwart"),
        cross_provider_messages(&harness, "imap-stalwart")
    );

    harness
        .service
        .enqueue_send(
            &AccountId::from("jmap-stalwart"),
            SendMessageRequest {
                from: Some(Recipient {
                    name: Some("Dev Account".to_string()),
                    email: stalwart.email(),
                }),
                to: vec![Recipient {
                    name: Some("Dev Account".to_string()),
                    email: stalwart.email(),
                }],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "JMAP parity self-send".to_string(),
                body: "Sent through the JMAP gateway.".to_string(),
                ..Default::default()
            },
        )
        .await
        .expect("JMAP send should queue");
    harness
        .service
        .flush_account(&AccountId::from("jmap-stalwart"), &jmap_gateway)
        .await
        .expect("JMAP send should flush");
    // Settle the JMAP self-send: the outgoing copy must file into Sent (the
    // onSuccessUpdateEmail Drafts→Sent move) and leave nothing in Drafts. The
    // same-server inbound delivery carries the same deterministic RFC5322
    // Message-ID and dedups into that Sent copy, so — exactly as in
    // stalwart_send_regression — the JMAP self-send yields one copy in Sent with
    // no separate Inbox copy to assert here (the SMTP send below covers inbox).
    sync_pair_until(&harness, &jmap_gateway, &imap_gateway, |harness| {
        mailbox_roles_across_copies(harness, "imap-stalwart", "JMAP parity self-send")
            .contains("sent")
    })
    .await;

    assert_eq!(
        cross_provider_messages(&harness, "jmap-stalwart"),
        cross_provider_messages(&harness, "imap-stalwart")
    );
    let jmap_send_roles =
        mailbox_roles_across_copies(&harness, "imap-stalwart", "JMAP parity self-send");
    assert!(
        jmap_send_roles.contains("sent"),
        "JMAP self-send outgoing copy must land in Sent (was: {jmap_send_roles:?})"
    );
    assert!(
        !jmap_send_roles.contains("drafts"),
        "JMAP self-send must NOT linger in Drafts — the onSuccessUpdateEmail \
         Drafts→Sent move must apply (was: {jmap_send_roles:?})"
    );

    harness
        .service
        .enqueue_send(
            &AccountId::from("imap-stalwart"),
            SendMessageRequest {
                from: Some(Recipient {
                    name: Some("Dev Account".to_string()),
                    email: stalwart.email(),
                }),
                to: vec![Recipient {
                    name: Some("Dev Account".to_string()),
                    email: stalwart.email(),
                }],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "SMTP parity self-send".to_string(),
                body: "Sent through the IMAP/SMTP gateway.".to_string(),
                ..Default::default()
            },
        )
        .await
        .expect("SMTP send should queue");
    harness
        .service
        .flush_account(&AccountId::from("imap-stalwart"), &imap_gateway)
        .await
        .expect("SMTP send should flush");
    // Settle the SMTP self-send: the SMTP path appends the outgoing copy to Sent
    // AND separately delivers a same-server copy to Inbox (no dedup on this
    // path), so a healthy self-send yields BOTH roles and never Drafts. This is
    // where inbox delivery is asserted separately for the self-send case.
    sync_pair_until(&harness, &jmap_gateway, &imap_gateway, |harness| {
        let roles = mailbox_roles_across_copies(harness, "imap-stalwart", "SMTP parity self-send");
        roles.contains("sent") && roles.contains("inbox")
    })
    .await;

    assert_eq!(
        cross_provider_messages(&harness, "jmap-stalwart"),
        cross_provider_messages(&harness, "imap-stalwart")
    );
    assert!(
        maybe_message_by_subject(&harness, "jmap-stalwart", "SMTP parity self-send").is_some(),
        "SMTP self-send should be visible through JMAP after sync"
    );
    let smtp_send_roles =
        mailbox_roles_across_copies(&harness, "imap-stalwart", "SMTP parity self-send");
    assert!(
        smtp_send_roles.contains("sent"),
        "SMTP self-send outgoing copy must land in Sent (was: {smtp_send_roles:?})"
    );
    assert!(
        !smtp_send_roles.contains("drafts"),
        "SMTP self-send must NOT linger in Drafts (was: {smtp_send_roles:?})"
    );
    assert!(
        smtp_send_roles.contains("inbox"),
        "SMTP self-send must ALSO deliver a separate Inbox copy for the \
         self-send case (was: {smtp_send_roles:?})"
    );
}
