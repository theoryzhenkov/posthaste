use super::*;
use posthaste_query_grammar::parse_query;

/// Runs the FTS5 external-content integrity check: verifies the inverted index
/// agrees with the `message_fts_content` view. Errors (SQLITE_CORRUPT_VTAB) if
/// any trigger fed the index a 'delete' whose values drifted from what was
/// inserted — the failure mode the trigger invariant exists to prevent.
fn assert_fts_integrity(store: &DatabaseStore) -> Result<(), StoreError> {
    store.write_transaction(|tx| {
        tx.execute(
            "INSERT INTO message_fts(message_fts, rank) VALUES('integrity-check', 1)",
            [],
        )
        .map_err(sql_to_store_error)?;
        Ok(())
    })
}

/// Parses `query` with the shared grammar and runs it through the compiled
/// rule path (the live `/messages/search` pipeline), returning matched ids.
fn search_ids(store: &DatabaseStore, query: &str) -> Result<Vec<String>, StoreError> {
    let rule = parse_query(query).map_err(StoreError::Failure)?;
    let page = store.query_message_page_by_rule(
        &rule,
        50,
        None,
        MessageSortField::Date,
        SortDirection::Desc,
    )?;
    Ok(page
        .items
        .iter()
        .map(|item| item.id.as_str().to_string())
        .collect())
}

#[test]
fn fts_search_matches_subject_terms_and_tracks_writes() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![
            MessageRecord {
                subject: Some("Quarterly invoice ready".to_string()),
                ..sample_message("m-1", "inbox", Some("a"))
            },
            MessageRecord {
                subject: Some("Team lunch".to_string()),
                ..sample_message("m-2", "inbox", Some("b"))
            },
            MessageRecord {
                subject: Some("Invoice reminder".to_string()),
                ..sample_message("m-3", "inbox", Some("c"))
            },
        ],
        "state",
    )?;

    let hits = store.fts_search_messages(&account, "invoice", 50)?;
    let ids: Vec<String> = hits.iter().map(|s| s.id.as_str().to_string()).collect();
    assert_eq!(
        hits.len(),
        2,
        "matches both invoice subjects, not the lunch one"
    );
    assert!(ids.contains(&"m-1".to_string()));
    assert!(ids.contains(&"m-3".to_string()));

    // The external-content index is trigger-maintained: a delete must drop the
    // row from search results too.
    store.destroy_message(&account, &MessageId::from("m-1"), None)?;
    let after_delete = store.fts_search_messages(&account, "invoice", 50)?;
    assert_eq!(after_delete.len(), 1);
    assert_eq!(after_delete[0].id.as_str(), "m-3");
    assert_fts_integrity(&store)?;

    Ok(())
}

/// A message is body-searchable only once the body cache stores its text: the
/// same query misses before the body-cache write and hits after (the
/// `message_body` triggers index the body at write time), through both the
/// `body:` prefix and a default unprefixed search.
#[test]
fn body_search_hits_only_after_body_cache_write() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("m-lazy");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![metadata_only_message(message_id.as_str(), "inbox")],
        "state",
    )?;

    // Header row exists, body not yet cached: no body match anywhere.
    assert!(search_ids(&store, "body:zanzibar")?.is_empty());
    assert!(search_ids(&store, "zanzibar")?.is_empty());
    assert!(store
        .fts_search_messages(&account, "zanzibar", 50)?
        .is_empty());

    // The lazy body fetch lands (the path that emits message.body_cached).
    store.apply_message_body(
        &account,
        &message_id,
        &FetchedBody {
            cc: Vec::new(),
            bcc: Vec::new(),
            reply_to: Vec::new(),
            body_html: Some("<p>the zanzibar shipment arrived</p>".to_string()),
            body_text: Some("the zanzibar shipment arrived".to_string()),
            attachments: Vec::new(),
            raw_mime: None,
            list_unsubscribe: None,
        },
    )?;

    assert_eq!(search_ids(&store, "body:zanzibar")?, vec!["m-lazy"]);
    assert_eq!(search_ids(&store, "zanzibar")?, vec!["m-lazy"]);
    assert_eq!(
        store.fts_search_messages(&account, "zanzibar", 50)?.len(),
        1
    );
    // Porter stemming + trailing prefix token: partial and inflected query
    // forms still hit through the compiled `body:` path.
    assert_eq!(search_ids(&store, "body:zanzib")?, vec!["m-lazy"]);
    assert_eq!(search_ids(&store, "body:shipments")?, vec!["m-lazy"]);
    assert_fts_integrity(&store)?;
    Ok(())
}

