use super::*;

#[test]
fn imap_mailbox_state_round_trips_provider_cursors() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let mut state = ImapMailboxSyncState::new(
        MailboxId::from("imap:inbox"),
        "INBOX".to_string(),
        ImapUidValidity(u32::MAX),
        "2026-04-25T00:00:00Z".to_string(),
    );
    state.record_seen_uid(ImapUid(u32::MAX));
    state.record_highest_modseq(ImapModSeq(u64::MAX));

    store.put_imap_mailbox_state(&account, &state)?;

    let loaded = store
        .get_imap_mailbox_state(&account, &MailboxId::from("imap:inbox"))?
        .expect("stored state");
    assert_eq!(loaded, state);
    assert_eq!(store.list_imap_mailbox_states(&account)?, vec![state]);
    Ok(())
}

#[test]
fn sender_address_cache_upserts_by_account_and_normalized_email() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let primary = AccountId::from("primary");
    let secondary = AccountId::from("secondary");

    store.remember_sender_address(
        &primary,
        &Recipient {
            name: Some("Catch One".to_string()),
            email: "Catch@Example.test".to_string(),
        },
    )?;
    store.remember_sender_address(
        &primary,
        &Recipient {
            name: Some("Catch Two".to_string()),
            email: "catch@example.test".to_string(),
        },
    )?;
    store.remember_sender_address(
        &secondary,
        &Recipient {
            name: None,
            email: "catch@example.test".to_string(),
        },
    )?;

    let cached = store.list_sender_address_cache()?;

    assert_eq!(cached.len(), 2);
    assert!(cached.iter().any(|sender| {
        sender.source_id == primary
            && sender.name.as_deref() == Some("Catch Two")
            && sender.email == "catch@example.test"
    }));
    assert!(cached.iter().any(|sender| {
        sender.source_id == secondary
            && sender.name.is_none()
            && sender.email == "catch@example.test"
    }));
    Ok(())
}

#[test]
fn sender_address_cache_ignores_non_concrete_sender_addresses() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");

    for email in [
        "*@example.test",
        "missing-at",
        "a b@example.test",
        "@example.test",
    ] {
        store.remember_sender_address(
            &account,
            &Recipient {
                name: None,
                email: email.to_string(),
            },
        )?;
    }

    assert!(store.list_sender_address_cache()?.is_empty());
    Ok(())
}

#[test]
fn imap_mailbox_state_delete_is_account_scoped() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let primary = AccountId::from("primary");
    let secondary = AccountId::from("secondary");
    let state = ImapMailboxSyncState::new(
        MailboxId::from("imap:inbox"),
        "INBOX".to_string(),
        ImapUidValidity(1),
        "2026-04-25T00:00:00Z".to_string(),
    );

    store.put_imap_mailbox_state(&primary, &state)?;
    store.put_imap_mailbox_state(&secondary, &state)?;
    store.delete_imap_mailbox_state(&primary, &MailboxId::from("imap:inbox"))?;

    assert!(store
        .get_imap_mailbox_state(&primary, &MailboxId::from("imap:inbox"))?
        .is_none());
    assert_eq!(
        store.get_imap_mailbox_state(&secondary, &MailboxId::from("imap:inbox"))?,
        Some(state)
    );
    Ok(())
}

#[test]
fn imap_message_locations_round_trip_multiple_mailboxes() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("imap:gmail:msgid:1278455344230334865");
    let inbox = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: MailboxId::from("imap:inbox"),
        uid_validity: ImapUidValidity(10),
        uid: ImapUid(101),
        modseq: Some(ImapModSeq(u64::MAX)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };
    let all_mail = ImapMessageLocation {
        mailbox_id: MailboxId::from("imap:all"),
        uid: ImapUid(202),
        ..inbox.clone()
    };

    store.put_imap_message_location(&account, &all_mail)?;
    store.put_imap_message_location(&account, &inbox)?;

    assert_eq!(
        store.list_imap_message_locations(&account, &message_id)?,
        vec![all_mail, inbox.clone()]
    );
    assert_eq!(
        store.list_imap_mailbox_message_locations(&account, &MailboxId::from("imap:inbox"))?,
        vec![inbox]
    );
    Ok(())
}

#[test]
fn imap_message_location_delete_is_account_scoped() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let primary = AccountId::from("primary");
    let secondary = AccountId::from("secondary");
    let message_id = MessageId::from("message-1");
    let location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: MailboxId::from("imap:inbox"),
        uid_validity: ImapUidValidity(10),
        uid: ImapUid(101),
        modseq: None,
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };

    store.put_imap_message_location(&primary, &location)?;
    store.put_imap_message_location(&secondary, &location)?;
    store.delete_imap_message_locations(&primary, &message_id)?;

    assert_eq!(
        store.list_imap_message_locations(&primary, &message_id)?,
        Vec::<ImapMessageLocation>::new()
    );
    assert_eq!(
        store.list_imap_message_locations(&secondary, &message_id)?,
        vec![location]
    );
    Ok(())
}

#[test]
fn delete_source_data_removes_imap_state_and_locations() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("message-1");
    let mailbox_id = MailboxId::from("imap:inbox");
    let state = ImapMailboxSyncState::new(
        mailbox_id.clone(),
        "INBOX".to_string(),
        ImapUidValidity(10),
        "2026-04-25T00:00:00Z".to_string(),
    );
    let location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: mailbox_id.clone(),
        uid_validity: ImapUidValidity(10),
        uid: ImapUid(101),
        modseq: None,
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };

    store.put_imap_mailbox_state(&account, &state)?;
    store.put_imap_message_location(&account, &location)?;
    store.delete_source_data(&account)?;

    assert!(store
        .get_imap_mailbox_state(&account, &mailbox_id)?
        .is_none());
    assert_eq!(
        store.list_imap_message_locations(&account, &message_id)?,
        Vec::<ImapMessageLocation>::new()
    );
    Ok(())
}
