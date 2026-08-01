//! The offline re-derive of message metadata from cached raw MIME: the repair
//! for mail whose body was cached BEFORE `cc`/`bcc`/`reply_to`/
//! `list_unsubscribe` existed, which the at-open fill can never reach (it only
//! runs on a body fetch, and a cached body never fetches again).

use posthaste_domain_model::{FetchedBody, Recipient};

use super::*;

const RAW_WITH_HEADERS: &str = concat!(
    "From: Alice <alice@example.com>\r\n",
    "To: Bob <bob@example.test>\r\n",
    "Cc: Carol <carol@example.test>\r\n",
    "Reply-To: replies@example.test\r\n",
    "List-Unsubscribe: <https://lists.example.test/u/1>\r\n",
    "Subject: Hello\r\n",
    "\r\n",
    "Hello.\r\n",
);

fn named(name: &str, email: &str) -> Recipient {
    Recipient {
        name: Some(name.to_string()),
        email: email.to_string(),
    }
}

fn bare(email: &str) -> Recipient {
    Recipient {
        name: None,
        email: email.to_string(),
    }
}

/// Caches a body (and its raw `.eml`) the way the body-fetch path does, then
/// blanks the derived columns back out — standing in for a row whose body was
/// cached by a build that did not yet know about them. Blanking directly is
/// the only way to reach that state: the current write path fills as it
/// caches, which is exactly the gap this pass exists to cover.
fn cache_body_as_pre_upgrade(
    store: &DatabaseStore,
    account: &AccountId,
    message_id: &MessageId,
    raw: &str,
) -> Result<(), StoreError> {
    store.apply_message_body(
        account,
        message_id,
        &FetchedBody {
            body_html: None,
            body_text: Some("Hello.".to_string()),
            raw_mime: Some(raw.to_string()),
            attachments: Vec::new(),
            list_unsubscribe: None,
            cc: Vec::new(),
            bcc: Vec::new(),
            reply_to: Vec::new(),
        },
    )?;
    blank_derived_columns(store, account, message_id)
}

fn blank_derived_columns(
    store: &DatabaseStore,
    account: &AccountId,
    message_id: &MessageId,
) -> Result<(), StoreError> {
    store.write_transaction(|tx| {
        tx.execute(
            "UPDATE message
                SET cc_json = '[]', bcc_json = '[]', reply_to_json = '[]',
                    list_unsubscribe = NULL
              WHERE account_id = ?1 AND id = ?2",
            params![account.as_str(), message_id.as_str()],
        )
        .map_err(sql_to_store_error)?;
        Ok(())
    })
}