/// `body:` is scoped to the FTS body column: a term that appears only in the
/// subject/preview does not match `body:`, and vice versa the body-only term
/// stays invisible to `subject:`/`preview:` — while a default unprefixed
/// search spans all of them.
#[test]
fn body_scope_is_pinned_against_header_scopes() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![
            MessageRecord {
                subject: Some("quokka sighting".to_string()),
                preview: Some("no body terms here".to_string()),
                body_text: Some("plain text without the marsupial".to_string()),
                body_html: None,
                ..sample_message("m-subject", "inbox", Some("a"))
            },
            MessageRecord {
                subject: Some("weekly digest".to_string()),
                preview: Some("weekly digest".to_string()),
                body_text: Some("a quokka appears only in the body".to_string()),
                body_html: None,
                ..sample_message("m-body", "inbox", Some("b"))
            },
        ],
        "state",
    )?;

    assert_eq!(search_ids(&store, "body:quokka")?, vec!["m-body"]);
    assert_eq!(search_ids(&store, "subject:quokka")?, vec!["m-subject"]);
    assert!(search_ids(&store, "preview:quokka")?.is_empty());
    // Default scope spans headers and bodies.
    let mut default_hits = search_ids(&store, "quokka")?;
    default_hits.sort();
    assert_eq!(default_hits, vec!["m-body", "m-subject"]);
    // Negation composes with the FTS subquery.
    assert_eq!(
        search_ids(&store, "-body:quokka subject:quokka")?,
        vec!["m-subject"]
    );
    Ok(())
}

/// A `body:` value with no indexable tokens (punctuation/whitespace only, or
/// FTS5 query metacharacters) compiles to a constant-false predicate instead
/// of a MATCH syntax error.
#[test]
fn body_search_with_only_metacharacters_matches_nothing() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![sample_message("m-1", "inbox", Some("a"))],
        "state",
    )?;
    for query in ["body:\"*\"", "body:...", "body:(-)"] {
        assert!(search_ids(&store, query)?.is_empty(), "query {query}");
    }
    Ok(())
}

/// bm25 column weights: a subject hit outranks a body-only hit even when the
/// body-only message is newer (recency would otherwise win).
#[test]
fn subject_hits_rank_above_body_only_hits() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![
            MessageRecord {
                subject: Some("meteor shower tonight".to_string()),
                preview: Some("look up".to_string()),
                body_text: Some("clear skies expected".to_string()),
                body_html: None,
                received_at: "2026-03-01T10:00:00Z".to_string(),
                ..sample_message("m-subject-hit", "inbox", Some("a"))
            },
            MessageRecord {
                subject: Some("astronomy newsletter".to_string()),
                preview: Some("this month in space".to_string()),
                body_text: Some("a meteor was photographed over the bay".to_string()),
                body_html: None,
                received_at: "2026-03-20T10:00:00Z".to_string(),
                ..sample_message("m-body-hit", "inbox", Some("b"))
            },
        ],
        "state",
    )?;

    let hits = store.fts_search_messages(&account, "meteor", 50)?;
    let ids: Vec<&str> = hits.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["m-subject-hit", "m-body-hit"],
        "the older subject match must outrank the newer body-only match"
    );
    Ok(())
}

/// The deferred startup backfill repopulates an empty index (the state the
/// body-indexing migration leaves behind) from already-stored messages AND
/// already-cached bodies, and is a no-op otherwise.
#[test]
fn backfill_message_fts_rebuilds_only_an_empty_index() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![MessageRecord {
            subject: Some("quarterly invoice".to_string()),
            body_text: Some("the xylophone budget line".to_string()),
            body_html: None,
            ..sample_message("m-1", "inbox", Some("a"))
        }],
        "state",
    )?;

    // Live index (trigger-maintained, non-empty): the backfill must not rerun
    // the rebuild on every startup.
    assert!(!store.backfill_message_fts()?);

    // Simulate the post-migration state: messages exist, index is empty.
    store.write_transaction(|tx| {
        tx.execute(
            "INSERT INTO message_fts(message_fts) VALUES('delete-all')",
            [],
        )
        .map_err(sql_to_store_error)?;
        Ok(())
    })?;
    assert!(store
        .fts_search_messages(&account, "invoice", 50)?
        .is_empty());

    assert!(store.backfill_message_fts()?, "rebuild should run once");
    assert_eq!(store.fts_search_messages(&account, "invoice", 50)?.len(), 1);
    assert_eq!(
        search_ids(&store, "body:xylophone")?,
        vec!["m-1"],
        "pre-existing cached bodies must be indexed by the backfill"
    );
    assert_fts_integrity(&store)?;
    assert!(!store.backfill_message_fts()?, "idempotent after rebuild");
    Ok(())
}

