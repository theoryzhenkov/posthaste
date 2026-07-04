use std::num::NonZeroU32;

use imap_client::imap_types::flag::Flag;
use posthaste_domain_model::RFC3339_EPOCH;
use posthaste_domain_service::imap_message_id;

use crate::build_smtp_message;
use crate::fetch::fetch_selected_mailbox_headers;
use crate::provider::ImapAdapterProviderProfile;

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
/// The returned [`MessageId`] is the SAME canonical id the next sync will derive
/// for the appended message, and its location is registered under that id at
/// save time (D128, closing the transient-twin wart). Immediately after the
/// APPEND we FETCH the new UID exactly as sync does — pulling Gmail `X-GM-MSGID`
/// metadata when the provider canonicalizes by it — and run the same provider
/// projection ([`ImapAdapterProviderProfile::project_headers`]) to obtain the id
/// and location sync would compute. That location is written through the
/// sync-owned location store, so sync's own upsert of the identical key is a
/// no-op and its delete-by-absence prune leaves it untouched: a save followed by
/// a sync yields exactly ONE Drafts row under a stable identity, never a
/// provider-canonical row surfacing as a duplicate beside an orphaned UID-based
/// one. For a UID-identity provider the projected id equals the plain
/// [`imap_message_id`] encoding (the pre-existing happy path); the Gmail case —
/// previously the documented limitation — now reconciles too.
///
/// When `replace` is set, the prior draft is deleted after the new one is
/// appended (append-then-delete preserves the old draft if the append fails);
/// the old version's location is pruned by sync once its UID vanishes.
///
/// The UID-based encoding remains the fallback id when the post-APPEND FETCH
/// yields no header (the draft was expunged out from under us between APPEND and
/// FETCH) or when no store is wired (the storeless gateway has no sync to agree
/// with).
///
/// @spec docs/L1-outbox#operation-model
/// @spec docs/eph/RFC-L2-drafts#3-decisions-proposed
pub(crate) async fn save_imap_draft(
    gateway: &LiveImapSmtpGateway,
    client: &mut ImapClient,
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
    // fetch on sync. Only draft saves stamp it; the send path never writes the
    // header (its `draft_id` names the draft to consume at settlement — D126).
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

    // UIDVALIDITY of the Drafts mailbox, matching the source sync uses to encode
    // ids (`selected.uid_validity` in `imap_header_message_record`).
    let selected = examine_selected_mailbox(client, &drafts_name)
        .await
        .map_err(imap_error_to_gateway)?;
    let uid_validity = selected.uid_validity;

    let uid = crate::timeout::with_deadline(
        "append",
        client.appenduid_or_fallback(drafts_name.as_str(), [Flag::Draft], &raw_message),
    )
    .await
    .map_err(imap_error_to_gateway)?
    .ok_or_else(|| {
        GatewayError::Rejected("IMAP APPEND did not yield a draft UID".to_string())
    })?;

    let uid_fallback_id = imap_message_id(&drafts_id, uid_validity, ImapUid(uid.get()));
    let new_message_id =
        register_saved_draft_location(gateway, client, &selected, account_id, uid, uid_fallback_id)
            .await?;

    if let Some(replace) = replace {
        delete_imap_draft(gateway, client, account_id, replace).await?;
    }

    Ok(new_message_id)
}

/// Resolve the canonical id + location the next sync will derive for a
/// just-APPENDed draft (`uid`) and register it under that id in the sync-owned
/// location store, returning the canonical id. Falls back to `uid_fallback_id`
/// (the plain [`imap_message_id`] encoding) when no store is wired or the FETCH
/// yields no header for the UID. See [`save_imap_draft`] for why this closes the
/// transient-twin wart.
async fn register_saved_draft_location(
    gateway: &LiveImapSmtpGateway,
    client: &mut ImapClient,
    selected: &posthaste_domain_model::ImapSelectedMailbox,
    account_id: &AccountId,
    uid: NonZeroU32,
    uid_fallback_id: MessageId,
) -> Result<MessageId, GatewayError> {
    let Some(store) = gateway.store.as_ref() else {
        return Ok(uid_fallback_id);
    };
    let capabilities = &gateway.discovery.capabilities;
    let updated_at = now_iso8601().map_err(GatewayError::Rejected)?;
    let headers = fetch_selected_mailbox_headers(
        client,
        selected,
        &[uid],
        capabilities.supports_condstore(),
        capabilities.supports_gmail_extensions(),
        updated_at,
    )
    .await
    .map_err(imap_error_to_gateway)?;
    // Run the SAME provider projection sync runs, so the canonical id (and the
    // location's own `message_id`) match sync's exactly — this is what makes the
    // registered location the identical key sync will re-observe.
    let projected =
        ImapAdapterProviderProfile::from_discovery(&gateway.discovery).project_headers(headers);
    let Some(header) = projected
        .into_iter()
        .find(|header| header.location.uid.0 == uid.get())
    else {
        return Ok(uid_fallback_id);
    };
    store
        .put_imap_message_location(account_id, &header.location)
        .map_err(store_error_to_gateway)?;
    Ok(header.location.message_id)
}

/// Delete a draft message from the Drafts mailbox.
///
/// Prefers the synced store location (authoritative, and the only way to locate
/// provider-canonical ids); falls back to decoding our own UID-based message id
/// for a draft that has been appended but not yet re-synced (the
/// create-then-edit-while-offline case).
///
/// Idempotent (D126): a draft that is already gone — the UID vanished from the
/// mailbox, or the id resolves to no location at all — counts as deleted, so a
/// redelivered send settlement (whose consume-the-draft effect re-enqueues the
/// delete) settles clean instead of erroring. Removal goes through
/// [`remove_imap_message_from_mailbox`], the UID-scoped expunge helper shared
/// with the archive path (UID EXPUNGE under UIDPLUS, mark-`\Deleted` fallback,
/// `MissingFetchData` tolerated as already-removed).
///
/// @spec docs/L1-outbox#operation-model
/// @spec docs/eph/RFC-L2-drafts#3-decisions-proposed
pub(crate) async fn delete_imap_draft(
    gateway: &LiveImapSmtpGateway,
    client: &mut ImapClient,
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
                remove_imap_message_from_mailbox(gateway, client, &mailbox_name, location).await?;
            }
            return Ok(());
        }
    }

    // Not yet synced: decode the UID-based id we returned from `save_imap_draft`.
    // A provider-canonical id (e.g. Gmail's msgid form) with no store location
    // names a message that is no longer in the projection — already deleted and
    // synced away — so deletion is an idempotent no-op, not an error.
    let Some(location) = decode_imap_message_location(message_id) else {
        ph_warn!(
            events::IMAP_DRAFT_DELETE_ALREADY_GONE,
            draft_id = %message_id,
            "draft to delete has no IMAP location; treating as already deleted"
        );
        return Ok(());
    };
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
    remove_imap_message_from_mailbox(gateway, client, &mailbox_name, &location).await
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