fn raw_path_of(
    store: &DatabaseStore,
    account: &AccountId,
    message_id: &MessageId,
) -> Result<String, StoreError> {
    let connection = store.read_connection()?;
    connection
        .query_row(
            "SELECT raw_path FROM message_body WHERE account_id = ?1 AND message_id = ?2",
            params![account.as_str(), message_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .map_err(sql_to_store_error)
}

fn seeded_store(
    root: &crate::test_support::TempDirGuard,
    message_ids: &[&str],
) -> Result<(DatabaseStore, AccountId), StoreError> {
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        message_ids
            .iter()
            .map(|id| metadata_only_message(id, "inbox"))
            .collect(),
        "state-1",
    )?;
    Ok((store, account))
}

#[test]
fn rederive_fills_empty_columns_from_the_cached_raw_message() -> Result<(), StoreError> {
    let root = temp_root();
    let (store, account) = seeded_store(&root, &["old-1"])?;
    let message_id = MessageId::from("old-1");
    cache_body_as_pre_upgrade(&store, &account, &message_id, RAW_WITH_HEADERS)?;

    let before = store
        .get_message_detail(&account, &message_id)?
        .expect("detail");
    assert!(before.summary.cc.is_empty(), "the gap this pass repairs");

    let report = store.rederive_message_metadata()?;
    assert_eq!(report.examined, 1);
    assert_eq!(report.filled, 1);
    assert_eq!(report.unreadable, 0);

    let after = store
        .get_message_detail(&account, &message_id)?
        .expect("detail");
    assert_eq!(after.summary.cc, vec![named("Carol", "carol@example.test")]);
    assert_eq!(after.summary.reply_to, vec![bare("replies@example.test")]);
    // Bcc is stripped in transit, so a received message has none to recover —
    // permanent absence, not missing data.
    assert!(after.summary.bcc.is_empty());
    assert!(
        after.list_unsubscribe.is_some(),
        "the nullable column fills on the same footing as the array ones"
    );
    Ok(())
}

#[test]
fn rederive_never_clobbers_a_populated_value() -> Result<(), StoreError> {
    let root = temp_root();
    let (store, account) = seeded_store(&root, &["kept-1"])?;
    let message_id = MessageId::from("kept-1");
    // A stored Cc that disagrees with the cached bytes: whatever put it there
    // saw the message too, and these headers are immutable per message, so the
    // stored value is never second-guessed.
    store.apply_message_body(
        &account,
        &message_id,
        &FetchedBody {
            body_html: None,
            body_text: Some("Hello.".to_string()),
            raw_mime: Some(RAW_WITH_HEADERS.to_string()),
            attachments: Vec::new(),
            list_unsubscribe: None,
            cc: vec![bare("stored@example.test")],
            bcc: Vec::new(),
            reply_to: Vec::new(),
        },
    )?;

    let report = store.rederive_message_metadata()?;
    assert_eq!(report.examined, 1);
    // Reply-To was still empty and does fill; Cc must not move.
    assert_eq!(report.filled, 1);

    let detail = store
        .get_message_detail(&account, &message_id)?
        .expect("detail");
    assert_eq!(detail.summary.cc, vec![bare("stored@example.test")]);
    assert_eq!(detail.summary.reply_to, vec![bare("replies@example.test")]);
    Ok(())
}

#[test]
fn a_second_rederive_writes_nothing() -> Result<(), StoreError> {
    let root = temp_root();
    let (store, account) = seeded_store(&root, &["old-1"])?;
    let message_id = MessageId::from("old-1");
    cache_body_as_pre_upgrade(&store, &account, &message_id, RAW_WITH_HEADERS)?;

    assert_eq!(store.rederive_message_metadata()?.filled, 1);
    let second = store.rederive_message_metadata()?;
    assert_eq!(second.examined, 1, "the row is still considered");
    assert_eq!(second.filled, 0, "but nothing is left to fill");

    let detail = store
        .get_message_detail(&account, &message_id)?
        .expect("detail");
    assert_eq!(
        detail.summary.cc,
        vec![named("Carol", "carol@example.test")]
    );
    Ok(())
}

#[test]
fn a_missing_or_corrupt_raw_object_is_skipped_not_fatal() -> Result<(), StoreError> {
    let root = temp_root();
    let (store, account) = seeded_store(&root, &["gone-1", "junk-1", "good-1"])?;
    for id in ["gone-1", "junk-1", "good-1"] {
        // Distinct bytes per message: the raw store is content-addressed, so
        // three identical bodies would share one file and damaging one would
        // damage all three.
        let raw = RAW_WITH_HEADERS.replace("Hello.", &format!("Hello {id}."));
        cache_body_as_pre_upgrade(&store, &account, &MessageId::from(id), &raw)?;
    }
    // The cache prunes files out from under their rows; a repair that aborted
    // on the first one would never reach the messages it can still fix.
    std::fs::remove_file(raw_path_of(&store, &account, &MessageId::from("gone-1"))?)
        .expect("prune the cached object");
    // Bytes that are not a message. The MIME parser is deliberately lenient,
    // so this usually parses into a message with no headers rather than
    // failing outright — either way the row derives nothing and is skipped.
    std::fs::write(
        raw_path_of(&store, &account, &MessageId::from("junk-1"))?,
        [0xff, 0xfe, 0x00, 0x01],
    )
    .expect("corrupt the cached object");

    let report = store.rederive_message_metadata()?;
    assert_eq!(report.examined, 3);
    assert_eq!(report.unreadable, 1, "the pruned file");
    assert_eq!(
        report.filled, 1,
        "the intact object still repairs; the corrupt one derives nothing"
    );
    let junk = store
        .get_message_detail(&account, &MessageId::from("junk-1"))?
        .expect("detail");
    assert!(junk.summary.cc.is_empty(), "and nothing invented for it");

    let good = store
        .get_message_detail(&account, &MessageId::from("good-1"))?
        .expect("detail");
    assert_eq!(good.summary.cc, vec![named("Carol", "carol@example.test")]);
    Ok(())
}

#[test]
fn a_message_with_no_cached_raw_is_left_alone() -> Result<(), StoreError> {
    let root = temp_root();
    // `metadata_only_message` carries no body at all, so nothing is staged to
    // disk and no `message_body` row gains a `raw_path`.
    let (store, account) = seeded_store(&root, &["headers-only-1"])?;

    let report = store.rederive_message_metadata()?;
    assert_eq!(
        report.examined, 0,
        "the pass is scoped to bodies it can actually read"
    );
    let detail = store
        .get_message_detail(&account, &MessageId::from("headers-only-1"))?
        .expect("detail");
    assert!(detail.summary.cc.is_empty());
    Ok(())
}

#[test]
fn the_guarded_pass_runs_once_and_then_short_circuits() -> Result<(), StoreError> {
    let root = temp_root();
    let (store, account) = seeded_store(&root, &["old-1"])?;
    let message_id = MessageId::from("old-1");
    cache_body_as_pre_upgrade(&store, &account, &message_id, RAW_WITH_HEADERS)?;

    let first = store
        .rederive_stale_message_metadata()?
        .expect("the first startup after the upgrade runs the pass");
    assert_eq!(first.filled, 1);

    // Re-blank the columns: if the guard were consulting the DATA it would see
    // work to do here, and the point is that it does not — it consults the
    // recorded fact that the pass ran, because "not derived yet" and
    // "legitimately has no Cc" are the same empty column.
    blank_derived_columns(&store, &account, &message_id)?;
    assert!(
        store.rederive_stale_message_metadata()?.is_none(),
        "every later startup must short-circuit"
    );
    Ok(())
}

#[test]
fn the_startup_guard_is_a_single_indexed_probe() -> Result<(), StoreError> {
    let root = temp_root();
    let (store, _account) = seeded_store(&root, &["old-1"])?;
    let connection = store.read_connection()?;
    // The hard requirement: this runs on EVERY startup, so it must not scan.
    // A `SEARCH ... USING ... INDEX` plan (never `SCAN`) is what makes that
    // true independently of how much mail the store holds.
    let plan: String = connection
        .query_row(
            "EXPLAIN QUERY PLAN
             SELECT value FROM store_maintenance_marker WHERE key = 'x'",
            [],
            |row| row.get(3),
        )
        .map_err(sql_to_store_error)?;
    assert!(
        plan.contains("SEARCH") && plan.contains("INDEX") && !plan.contains("SCAN"),
        "guard probe must be an index seek, got: {plan}"
    );

    // The pass's own paging must also be an index seek, so a big body cache
    // costs one primary-key descent per page rather than a re-scan.
    let page_plan: String = connection
        .query_row(
            "EXPLAIN QUERY PLAN
             SELECT account_id, message_id, raw_path
               FROM message_body
              WHERE raw_path IS NOT NULL
                AND (account_id > '' OR (account_id = '' AND message_id > ''))
              ORDER BY account_id, message_id
              LIMIT 200",
            [],
            |row| row.get(3),
        )
        .map_err(sql_to_store_error)?;
    assert!(
        page_plan.contains("USING INDEX") || page_plan.contains("USING PRIMARY KEY"),
        "page query must ride the primary key, got: {page_plan}"
    );
    Ok(())
}

#[test]
fn the_at_open_fill_and_the_rederive_agree_on_the_same_bytes() -> Result<(), StoreError> {
    let root = temp_root();
    let (store, account) = seeded_store(&root, &["filled-at-open", "repaired-offline"])?;
    // The at-open path, as a provider drives it: the gateway parses the raw
    // MIME and hands the fields over on the fetched body.
    let at_open = MessageId::from("filled-at-open");
    let derived = posthaste_domain_service::derive_message_metadata(RAW_WITH_HEADERS.as_bytes())
        .expect("parses");
    store.apply_message_body(
        &account,
        &at_open,
        &FetchedBody {
            body_html: None,
            body_text: Some("Hello.".to_string()),
            raw_mime: Some(RAW_WITH_HEADERS.to_string()),
            attachments: Vec::new(),
            list_unsubscribe: derived.list_unsubscribe,
            cc: derived.cc,
            bcc: derived.bcc,
            reply_to: derived.reply_to,
        },
    )?;

    // The offline path, over the same bytes.
    let offline = MessageId::from("repaired-offline");
    cache_body_as_pre_upgrade(&store, &account, &offline, RAW_WITH_HEADERS)?;
    store.rederive_message_metadata()?;

    let filled = store
        .get_message_detail(&account, &at_open)?
        .expect("detail");
    let repaired = store
        .get_message_detail(&account, &offline)?
        .expect("detail");
    assert_eq!(filled.summary.cc, repaired.summary.cc);
    assert_eq!(filled.summary.bcc, repaired.summary.bcc);
    assert_eq!(filled.summary.reply_to, repaired.summary.reply_to);
    assert_eq!(filled.list_unsubscribe, repaired.list_unsubscribe);
    Ok(())
}

#[test]
fn rederive_pages_through_more_rows_than_one_chunk() -> Result<(), StoreError> {
    let root = temp_root();
    // One past the chunk size, so the keyset cursor has to advance correctly:
    // an off-by-one there either loops forever or silently drops the tail.
    let ids = (0..crate::derived_metadata::REDERIVE_CHUNK + 1)
        .map(|index| format!("bulk-{index:04}"))
        .collect::<Vec<_>>();
    let (store, account) =
        seeded_store(&root, &ids.iter().map(String::as_str).collect::<Vec<_>>())?;
    for (index, id) in ids.iter().enumerate() {
        // Distinct bytes per message: the raw store is content-addressed, so
        // identical bodies would dedup to one file and one `raw_path`.
        let raw = RAW_WITH_HEADERS.replace("Hello.", &format!("Hello {index}."));
        cache_body_as_pre_upgrade(&store, &account, &MessageId::from(id.as_str()), &raw)?;
    }

    let report = store.rederive_message_metadata()?;
    assert_eq!(report.examined as usize, ids.len());
    assert_eq!(report.filled as usize, ids.len());

    let last = store
        .get_message_detail(&account, &MessageId::from(ids.last().unwrap().as_str()))?
        .expect("detail");
    assert_eq!(last.summary.cc, vec![named("Carol", "carol@example.test")]);
    Ok(())
}