/// The trigger invariant survives the full write lifecycle — metadata re-sync
/// (message UPDATE), lazy body fetch (message_body INSERT then UPDATE), keyword
/// flips, and message destroy — verified by FTS5's own external-content
/// integrity check plus post-delete searches.
#[test]
fn fts_index_stays_consistent_across_write_lifecycle() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![
            metadata_only_message("m-1", "inbox"),
            sample_message("m-2", "inbox", Some("b")),
        ],
        "state",
    )?;

    // Lazy body fetch: message_body INSERT…
    store.apply_message_body(
        &account,
        &MessageId::from("m-1"),
        &FetchedBody {
            cc: Vec::new(),
            bcc: Vec::new(),
            reply_to: Vec::new(),
            body_html: None,
            body_text: Some("first fetched body with a pangolin".to_string()),
            attachments: Vec::new(),
            raw_mime: None,
            list_unsubscribe: None,
        },
    )?;
    // …then a re-fetch: message_body UPDATE replacing the text.
    store.apply_message_body(
        &account,
        &MessageId::from("m-1"),
        &FetchedBody {
            cc: Vec::new(),
            bcc: Vec::new(),
            reply_to: Vec::new(),
            body_html: None,
            body_text: Some("second fetched body with an axolotl".to_string()),
            attachments: Vec::new(),
            raw_mime: None,
            list_unsubscribe: None,
        },
    )?;
    assert!(search_ids(&store, "body:pangolin")?.is_empty());
    assert_eq!(search_ids(&store, "body:axolotl")?, vec!["m-1"]);

    // Metadata re-sync: message UPDATE (subject change) with the body present —
    // the message trigger must carry the live body through delete+reinsert.
    seed_messages(
        &store,
        &account,
        vec![MessageRecord {
            subject: Some("updated subject".to_string()),
            body_html: None,
            body_text: None,
            raw_mime: None,
            ..metadata_only_message("m-1", "inbox")
        }],
        "state-2",
    )?;
    assert_eq!(search_ids(&store, "body:axolotl")?, vec!["m-1"]);

    // Keyword flip: another message UPDATE path.
    store.set_keywords(
        &account,
        &MessageId::from("m-1"),
        None,
        &SetKeywordsCommand {
            add: vec!["$flagged".to_string()],
            remove: Vec::new(),
        },
    )?;

    // Destroy: message + message_body rows go; the index row must too.
    store.destroy_message(&account, &MessageId::from("m-1"), None)?;
    assert!(search_ids(&store, "body:axolotl")?.is_empty());

    assert_fts_integrity(&store)?;
    Ok(())
}

/// `to:` end-to-end: the grammar's To condition compiled against `to_json`
/// finds the message by recipient email or display name.
#[test]
fn to_prefix_matches_recipients_end_to_end() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![
            MessageRecord {
                to: vec![Recipient {
                    name: Some("Alice Aardvark".to_string()),
                    email: "alice@example.com".to_string(),
                }],
                ..sample_message("m-to-alice", "inbox", Some("a"))
            },
            MessageRecord {
                to: vec![Recipient {
                    name: None,
                    email: "carol@example.net".to_string(),
                }],
                ..sample_message("m-to-carol", "inbox", Some("b"))
            },
        ],
        "state",
    )?;

    assert_eq!(search_ids(&store, "to:alice@")?, vec!["m-to-alice"]);
    assert_eq!(search_ids(&store, "to:aardvark")?, vec!["m-to-alice"]);
    assert_eq!(search_ids(&store, "recipient:carol")?, vec!["m-to-carol"]);
    assert_eq!(search_ids(&store, "-to:alice@")?, vec!["m-to-carol"]);
    assert!(search_ids(&store, "to:mallory")?.is_empty());
    Ok(())
}

