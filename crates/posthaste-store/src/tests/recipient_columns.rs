//! The `cc_json` / `bcc_json` / `reply_to_json` columns: persisted at
//! sync-apply, projected onto the SUMMARY (so list rows and the detail pane
//! read one field set), kept through recipient-less re-applies, and backfilled
//! by the lazy body fetch for rows synced before the columns existed.

use posthaste_domain_model::{FetchedBody, Recipient};

use super::*;

fn recipient(email: &str) -> Recipient {
    Recipient {
        name: Some(email.split('@').next().unwrap_or(email).to_string()),
        email: email.to_string(),
    }
}

fn with_recipients(message_id: &str) -> MessageRecord {
    let mut message = metadata_only_message(message_id, "inbox");
    message.cc = vec![recipient("cc@example.test")];
    message.bcc = vec![recipient("bcc@example.test")];
    message.reply_to = vec![recipient("replies@example.test")];
    message
}

#[test]
fn recipients_reach_the_list_row_and_the_detail() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![
            with_recipients("full-1"),
            metadata_only_message("plain-1", "inbox"),
        ],
        "state-1",
    )?;

    // Both surfaces project the same field set — the point of putting these on
    // the summary rather than the detail.
    let rows = store.list_messages(&account, Some(&MailboxId::from("inbox")))?;
    let full = rows
        .iter()
        .find(|row| row.id.as_str() == "full-1")
        .expect("list row");
    assert_eq!(full.cc, vec![recipient("cc@example.test")]);
    assert_eq!(full.bcc, vec![recipient("bcc@example.test")]);
    assert_eq!(full.reply_to, vec![recipient("replies@example.test")]);

    let detail = store
        .get_message_detail(&account, &MessageId::from("full-1"))?
        .expect("detail");
    assert_eq!(detail.summary.cc, full.cc);
    assert_eq!(detail.summary.bcc, full.bcc);
    assert_eq!(detail.summary.reply_to, full.reply_to);

    // A message with none of the headers reads as empty everywhere — the state
    // the registry renders as a non-render, and the normal case for real mail.
    let plain = rows
        .iter()
        .find(|row| row.id.as_str() == "plain-1")
        .expect("plain list row");
    assert!(plain.cc.is_empty() && plain.bcc.is_empty() && plain.reply_to.is_empty());
    let plain_detail = store
        .get_message_detail(&account, &MessageId::from("plain-1"))?
        .expect("plain detail");
    assert!(plain_detail.summary.cc.is_empty());
    Ok(())
}

#[test]
fn recipients_survive_a_reapply_that_carries_none() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(&store, &account, vec![with_recipients("full-1")], "state-1")?;

    // A later re-apply from a path that carried no header data (a flag-only
    // delta, say) must not blank the parsed recipients: these headers are
    // immutable per message, so an empty incoming value means "no data here",
    // never "the sender removed them".
    let mut reapplied = metadata_only_message("full-1", "inbox");
    reapplied.keywords = vec!["$seen".to_string(), "$flagged".to_string()];
    seed_messages(&store, &account, vec![reapplied], "state-2")?;

    let detail = store
        .get_message_detail(&account, &MessageId::from("full-1"))?
        .expect("detail");
    assert_eq!(detail.summary.cc, vec![recipient("cc@example.test")]);
    assert_eq!(detail.summary.bcc, vec![recipient("bcc@example.test")]);
    assert!(detail.summary.is_flagged, "the re-apply still lands");
    Ok(())
}

#[test]
fn body_fetch_backfills_recipients_for_pre_column_rows() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("old-1");
    setup_source(&store, &account, "Primary")?;
    // Stands in for a row synced before the columns existed: no recipients.
    seed_messages(
        &store,
        &account,
        vec![metadata_only_message(message_id.as_str(), "inbox")],
        "state-1",
    )?;

    let result = store.apply_message_body(
        &account,
        &message_id,
        &FetchedBody {
            body_html: None,
            body_text: Some("Hello".to_string()),
            raw_mime: None,
            attachments: Vec::new(),
            list_unsubscribe: None,
            cc: vec![recipient("cc@example.test")],
            bcc: Vec::new(),
            reply_to: vec![recipient("replies@example.test")],
        },
    )?;

    let detail = result.detail.expect("detail");
    assert_eq!(detail.summary.cc, vec![recipient("cc@example.test")]);
    assert_eq!(
        detail.summary.reply_to,
        vec![recipient("replies@example.test")]
    );
    // The fetch carried no Bcc, which is what a received message looks like:
    // the header is stripped in transit. Nothing to fill, nothing filled.
    assert!(detail.summary.bcc.is_empty());
    Ok(())
}

#[test]
fn a_body_fetch_without_recipients_cannot_blank_stored_ones() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("full-1");
    setup_source(&store, &account, "Primary")?;
    seed_messages(&store, &account, vec![with_recipients("full-1")], "state-1")?;

    store.apply_message_body(
        &account,
        &message_id,
        &FetchedBody {
            body_html: None,
            body_text: Some("Hello".to_string()),
            raw_mime: None,
            attachments: Vec::new(),
            list_unsubscribe: None,
            cc: Vec::new(),
            bcc: Vec::new(),
            reply_to: Vec::new(),
        },
    )?;

    let detail = store
        .get_message_detail(&account, &message_id)?
        .expect("detail");
    assert_eq!(detail.summary.cc, vec![recipient("cc@example.test")]);
    assert_eq!(detail.summary.bcc, vec![recipient("bcc@example.test")]);
    Ok(())
}
