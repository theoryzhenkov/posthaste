use super::*;

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

    Ok(())
}
