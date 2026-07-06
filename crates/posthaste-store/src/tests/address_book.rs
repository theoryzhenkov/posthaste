//! The persistent address book: complete (senders *and* recipients), ranked,
//! uncapped, backfilled from existing mail, and maintained on ingest.

use super::*;
use crate::test_support::TempDirGuard;

fn message_with(
    id: &str,
    from: Option<(&str, &str)>,
    to: Vec<Recipient>,
    received_at: &str,
) -> MessageRecord {
    MessageRecord {
        id: MessageId::from(id),
        source_thread_id: ThreadId::from(id),
        subject: Some("Subject".to_string()),
        from_name: from.map(|(name, _)| name.to_string()),
        from_email: from.map(|(_, email)| email.to_string()),
        to,
        preview: Some("preview".to_string()),
        received_at: received_at.to_string(),
        size: 10,
        mailbox_ids: vec![MailboxId::from("inbox")],
        keywords: vec!["$seen".to_string()],
        rfc_message_id: Some(format!("<{id}@example.test>")),
        ..Default::default()
    }
}

fn recipient(name: Option<&str>, email: &str) -> Recipient {
    Recipient {
        name: name.map(str::to_string),
        email: email.to_string(),
    }
}

fn open_seeded(account: &AccountId, messages: Vec<MessageRecord>) -> (TempDirGuard, DatabaseStore) {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))
        .expect("store should open");
    setup_source(&store, account, "Primary").expect("source");
    seed_messages(&store, account, messages, "cursor-1").expect("seed");
    (root, store)
}

/// The book harvests from BOTH the sender (`from`) and every recipient (`to`),
/// across many messages, with NO 40-cap: seed 60+ distinct correspondents and
/// assert every one is present.
#[test]
fn ingest_harvests_senders_and_recipients_with_no_cap() {
    let account = AccountId::from("primary");
    // 40 messages: each contributes a distinct sender AND a distinct recipient
    // => 80 distinct correspondents, well past the retired 40-slot cap.
    let messages: Vec<MessageRecord> = (0..40)
        .map(|index| {
            message_with(
                &format!("m{index}"),
                Some((
                    &format!("Sender {index}"),
                    &format!("sender{index}@example.test"),
                )),
                vec![recipient(
                    Some(&format!("Recipient {index}")),
                    &format!("recipient{index}@example.test"),
                )],
                "2026-03-31T10:00:00Z",
            )
        })
        .collect();
    let (_root, store) = open_seeded(&account, messages);

    let book = store.list_sender_address_cache().expect("list");
    assert!(
        book.len() >= 80,
        "expected the uncapped book to hold every correspondent, got {}",
        book.len()
    );
    for index in 0..40 {
        assert!(
            book.iter()
                .any(|entry| entry.email == format!("sender{index}@example.test")),
            "sender {index} missing from book"
        );
        assert!(
            book.iter()
                .any(|entry| entry.email == format!("recipient{index}@example.test")),
            "recipient {index} missing from book"
        );
    }
}

/// The book is ranked by frequency (occurrence count) first, then recency.
#[test]
fn book_is_ranked_by_frequency_then_recency() {
    let account = AccountId::from("primary");
    let mut messages = Vec::new();
    // `frequent@` appears in five messages; `rare@` in one; `recent@` in one but
    // more recently than `rare@`.
    for index in 0..5 {
        messages.push(message_with(
            &format!("freq{index}"),
            Some(("Frequent", "frequent@example.test")),
            vec![recipient(
                Some("Other"),
                &format!("other{index}@example.test"),
            )],
            "2026-01-01T00:00:00Z",
        ));
    }
    messages.push(message_with(
        "rare",
        Some(("Rare", "rare@example.test")),
        vec![],
        "2026-02-01T00:00:00Z",
    ));
    messages.push(message_with(
        "recent",
        Some(("Recent", "recent@example.test")),
        vec![],
        "2026-05-01T00:00:00Z",
    ));
    let (_root, store) = open_seeded(&account, messages);

    let book = store.list_sender_address_cache().expect("list");
    assert_eq!(
        book.first().map(|entry| entry.email.as_str()),
        Some("frequent@example.test"),
        "most frequent correspondent should rank first"
    );
    let recent_pos = book
        .iter()
        .position(|entry| entry.email == "recent@example.test")
        .expect("recent present");
    let rare_pos = book
        .iter()
        .position(|entry| entry.email == "rare@example.test")
        .expect("rare present");
    assert!(
        recent_pos < rare_pos,
        "at equal frequency the more recent correspondent should rank higher"
    );
}

