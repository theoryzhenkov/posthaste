// NS1 (RFC-L2-client-replication-model D167/D168): the overlay plane's merge
// semantics through the STRANGLED read path (the smart-mailbox/rule-query
// family reads `*_effective`). Base is written only via `apply_sync_batch`
// here — never via the overlay — and vice versa, mirroring the one-writer
// contract the views encode. The existing suite passing untouched is the
// empty-overlay differential; these tests cover the merge itself.
use super::*;

fn keyword_rule(keyword: &str) -> MailQueryRule {
    all_rule(vec![rule_condition(
        MailQueryField::Keyword,
        MailQueryOperator::Equals,
        keyword,
    )])
}

fn subject_rule(fragment: &str) -> MailQueryRule {
    all_rule(vec![rule_condition(
        MailQueryField::Subject,
        MailQueryOperator::Contains,
        fragment,
    )])
}

fn page_ids(store: &DatabaseStore, rule: &MailQueryRule) -> Result<Vec<String>, StoreError> {
    let page = store.query_message_page_by_rule(
        rule,
        50,
        None,
        MessageSortField::Date,
        SortDirection::Desc,
    )?;
    Ok(page
        .items
        .into_iter()
        .map(|item| item.id.as_str().to_string())
        .collect())
}

fn overlay_store(root: &std::path::Path) -> DatabaseStore {
    DatabaseStore::open(root.join("mail.sqlite"), root.join("data")).unwrap()
}

fn seeded(store: &DatabaseStore, account: &AccountId) -> Result<(), StoreError> {
    setup_source(store, account, "Primary")?;
    seed_messages(
        store,
        account,
        vec![
            sample_message("message-1", "inbox", Some("mime-1")),
            sample_message("message-2", "inbox", Some("mime-2")),
        ],
        "seed-state",
    )
}

#[test]
fn overlay_row_overrides_base_for_rule_queries() -> Result<(), StoreError> {
    let root = temp_root();
    let store = overlay_store(&root);
    let account = AccountId::from("primary");
    seeded(&store, &account)?;

    // Fold output: message-1 marked unread ($seen removed) and moved to
    // archive. The base row still says read+inbox; the overlay must win on
    // every axis the strangled path reads.
    let mut folded = sample_message("message-1", "archive", None);
    folded.keywords = vec![];
    store.upsert_overlay_message(&account, &folded)?;

    // Keyword predicate: message-1 no longer matches $seen; message-2 does.
    assert_eq!(page_ids(&store, &keyword_rule("$seen"))?, vec!["message-2"]);

    // Membership predicate reads the overlay set.
    let archived = page_ids(
        &store,
        &all_rule(vec![rule_condition(
            MailQueryField::MailboxId,
            MailQueryOperator::Equals,
            "archive",
        )]),
    )?;
    assert_eq!(archived, vec!["message-1"]);

    // Hydration serves the folded sets, not base's.
    let page = store.query_message_page_by_rule(
        &subject_rule("Hello"),
        50,
        None,
        MessageSortField::Date,
        SortDirection::Desc,
    )?;
    let folded_row = page
        .items
        .iter()
        .find(|item| item.id.as_str() == "message-1")
        .expect("overlaid message still pages");
    assert_eq!(folded_row.mailbox_ids, vec![MailboxId::from("archive")]);
    assert!(folded_row.keywords.is_empty(), "keywords come from overlay");
    assert!(!folded_row.is_read, "is_read derives from folded keywords");

    // Counts see the fold too: one of two is unread.
    let (unread, total) = store.query_smart_mailbox_counts(&subject_rule("Hello"))?;
    assert_eq!((unread, total), (1, 2));
    Ok(())
}

#[test]
fn tombstone_hides_message_and_retire_restores_it() -> Result<(), StoreError> {
    let root = temp_root();
    let store = overlay_store(&root);
    let account = AccountId::from("primary");
    seeded(&store, &account)?;

    store.tombstone_overlay_message(&account, &MessageId::from("message-2"))?;
    assert_eq!(
        page_ids(&store, &subject_rule("Hello"))?,
        vec!["message-1"],
        "a tombstoned message is hidden from the effective read"
    );
    let (_, total) = store.query_smart_mailbox_counts(&subject_rule("Hello"))?;
    assert_eq!(total, 1);

    // Retire (op reverted or confirmed-and-synced): base shows through again,
    // with its original values.
    store.remove_overlay_message(&account, &MessageId::from("message-2"))?;
    assert_eq!(
        page_ids(&store, &subject_rule("Hello"))?,
        vec!["message-2", "message-1"],
        "retiring the overlay restores the base row"
    );
    assert_eq!(page_ids(&store, &keyword_rule("$seen"))?.len(), 2);
    Ok(())
}

#[test]
fn overlay_only_row_appears_before_any_sync() -> Result<(), StoreError> {
    let root = temp_root();
    let store = overlay_store(&root);
    let account = AccountId::from("primary");
    seeded(&store, &account)?;

    // A pending draft create: no base row exists yet.
    let mut draft = sample_message("draft-local-1", "drafts", None);
    draft.subject = Some("Unsent draft".to_string());
    draft.keywords = vec!["$draft".to_string()];
    store.upsert_overlay_message(&account, &draft)?;

    assert_eq!(
        page_ids(&store, &keyword_rule("$draft"))?,
        vec!["draft-local-1"],
        "an overlay-only row is visible to the strangled path"
    );
    let page = store.query_message_page_by_rule(
        &keyword_rule("$draft"),
        50,
        None,
        MessageSortField::Date,
        SortDirection::Desc,
    )?;
    assert_eq!(
        page.items[0].mailbox_ids,
        vec![MailboxId::from("drafts")],
        "membership hydrates from the overlay set"
    );

    // Retire without it ever reaching base (e.g. the create was reverted).
    store.remove_overlay_message(&account, &MessageId::from("draft-local-1"))?;
    assert!(page_ids(&store, &keyword_rule("$draft"))?.is_empty());
    Ok(())
}

