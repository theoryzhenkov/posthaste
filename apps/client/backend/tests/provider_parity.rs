#[path = "provider_parity/fixtures.rs"]
mod fixtures;
#[path = "provider_parity/imap_batches.rs"]
mod imap_batches;
#[path = "provider_parity/jmap_batches.rs"]
mod jmap_batches;
#[path = "provider_parity/support.rs"]
mod support;

use posthaste_domain_model::{AccountDriver, AccountId, MessageId, SyncTrigger};
use posthaste_imap::imap_body_from_raw_mime;

use fixtures::{empty_body, parity_attachment_blob, parity_body, parity_raw_mime};
use imap_batches::{
    imap_gmail_flagged_delta_batch, imap_gmail_label_sync_batch, imap_single_label_vanished_batch,
    imap_sync_batch,
};
use jmap_batches::{jmap_label_initial_batch, jmap_label_removed_batch, jmap_sync_batch};
use support::{
    imap_location_count_for_subject, maybe_mailbox_roles_for_subject, message_by_subject, Harness,
    StaticGateway,
};

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
            &MessageId::from("jmap-message-1"),
            &jmap_detail.attachments[0].blob_id,
            &jmap_gateway,
        )
        .await
        .expect("JMAP blob should download");
    let imap_blob = harness
        .service
        .download_blob(
            &AccountId::from("imap"),
            &imap_message_id,
            &imap_detail.attachments[0].blob_id,
            &imap_gateway,
        )
        .await
        .expect("IMAP blob should download");
    assert_eq!(jmap_blob, imap_blob);
}

// spec: docs/L0-testing#provider-observation-contracts
#[tokio::test]
async fn imap_single_label_vanish_converges_like_jmap_mailbox_removal() {
    let harness = Harness::new();
    harness.save_account("jmap-labels", "JMAP Labels", AccountDriver::Jmap);
    harness.save_account("imap-labels", "IMAP Labels", AccountDriver::ImapSmtp);
    let imap_initial = imap_gmail_label_sync_batch();
    let imap_locations = imap_initial.imap_message_locations.clone();
    let jmap_gateway = StaticGateway::from_batches(
        vec![jmap_label_initial_batch(), jmap_label_removed_batch()],
        empty_body(),
        Vec::new(),
    );
    let imap_gateway = StaticGateway::from_batches(
        vec![
            imap_initial,
            imap_single_label_vanished_batch(imap_locations),
        ],
        empty_body(),
        Vec::new(),
    );

    harness
        .service
        .sync_account(
            &AccountId::from("jmap-labels"),
            SyncTrigger::Manual,
            &jmap_gateway,
            None,
        )
        .await
        .expect("initial JMAP sync should apply");
    harness
        .service
        .sync_account(
            &AccountId::from("imap-labels"),
            SyncTrigger::Manual,
            &imap_gateway,
            None,
        )
        .await
        .expect("initial IMAP sync should apply");

    harness
        .service
        .sync_account(
            &AccountId::from("jmap-labels"),
            SyncTrigger::Manual,
            &jmap_gateway,
            None,
        )
        .await
        .expect("JMAP mailbox removal should apply");
    harness
        .service
        .sync_account(
            &AccountId::from("imap-labels"),
            SyncTrigger::Manual,
            &imap_gateway,
            None,
        )
        .await
        .expect("IMAP single-label vanish should apply");

    assert_eq!(
        maybe_mailbox_roles_for_subject(&harness, "jmap-labels", "Label parity"),
        Some(vec!["archive".to_string()])
    );
    assert_eq!(
        maybe_mailbox_roles_for_subject(&harness, "imap-labels", "Label parity"),
        Some(vec!["archive".to_string()])
    );
    assert_eq!(
        imap_location_count_for_subject(&harness, "imap-labels", "Label parity"),
        1
    );
}

// spec: docs/L0-testing#provider-observation-contracts
#[tokio::test]
async fn imap_flag_delta_updates_keywords_without_losing_existing_mailboxes() {
    let harness = Harness::new();
    harness.save_account("imap-flags", "IMAP Flags", AccountDriver::ImapSmtp);
    let initial = imap_gmail_label_sync_batch();
    let locations = initial.imap_message_locations.clone();
    let gateway = StaticGateway::from_batches(
        vec![initial, imap_gmail_flagged_delta_batch(locations)],
        empty_body(),
        Vec::new(),
    );

    harness
        .service
        .sync_account(
            &AccountId::from("imap-flags"),
            SyncTrigger::Manual,
            &gateway,
            None,
        )
        .await
        .expect("initial IMAP sync should apply");
    harness
        .service
        .sync_account(
            &AccountId::from("imap-flags"),
            SyncTrigger::Manual,
            &gateway,
            None,
        )
        .await
        .expect("IMAP flag delta should apply");

    let message = message_by_subject(&harness, "imap-flags", "Label parity");
    assert!(message.is_flagged);
    assert_eq!(
        maybe_mailbox_roles_for_subject(&harness, "imap-flags", "Label parity"),
        Some(vec!["archive".to_string(), "inbox".to_string()])
    );
    assert_eq!(
        imap_location_count_for_subject(&harness, "imap-flags", "Label parity"),
        2
    );
}
