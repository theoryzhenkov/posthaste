use imap_client::imap_types::flag::Flag;
use posthaste_domain_model::RFC3339_EPOCH;
use posthaste_domain_service::imap_message_id;

use crate::build_smtp_message;

use super::*;

/// Resolve the account's selectable Drafts mailbox from discovery.
fn drafts_mailbox(gateway: &LiveImapSmtpGateway) -> Result<&DiscoveredImapMailbox, GatewayError> {
    gateway
        .discovery
        .mailboxes
        .iter()
        .find(|mailbox| mailbox.selectable && mailbox.role == Some("drafts"))
        .ok_or_else(|| {
            GatewayError::Rejected("no selectable IMAP Drafts mailbox was discovered".to_string())
        })
}

/// Persist a draft by APPENDing it to the Drafts mailbox with the `\Draft` flag.
///
/// The returned [`MessageId`] is encoded with [`imap_message_id`] exactly as the
/// IMAP sync path encodes message ids for UID-identity providers, so the
/// runtime's draft alias reconciles to the real message on the next sync.
///
/// When `replace` is set, the prior draft is deleted after the new one is
/// appended (append-then-delete preserves the old draft if the append fails).
///
/// LIMITATION: providers that canonicalize message ids by a provider key rather
/// than UIDVALIDITY/UID (currently only Gmail, via `X-GM-MSGID`) will sync the
/// appended draft under a different canonical id than the UID-based id returned
/// here, so the alias cannot reconcile and an edited draft may leave a duplicate
/// in the Drafts mailbox. This is acceptable for the JMAP-first beta where IMAP
/// support targets standard UID-identity providers (Generic/Outlook/iCloud).
///
/// @spec docs/L1-outbox#operation-model
pub(crate) async fn save_imap_draft(
    gateway: &LiveImapSmtpGateway,
    config: &ImapConnectionConfig,
    account_id: &AccountId,
    request: &SendMessageRequest,
    replace: Option<&MessageId>,
) -> Result<MessageId, GatewayError> {
    let drafts = drafts_mailbox(gateway)?;
    let drafts_name = drafts.name.clone();
    let drafts_id = drafts.id.clone();

    let smtp_config = &gateway.smtp_config;
    let mut raw_message = build_smtp_message(smtp_config, request, None)
        .map_err(imap_error_to_gateway)?
        .formatted();
    // Stamp the stable draft identity as a top-level header so a resumed edit
    // replaces this draft in place. Prepended to the existing header block
    // (header order is irrelevant) so it round-trips through the RFC822 header
    // fetch on sync. Only drafts carry it; sends never set `draft_id`.
    if let Some(draft_id) = request
        .draft_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        let header_line = format!("{}: {}\r\n", posthaste_domain_model::DRAFT_ID_HEADER, draft_id);
        let mut prefixed = header_line.into_bytes();
        prefixed.extend_from_slice(&raw_message);
        raw_message = prefixed;
    }

    let mut client = connect_authenticated_client(config)
        .await
        .map_err(imap_error_to_gateway)?;
    client
        .refresh_capabilities()
        .await
        .map_err(|error| imap_error_to_gateway(ImapAdapterError::from(error)))?;

    // UIDVALIDITY of the Drafts mailbox, matching the source sync uses to encode
    // ids (`selected.uid_validity` in `imap_header_message_record`).
    let selected = examine_selected_mailbox(&mut client, &drafts_name)
        .await
        .map_err(imap_error_to_gateway)?;
    let uid_validity = selected.uid_validity;

    let uid = client
        .appenduid_or_fallback(drafts_name.as_str(), [Flag::Draft], &raw_message)
        .await
        .map_err(|error| imap_error_to_gateway(ImapAdapterError::from(error)))?
        .ok_or_else(|| {
            GatewayError::Rejected("IMAP APPEND did not yield a draft UID".to_string())
        })?;

    let new_message_id = imap_message_id(&drafts_id, uid_validity, ImapUid(uid.get()));

    if let Some(replace) = replace {
        delete_imap_draft(gateway, config, account_id, replace).await?;
    }

    Ok(new_message_id)
}

