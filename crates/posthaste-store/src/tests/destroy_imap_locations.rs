use super::*;

/// Whether a canonical `message` row exists (the optimistic-hide dimension).
fn message_row_exists(
    store: &DatabaseStore,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<bool, StoreError> {
    let connection = store.read_connection()?;
    let exists = connection
        .query_row(
            "SELECT 1 FROM message WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), message_id.as_str()],
            |_row| Ok(()),
        )
        .optional()
        .map_err(sql_to_store_error)?
        .is_some();
    Ok(exists)
}

/// A minimal server-confirmed deletion batch: the message id the provider now
/// reports gone (VANISHED / absent from `UID SEARCH UNDELETED`), nothing else.
fn server_deleted_batch(message_id: &MessageId) -> SyncBatch {
    SyncBatch {
        mailboxes: Vec::new(),
        messages: Vec::new(),
        imap_mailbox_states: Vec::new(),
        imap_message_locations: Vec::new(),
        deleted_imap_message_locations: Vec::new(),
        deleted_mailbox_ids: Vec::new(),
        absence_deleted_imap_message_locations: Vec::new(),
        absence_deleted_message_ids: Vec::new(),
        deleted_message_ids: vec![message_id.clone()],
        replace_all_mailboxes: false,
        replace_all_messages: false,
        cursors: vec![message_cursor("cursor-settle", "2026-03-31T11:00:00Z")],
    }
}

/// DP-C1 regression: the optimistic IMAP hard-delete (Destroy) must HIDE the
/// message immediately but must NOT wipe the sync-owned `imap_message_location`
/// coordinates — the outbox flush reads them back to issue the server-side
/// `UID STORE \Deleted` + `UID EXPUNGE`. Wiping them optimistically left the
/// flush with no coordinates (op `Rejected`→`Failed`, server delete never
/// issued) and the next IMAP delta re-imported the still-live UID → the message
/// resurrected. The coordinates are torn down exactly once, later, when the
/// server confirms the expunge.
///
/// Fails before the fix (the optimistic Destroy deleted the location row, so the
/// post-destroy `list_imap_message_locations` is empty); passes after.
#[test]
fn optimistic_destroy_retains_imap_locations_until_server_confirms() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let message_id = MessageId::from("imap:primary:msgid:42");
    seed_messages(
        &store,
        &account,
        vec![sample_message(
            message_id.as_str(),
            "inbox",
            Some("mime-42"),
        )],
        "cursor-seed",
    )?;

    // The sync-owned IMAP coordinates the flush needs to address the delete.
    let location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: MailboxId::from("inbox"),
        uid_validity: ImapUidValidity(10),
        uid: ImapUid(101),
        modseq: Some(ImapModSeq(7)),
        updated_at: "2026-03-31T10:00:00Z".to_string(),
    };
    store.put_imap_message_location(&account, &location)?;

    assert!(message_row_exists(&store, &account, &message_id)?);
    assert_eq!(
        store.list_imap_message_locations(&account, &message_id)?,
        vec![location.clone()],
        "precondition: the message has IMAP coordinates the flush will read",
    );

    // Optimistic hard-delete write-through (the S2 canonical Destroy).
    store.destroy_message(&account, &message_id, None)?;

    // Optimistic hide is immediate: the canonical row is gone, so the UI
    // reflects the delete right away.
    assert!(
        !message_row_exists(&store, &account, &message_id)?,
        "optimistic Destroy must hide the message immediately",
    );

    // ...but the IMAP coordinates SURVIVE the optimistic write so the outbox
    // flush can still build the server-side delete command. This is the guard
    // that would have caught DP-C1: before the fix this vector is empty.
    assert_eq!(
        store.list_imap_message_locations(&account, &message_id)?,
        vec![location],
        "optimistic Destroy must NOT wipe imap_message_location — the flush needs \
         it to issue the server-side delete (DP-C1)",
    );

    // The flush issues the server delete; the next sync observes the expunge and
    // reports the message gone. THAT is when the coordinates are torn down —
    // exactly once, on server confirmation.
    store.apply_sync_batch(&account, &server_deleted_batch(&message_id))?;
    assert_eq!(
        store.list_imap_message_locations(&account, &message_id)?,
        Vec::<ImapMessageLocation>::new(),
        "server-confirmed deletion tears down the coordinates on settle",
    );
    assert!(!message_row_exists(&store, &account, &message_id)?);

    Ok(())
}
