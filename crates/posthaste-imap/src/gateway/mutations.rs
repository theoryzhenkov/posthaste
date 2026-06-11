use super::*;

pub(crate) async fn replace_message_mailboxes(
    gateway: &LiveImapSmtpGateway,
    account_id: &AccountId,
    message_id: &MessageId,
    mailbox_ids: &[MailboxId],
) -> Result<MutationOutcome, GatewayError> {
    let store = gateway.store("mailbox replacement state lookup")?;
    let current_mailbox_ids = store
        .get_message_mailboxes(account_id, message_id)
        .map_err(store_error_to_gateway)?;
    let locations = store
        .list_imap_message_locations(account_id, message_id)
        .map_err(store_error_to_gateway)?;
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
            &gateway.config,
            &source_mailbox_name,
            source_location,
            &target_mailbox_name,
        )
        .await
        .map_err(imap_error_to_gateway)?;

        return Ok(MutationOutcome { cursor: None });
    }

    let source_location = locations.first().cloned().ok_or_else(|| {
        GatewayError::Rejected(format!("missing IMAP location for message {message_id}"))
    })?;
    let source_mailbox_name =
        gateway.mailbox_name_for_id(account_id, &source_location.mailbox_id)?;

    for mailbox_id in &delta.add {
        let target_mailbox_name = gateway.mailbox_name_for_id(account_id, mailbox_id)?;
        copy_imap_message_to_mailbox_by_location(
            &gateway.config,
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
        mark_imap_message_deleted_by_location(&gateway.config, &mailbox_name, location)
            .await
            .map_err(imap_error_to_gateway)?;
    }

    Ok(MutationOutcome { cursor: None })
}

pub(crate) async fn destroy_message_by_imap(
    gateway: &LiveImapSmtpGateway,
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
        if gateway.discovery.capabilities.supports_uidplus() {
            expunge_imap_message_by_location(&gateway.config, &mailbox_name, location)
                .await
                .map_err(imap_error_to_gateway)?;
        } else {
            mark_imap_message_deleted_by_location(&gateway.config, &mailbox_name, location)
                .await
                .map_err(imap_error_to_gateway)?;
        }
    }

    Ok(MutationOutcome { cursor: None })
}
