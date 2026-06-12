use std::collections::BTreeSet;

use posthaste_domain::*;
use posthaste_engine::LiveJmapGateway;
use posthaste_imap::{ImapConnectionConfig, LiveImapSmtpGateway, SmtpConnectionConfig};

use crate::fixture::StalwartFixture;
use crate::harness::Harness;
use crate::helpers::*;

#[tokio::test]
// spec: docs/L0-providers#live-provider-parity
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
    )
    .await
    .expect("IMAP gateway should connect");

    sync_pair(&harness, &jmap_gateway, &imap_gateway).await;

    let jmap_messages = normalized_messages(&harness, "jmap-stalwart");
    let imap_messages = normalized_messages(&harness, "imap-stalwart");

    assert_eq!(jmap_messages, imap_messages);
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
            &jmap_gateway,
        )
        .await
        .expect("JMAP flag mutation should succeed");
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
        normalized_messages(&harness, "jmap-stalwart"),
        normalized_messages(&harness, "imap-stalwart")
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
            &jmap_gateway,
        )
        .await
        .expect("JMAP mailbox move should succeed");
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
        normalized_messages(&harness, "jmap-stalwart"),
        normalized_messages(&harness, "imap-stalwart")
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
            &imap_gateway,
        )
        .await
        .expect("IMAP mailbox move should succeed");
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
        normalized_messages(&harness, "jmap-stalwart"),
        normalized_messages(&harness, "imap-stalwart")
    );

    let deleted = jmap_message_by_subject(
        &harness,
        "jmap-stalwart",
        "Build failure on obsolete branch",
    );
    harness
        .service
        .destroy_message(
            &AccountId::from("jmap-stalwart"),
            &deleted.id,
            &jmap_gateway,
        )
        .await
        .expect("JMAP delete should succeed");
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
        normalized_messages(&harness, "jmap-stalwart"),
        normalized_messages(&harness, "imap-stalwart")
    );

    harness
        .service
        .send_message(
            &AccountId::from("jmap-stalwart"),
            &SendMessageRequest {
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
                in_reply_to: None,
                references: None,
                attachments: Vec::new(),
            },
            &jmap_gateway,
        )
        .await
        .expect("JMAP send should succeed");
    sync_pair(&harness, &jmap_gateway, &imap_gateway).await;

    assert_eq!(
        normalized_messages(&harness, "jmap-stalwart"),
        normalized_messages(&harness, "imap-stalwart")
    );
    let sent_labels =
        mailbox_labels_for_subject(&harness, "imap-stalwart", "JMAP parity self-send");
    assert!(
        sent_labels.contains("sent") || sent_labels.contains("inbox"),
        "self-send should be visible in sent or inbox after sync"
    );

    harness
        .service
        .send_message(
            &AccountId::from("imap-stalwart"),
            &SendMessageRequest {
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
                in_reply_to: None,
                references: None,
                attachments: Vec::new(),
            },
            &imap_gateway,
        )
        .await
        .expect("SMTP send should succeed");
    sync_pair(&harness, &jmap_gateway, &imap_gateway).await;

    assert_eq!(
        normalized_messages(&harness, "jmap-stalwart"),
        normalized_messages(&harness, "imap-stalwart")
    );
    assert!(
        maybe_message_by_subject(&harness, "jmap-stalwart", "SMTP parity self-send").is_some(),
        "SMTP self-send should be visible through JMAP after sync"
    );
    let smtp_sent_labels =
        mailbox_labels_for_subject(&harness, "imap-stalwart", "SMTP parity self-send");
    assert!(
        smtp_sent_labels.contains("sent") || smtp_sent_labels.contains("inbox"),
        "SMTP self-send should be visible in sent or inbox after sync"
    );
}