/// The one-time backfill populates the book from messages already in the store
/// (proven by wiping the ingest-maintained rows first, then backfilling).
#[test]
fn backfill_repopulates_book_from_existing_messages() {
    let account = AccountId::from("primary");
    let messages: Vec<MessageRecord> = (0..60)
        .map(|index| {
            message_with(
                &format!("m{index}"),
                Some((
                    &format!("Sender {index}"),
                    &format!("person{index}@example.test"),
                )),
                vec![recipient(None, &format!("peer{index}@example.test"))],
                "2026-03-31T10:00:00Z",
            )
        })
        .collect();
    let (_root, store) = open_seeded(&account, messages);

    // Simulate a mailbox that predates the address book: clear the rows ingest
    // maintained, leaving only the `message` table populated.
    store
        .write_transaction(|tx| {
            tx.execute("DELETE FROM address_book", [])
                .map_err(sql_to_store_error)?;
            Ok(())
        })
        .expect("wipe");
    assert!(store.list_sender_address_cache().expect("list").is_empty());

    store.backfill_address_book().expect("backfill");

    let book = store.list_sender_address_cache().expect("list");
    assert!(book.len() >= 120, "backfill should recover the full book");
    for index in 0..60 {
        assert!(book
            .iter()
            .any(|entry| entry.email == format!("person{index}@example.test")));
        assert!(book
            .iter()
            .any(|entry| entry.email == format!("peer{index}@example.test")));
    }
}

/// Backfill is idempotent and consistent with ingest: re-running it never
/// inflates the frequency ingest already accumulated.
#[test]
fn backfill_is_idempotent_and_consistent_with_ingest() {
    let account = AccountId::from("primary");
    let messages: Vec<MessageRecord> = (0..3)
        .map(|index| {
            message_with(
                &format!("m{index}"),
                Some(("Repeat", "repeat@example.test")),
                vec![],
                "2026-03-31T10:00:00Z",
            )
        })
        .collect();
    let (_root, store) = open_seeded(&account, messages);

    let frequency = |store: &DatabaseStore| -> i64 {
        let connection = store.read_connection().expect("read");
        connection
            .query_row(
                "SELECT frequency FROM address_book WHERE normalized_email = ?1",
                params!["repeat@example.test"],
                |row| row.get::<_, i64>(0),
            )
            .expect("row")
    };

    assert_eq!(frequency(&store), 3, "ingest counts each message once");
    store.backfill_address_book().expect("backfill once");
    assert_eq!(
        frequency(&store),
        3,
        "backfill matches, does not double-count"
    );
    store.backfill_address_book().expect("backfill twice");
    assert_eq!(frequency(&store), 3, "backfill is idempotent");
}

/// Junk addresses (wildcards, whitespace, malformed local@domain) are filtered
/// from both the ingest harvest and the backfill.
#[test]
fn junk_addresses_are_filtered() {
    let account = AccountId::from("primary");
    let messages = vec![message_with(
        "m0",
        Some(("Wild", "*@example.test")),
        vec![
            recipient(Some("Spaced"), "a b@example.test"),
            recipient(None, "@example.test"),
            recipient(None, "no-at-sign"),
            recipient(Some("Real"), "real@example.test"),
        ],
        "2026-03-31T10:00:00Z",
    )];
    let (_root, store) = open_seeded(&account, messages);

    let book = store.list_sender_address_cache().expect("list");
    assert_eq!(
        book.iter()
            .map(|entry| entry.email.as_str())
            .collect::<Vec<_>>(),
        vec!["real@example.test"],
        "only the valid recipient should survive the filter"
    );
}

/// A re-applied message (a flag update re-running the ingest path) does not
/// double-count its correspondents.
#[test]
fn reapplying_a_message_does_not_double_count() {
    let account = AccountId::from("primary");
    let message = message_with(
        "m0",
        Some(("Solo", "solo@example.test")),
        vec![],
        "2026-03-31T10:00:00Z",
    );
    let (_root, store) = open_seeded(&account, vec![message.clone()]);

    // Re-ingest the same message (as a later sync would for a flag change).
    let mut updated = message;
    updated.keywords = vec!["$seen".to_string(), "$flagged".to_string()];
    seed_messages(&store, &account, vec![updated], "cursor-2").expect("re-seed");

    let connection = store.read_connection().expect("read");
    let frequency: i64 = connection
        .query_row(
            "SELECT frequency FROM address_book WHERE normalized_email = ?1",
            params!["solo@example.test"],
            |row| row.get(0),
        )
        .expect("row");
    assert_eq!(frequency, 1, "re-apply must not re-count the correspondent");
}