#[test]
fn refold_upsert_replaces_prior_overlay_state() -> Result<(), StoreError> {
    let root = temp_root();
    let store = overlay_store(&root);
    let account = AccountId::from("primary");
    seeded(&store, &account)?;

    let mut first_fold = sample_message("message-1", "archive", None);
    first_fold.keywords = vec![];
    store.upsert_overlay_message(&account, &first_fold)?;

    // Refold (base changed under the pending effect, or a second op folded
    // in): the upsert fully replaces row and sets — no residue of the first.
    let second_fold = sample_message("message-1", "trash", None);
    store.upsert_overlay_message(&account, &second_fold)?;

    let page = store.query_message_page_by_rule(
        &subject_rule("Hello"),
        50,
        None,
        MessageSortField::Date,
        SortDirection::Desc,
    )?;
    let row = page
        .items
        .iter()
        .find(|item| item.id.as_str() == "message-1")
        .expect("refolded message pages");
    assert_eq!(row.mailbox_ids, vec![MailboxId::from("trash")]);
    assert_eq!(row.keywords, vec!["$seen".to_string()]);
    assert!(row.is_read, "is_read re-derived from the second fold");
    Ok(())
}

#[test]
fn tombstone_then_upsert_clears_the_tombstone() -> Result<(), StoreError> {
    let root = temp_root();
    let store = overlay_store(&root);
    let account = AccountId::from("primary");
    seeded(&store, &account)?;

    let target = MessageId::from("message-1");
    store.tombstone_overlay_message(&account, &target)?;
    assert_eq!(page_ids(&store, &subject_rule("Hello"))?.len(), 1);

    // The destroy reverted and a newer fold landed: the row must come back
    // with the folded values, not stay hidden.
    store.upsert_overlay_message(&account, &sample_message("message-1", "inbox", None))?;
    assert_eq!(page_ids(&store, &subject_rule("Hello"))?.len(), 2);
    Ok(())
}

#[test]
fn strangled_read_ports_serve_the_fold() -> Result<(), StoreError> {
    // The second strangle wave: list/detail/tags/search read `_effective` too.
    let root = temp_root();
    let store = overlay_store(&root);
    let account = AccountId::from("primary");
    seeded(&store, &account)?;

    // Fold: message-1 moved inbox→archive with a tag keyword, unread.
    let mut folded = sample_message("message-1", "archive", None);
    folded.keywords = vec!["urgent".to_string()];
    store.upsert_overlay_message(&account, &folded)?;

    // list_messages honors the folded membership on both filter branches.
    let inbox = store.list_messages(&account, Some(&MailboxId::from("inbox")))?;
    assert_eq!(inbox.len(), 1, "message-1 left inbox in the fold");
    assert_eq!(inbox[0].id.as_str(), "message-2");
    let archive = store.list_messages(&account, Some(&MailboxId::from("archive")))?;
    assert_eq!(archive.len(), 1);
    assert_eq!(archive[0].id.as_str(), "message-1");

    // Detail/summary reads serve folded values.
    let summary = store
        .get_message_summary(&account, &MessageId::from("message-1"))?
        .expect("summary for a folded message");
    assert_eq!(summary.mailbox_ids, vec![MailboxId::from("archive")]);
    assert!(!summary.is_read);

    // Tag aggregation counts the folded (non-$) keyword.
    let tags = store.list_tags(&account)?;
    let urgent = tags
        .iter()
        .find(|tag| tag.name == "urgent")
        .expect("folded tag appears");
    assert_eq!((urgent.unread_messages, urgent.total_messages), (1, 1));

    // FTS search: a tombstoned message drops out of results even though its
    // base content is still indexed.
    store.tombstone_overlay_message(&account, &MessageId::from("message-2"))?;
    let hits = store.fts_search_messages(&account, "Hello", 10)?;
    assert_eq!(hits.len(), 1, "tombstoned message hidden from search");
    assert_eq!(hits[0].id.as_str(), "message-1");
    Ok(())
}

#[test]
fn list_overlay_message_ids_inventories_live_and_tombstoned() -> Result<(), StoreError> {
    let root = temp_root();
    let store = overlay_store(&root);
    let account = AccountId::from("primary");
    seeded(&store, &account)?;

    store.upsert_overlay_message(&account, &sample_message("message-1", "archive", None))?;
    store.tombstone_overlay_message(&account, &MessageId::from("message-2"))?;

    let ids = store.list_overlay_message_ids(&account)?;
    assert_eq!(
        ids,
        vec![MessageId::from("message-1"), MessageId::from("message-2")],
        "the reseed inventory includes tombstoned entries"
    );

    // Account-scoped: another account sees nothing.
    let other = AccountId::from("secondary");
    assert!(store.list_overlay_message_ids(&other)?.is_empty());
    Ok(())
}
