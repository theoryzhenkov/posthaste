//! The `message.list_unsubscribe` column: persisted at sync-apply, surfaced on
//! the detail projection only, kept through target-less re-applies (COALESCE
//! guard), and backfilled by the lazy body fetch for pre-column rows.

use posthaste_domain_model::{FetchedBody, ListUnsubscribe};

use super::*;

fn sample_targets() -> ListUnsubscribe {
    ListUnsubscribe {
        https: Some("https://news.example.test/unsub/opaque".to_string()),
        mailto: Some("mailto:unsub@example.test?subject=stop".to_string()),
        one_click: true,
    }
}

#[test]
fn list_unsubscribe_roundtrips_into_the_detail_projection() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let mut message = metadata_only_message("newsletter-1", "inbox");
    message.list_unsubscribe = Some(sample_targets());
    let plain = metadata_only_message("plain-1", "inbox");
    seed_messages(&store, &account, vec![message, plain], "state-1")?;

    let detail = store
        .get_message_detail(&account, &MessageId::from("newsletter-1"))?
        .expect("detail");
    assert_eq!(detail.list_unsubscribe, Some(sample_targets()));
    // The body-free detail read (the `/messages/{id}` surface) carries it too.
    let header_detail = store
        .get_message_detail_without_body(&account, &MessageId::from("newsletter-1"))?
        .expect("detail without body");
    assert_eq!(header_detail.list_unsubscribe, Some(sample_targets()));

    // A message without the headers stays clean.
    let plain_detail = store
        .get_message_detail(&account, &MessageId::from("plain-1"))?
        .expect("plain detail");
    assert_eq!(plain_detail.list_unsubscribe, None);
    Ok(())
}

#[test]
fn target_less_reapply_does_not_clobber_stored_targets() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let mut message = metadata_only_message("newsletter-1", "inbox");
    message.list_unsubscribe = Some(sample_targets());
    seed_messages(&store, &account, vec![message], "state-1")?;

    // A later re-apply of the same row without header data (e.g. a flag-only
    // delta from a path that did not carry the headers) must not erase the
    // parsed targets.
    let mut reapplied = metadata_only_message("newsletter-1", "inbox");
    reapplied.keywords = vec!["$seen".to_string(), "$flagged".to_string()];
    reapplied.list_unsubscribe = None;
    seed_messages(&store, &account, vec![reapplied], "state-2")?;

    let detail = store
        .get_message_detail(&account, &MessageId::from("newsletter-1"))?
        .expect("detail");
    assert_eq!(detail.list_unsubscribe, Some(sample_targets()));
    assert!(detail.summary.is_flagged, "re-apply still lands");
    Ok(())
}

#[test]
fn body_fetch_backfills_targets_for_pre_column_rows() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("old-newsletter-1");
    setup_source(&store, &account, "Primary")?;
    // Simulates a row synced before the column existed: no targets stored.
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
            list_unsubscribe: Some(sample_targets()),
        },
    )?;

    // The command result's detail (served straight back to the opener) and a
    // fresh read both carry the backfilled targets.
    assert_eq!(
        result.detail.expect("detail").list_unsubscribe,
        Some(sample_targets())
    );
    let detail = store
        .get_message_detail(&account, &message_id)?
        .expect("detail");
    assert_eq!(detail.list_unsubscribe, Some(sample_targets()));
    Ok(())
}