/// Upgrade from the pre-body prototype schema: a database whose `message_fts`
/// is the old header-only external-content table (over `message` directly) is
/// migrated on open — old table + triggers dropped, new shape created empty —
/// and the deferred backfill re-indexes headers AND the bodies that were
/// already cached before the upgrade.
#[test]
fn migration_from_header_only_fts_schema_reindexes_bodies() -> Result<(), StoreError> {
    let root = temp_root();
    let db_path = root.join("mail.sqlite");
    let account = AccountId::from("primary");

    // 1. Build a database with real content (message + cached body).
    {
        let store = DatabaseStore::open(&db_path, root.join("data"))?;
        setup_source(&store, &account, "Primary")?;
        seed_messages(
            &store,
            &account,
            vec![MessageRecord {
                subject: Some("quarterly invoice".to_string()),
                body_text: Some("the xylophone budget line".to_string()),
                body_html: None,
                ..sample_message("m-old", "inbox", Some("a"))
            }],
            "state",
        )?;
        store.close();
    }

    // 2. Downgrade its FTS artifacts to the legacy header-only prototype shape.
    {
        let connection = rusqlite::Connection::open(&db_path).map_err(sql_to_store_error)?;
        connection
            .execute_batch(
                "DROP TRIGGER message_fts_ai;
                 DROP TRIGGER message_fts_ad;
                 DROP TRIGGER message_fts_au;
                 DROP TRIGGER message_body_fts_ai;
                 DROP TRIGGER message_body_fts_au;
                 DROP TRIGGER message_body_fts_ad;
                 DROP TABLE message_fts;
                 DROP VIEW message_fts_content;
                 CREATE VIRTUAL TABLE message_fts USING fts5(
                     subject, from_name, from_email, preview,
                     content='message', content_rowid='rowid',
                     tokenize='porter unicode61 remove_diacritics 2'
                 );
                 CREATE TRIGGER message_fts_ai AFTER INSERT ON message BEGIN
                     INSERT INTO message_fts(rowid, subject, from_name, from_email, preview)
                     VALUES (new.rowid, new.subject, new.from_name, new.from_email, new.preview);
                 END;
                 CREATE TRIGGER message_fts_ad AFTER DELETE ON message BEGIN
                     INSERT INTO message_fts(message_fts, rowid, subject, from_name, from_email, preview)
                     VALUES ('delete', old.rowid, old.subject, old.from_name, old.from_email, old.preview);
                 END;
                 CREATE TRIGGER message_fts_au AFTER UPDATE ON message BEGIN
                     INSERT INTO message_fts(message_fts, rowid, subject, from_name, from_email, preview)
                     VALUES ('delete', old.rowid, old.subject, old.from_name, old.from_email, old.preview);
                     INSERT INTO message_fts(rowid, subject, from_name, from_email, preview)
                     VALUES (new.rowid, new.subject, new.from_name, new.from_email, new.preview);
                 END;
                 INSERT INTO message_fts(message_fts) VALUES('rebuild');",
            )
            .map_err(sql_to_store_error)?;
    }

    // 3. Reopen: init_schema migrates the shape; the deferred backfill (run
    //    here directly, as the composition root would) restores the content.
    let store = DatabaseStore::open(&db_path, root.join("data"))?;
    let schema_sql: String = {
        let connection = store.read_connection()?;
        connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'message_fts'",
                [],
                |row| row.get(0),
            )
            .map_err(sql_to_store_error)?
    };
    assert!(
        schema_sql.contains("message_fts_content"),
        "migration must recreate message_fts over the content view: {schema_sql}"
    );
    assert!(
        store
            .fts_search_messages(&account, "invoice", 50)?
            .is_empty(),
        "the migrated index starts empty until the backfill runs"
    );

    assert!(store.backfill_message_fts()?);
    assert_eq!(store.fts_search_messages(&account, "invoice", 50)?.len(), 1);
    assert_eq!(search_ids(&store, "body:xylophone")?, vec!["m-old"]);
    assert_fts_integrity(&store)?;

    // A database already on the new shape is untouched by another open.
    store.close();
    let reopened = DatabaseStore::open(&db_path, root.join("data"))?;
    assert_eq!(
        reopened.fts_search_messages(&account, "invoice", 50)?.len(),
        1,
        "re-opening a migrated database must not drop the index again"
    );
    Ok(())
}
