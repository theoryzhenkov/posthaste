use super::*;

pub(crate) async fn replace_message_mailboxes(
    gateway: &LiveImapSmtpGateway,
    client: &mut ImapClient,
    account_id: &AccountId,
    message_id: &MessageId,
    mailbox_ids: &[MailboxId],
) -> Result<MutationOutcome, GatewayError> {
    let store = gateway.store("mailbox replacement state lookup")?;
    let locations = store
        .list_imap_message_locations(account_id, message_id)
        .map_err(store_error_to_gateway)?;
    // The wire delta is computed against the *provider-side* membership — the
    // sync-owned IMAP locations — NOT the local canonical mailbox junction
    // (`get_message_mailboxes`). The runtime applies the optimistic
    // write-through to the junction *before* the outbox pushes this command,
    // so by the time this runs the junction already equals the target set and
    // a delta computed against it is empty: every move silently became a
    // wire-level no-op (the Gmail "archive never reaches other clients" bug).
    // Locations are only written by sync, so they still reflect what the
    // server believes.
    let current_mailbox_ids: Vec<MailboxId> = locations
        .iter()
        .map(|location| location.mailbox_id.clone())
        .collect();
    let delta = imap_mailbox_replacement_delta(&current_mailbox_ids, mailbox_ids);

    if let Some((source_mailbox_id, target_mailbox_id)) =
        simple_imap_move_mailboxes(&gateway.discovery.capabilities, &delta)
    {
        let source_location = locations
            .iter()
            .find(|location| &location.mailbox_id == source_mailbox_id)
            .ok_or_else(|| {
                imap_error_to_gateway(ImapAdapterError::MissingMessageLocation(
                    source_mailbox_id.to_string(),
                ))
            })?;
        let source_mailbox_name = gateway.mailbox_name_for_id(account_id, source_mailbox_id)?;
        let target_mailbox_name = gateway.mailbox_name_for_id(account_id, target_mailbox_id)?;
        move_imap_message_to_mailbox_by_location(
            client,
            &source_mailbox_name,
            source_location,
            &target_mailbox_name,
        )
        .await
        .map_err(imap_error_to_gateway)?;

        return Ok(MutationOutcome {
            cursor: None,
            message: None,
        });
    }

    let source_location = locations.first().cloned().ok_or_else(|| {
        GatewayError::Rejected(format!("missing IMAP location for message {message_id}"))
    })?;
    let source_mailbox_name =
        gateway.mailbox_name_for_id(account_id, &source_location.mailbox_id)?;

    for mailbox_id in &delta.add {
        let target_mailbox_name = gateway.mailbox_name_for_id(account_id, mailbox_id)?;
        copy_imap_message_to_mailbox_by_location(
            client,
            &source_mailbox_name,
            &source_location,
            &target_mailbox_name,
        )
        .await
        .map_err(imap_error_to_gateway)?;
    }

    for mailbox_id in &delta.remove {
        let location = locations
            .iter()
            .find(|location| &location.mailbox_id == mailbox_id)
            .ok_or_else(|| {
                imap_error_to_gateway(ImapAdapterError::MissingMessageLocation(
                    mailbox_id.to_string(),
                ))
            })?;
        let mailbox_name = gateway.mailbox_name_for_id(account_id, mailbox_id)?;
        remove_imap_message_from_mailbox(gateway, client, &mailbox_name, location).await?;
    }

    Ok(MutationOutcome {
        cursor: None,
        message: None,
    })
}

/// Actually remove one message from one remote mailbox.
///
/// Under UIDPLUS (or IMAP4rev2) this is `UID STORE +FLAGS (\Deleted)` followed
/// by `UID EXPUNGE <uid>` — the UID-scoped expunge that cannot sweep other
/// clients' `\Deleted` messages (the plain-EXPUNGE footgun). A `\Deleted` flag
/// left unexpunged is NOT a removal: other IMAP clients still list the message,
/// and on Gmail (label model, Auto-Expunge configurable) it may never take
/// effect at all — archiving is exactly a remove-from-INBOX, so it must expunge.
///
/// Non-UIDPLUS fallback: mark `\Deleted` only, leaving a flagged residual for
/// the server/other clients to expunge. This is deliberate — plain `EXPUNGE`
/// (and CLOSE-based expunge) removes every `\Deleted` message in the mailbox,
/// including ones this client never touched, so it is never issued.
///
/// A UID that is already gone from the mailbox (`MissingFetchData` from the
/// pre-mutation UID probe) counts as removed: removal is idempotent, and Gmail
/// strips a message's other labels itself when it is copied/moved into Trash
/// or Spam, so the follow-up removals of a trash flow find nothing to expunge.
pub(crate) async fn remove_imap_message_from_mailbox(
    gateway: &LiveImapSmtpGateway,
    client: &mut ImapClient,
    mailbox_name: &str,
    location: &posthaste_domain_model::ImapMessageLocation,
) -> Result<(), GatewayError> {
    let result = if gateway.discovery.capabilities.supports_uidplus() {
        expunge_imap_message_by_location(client, mailbox_name, location).await
    } else {
        mark_imap_message_deleted_by_location(client, mailbox_name, location).await
    };
    match result {
        Ok(_) => Ok(()),
        Err(ImapAdapterError::MissingFetchData(_)) => Ok(()),
        Err(error) => Err(imap_error_to_gateway(error)),
    }
}

pub(crate) async fn destroy_message_by_imap(
    gateway: &LiveImapSmtpGateway,
    client: &mut ImapClient,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<MutationOutcome, GatewayError> {
    let locations = gateway
        .store("message deletion state lookup")?
        .list_imap_message_locations(account_id, message_id)
        .map_err(store_error_to_gateway)?;
    if locations.is_empty() {
        return Err(GatewayError::Rejected(format!(
            "missing IMAP location for message {message_id}"
        )));
    }

    for location in &locations {
        let mailbox_name = gateway.mailbox_name_for_id(account_id, &location.mailbox_id)?;
        remove_imap_message_from_mailbox(gateway, client, &mailbox_name, location).await?;
    }

    Ok(MutationOutcome {
        cursor: None,
        message: None,
    })
}