/// Delete a draft message from the Drafts mailbox.
///
/// Prefers the synced store location (authoritative, and the only way to locate
/// provider-canonical ids); falls back to decoding our own UID-based message id
/// for a draft that has been appended but not yet re-synced (the
/// create-then-edit-while-offline case).
///
/// @spec docs/L1-outbox#operation-model
pub(crate) async fn delete_imap_draft(
    gateway: &LiveImapSmtpGateway,
    config: &ImapConnectionConfig,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<(), GatewayError> {
    if let Some(store) = gateway.store.as_ref() {
        let locations = store
            .list_imap_message_locations(account_id, message_id)
            .map_err(store_error_to_gateway)?;
        if !locations.is_empty() {
            for location in &locations {
                let mailbox_name = gateway.mailbox_name_for_id(account_id, &location.mailbox_id)?;
                delete_draft_by_location(gateway, config, &mailbox_name, location).await?;
            }
            return Ok(());
        }
    }

    // Not yet synced: decode the UID-based id we returned from `save_imap_draft`.
    let location = decode_imap_message_location(message_id).ok_or_else(|| {
        GatewayError::Rejected(format!("cannot locate draft {message_id} for deletion"))
    })?;
    let mailbox_name = gateway
        .discovery
        .mailboxes
        .iter()
        .find(|mailbox| mailbox.id == location.mailbox_id)
        .map(|mailbox| mailbox.name.clone())
        .ok_or_else(|| {
            GatewayError::Rejected(format!(
                "unknown IMAP mailbox for draft {message_id} deletion"
            ))
        })?;
    delete_draft_by_location(gateway, config, &mailbox_name, &location).await
}

/// Capability-aware deletion: UID EXPUNGE under UIDPLUS, otherwise mark `\Deleted`.
async fn delete_draft_by_location(
    gateway: &LiveImapSmtpGateway,
    config: &ImapConnectionConfig,
    mailbox_name: &str,
    location: &ImapMessageLocation,
) -> Result<(), GatewayError> {
    if gateway.discovery.capabilities.supports_uidplus() {
        expunge_imap_message_by_location(config, mailbox_name, location)
            .await
            .map_err(imap_error_to_gateway)?;
    } else {
        mark_imap_message_deleted_by_location(config, mailbox_name, location)
            .await
            .map_err(imap_error_to_gateway)?;
    }
    Ok(())
}

/// Decode a UID-identity message id (`imap:{uidvalidity}:{uid}:{hex(mailboxId)}`)
/// produced by [`imap_message_id`] back into a location. Returns `None` for
/// provider-canonical ids (e.g. `imap:gmail:msgid:...`), whose UID is unknown.
fn decode_imap_message_location(message_id: &MessageId) -> Option<ImapMessageLocation> {
    let rest = message_id.as_str().strip_prefix("imap:")?;
    let mut parts = rest.splitn(3, ':');
    let uid_validity: u32 = parts.next()?.parse().ok()?;
    let uid: u32 = parts.next()?.parse().ok()?;
    let mailbox_id = String::from_utf8(hex::decode(parts.next()?).ok()?).ok()?;
    Some(ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: MailboxId(mailbox_id),
        uid_validity: ImapUidValidity(uid_validity),
        uid: ImapUid(uid),
        modseq: None,
        updated_at: RFC3339_EPOCH.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imap_mailbox_id;

    #[test]
    fn decodes_the_id_imap_message_id_encodes() {
        let mailbox_id = imap_mailbox_id("Drafts");
        let id = imap_message_id(&mailbox_id, ImapUidValidity(42), ImapUid(7));

        let location = decode_imap_message_location(&id).expect("id should decode");
        assert_eq!(location.mailbox_id, mailbox_id);
        assert_eq!(location.uid_validity, ImapUidValidity(42));
        assert_eq!(location.uid, ImapUid(7));
        assert_eq!(location.message_id, id);
    }

    #[test]
    fn rejects_provider_canonical_ids() {
        // Gmail-canonical ids carry no UID and must not decode to a location.
        assert!(decode_imap_message_location(&MessageId::from("imap:gmail:msgid:12345")).is_none());
    }
}
