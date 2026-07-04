use jmap_client::mailbox;
use posthaste_domain_model::{AccountId, GatewayError, MessageId, SendMessageRequest};

use crate::compose::{recipient_to_address, render_markdown};
use crate::live::{map_gateway_error, required_method_response, LiveJmapGateway};
use crate::live_compose::attachments::upload_send_attachments;
use crate::live_compose::identity::fetch_draft_sender;

/// Persist a draft to the Drafts mailbox via `Email/set` create, returning the
/// created provider Email id.
///
/// Mirrors [`send_message`](super::send::send_message)'s `Email/set` create but
/// (a) sets the `$draft` keyword, (b) omits the `EmailSubmission/set`, and
/// (c) when `replace` is set, destroys the prior draft in the same `Email/set`
/// call -- JMAP emails are immutable, so an update is create-new + destroy-old.
///
/// @spec docs/L1-outbox#operation-model
/// @spec docs/L1-jmap#methods-used
pub(crate) async fn save_draft(
    gateway: &LiveJmapGateway,
    request_data: &SendMessageRequest,
    replace: Option<&MessageId>,
) -> Result<MessageId, GatewayError> {
    // A draft create carries no `identityId`, only the `from` address, so resolve
    // the sender tolerantly: a provider with an empty `Identity/get` must not
    // block saving a draft.
    let identity = fetch_draft_sender(gateway, request_data.from.as_ref()).await?;
    let drafts_mailbox_id = gateway
        .fetch_mailbox_id_by_role(mailbox::Role::Drafts)
        .await?;
    let html_body = render_markdown(&request_data.body);
    let uploaded_attachments = upload_send_attachments(gateway, &request_data.attachments).await?;

    let mut request = gateway.client().build();
    let email_set = request.set_email();
    {
        let email_obj = email_set.create();
        email_obj.mailbox_ids([drafts_mailbox_id.as_str()]);
        email_obj.keyword("$draft", true);
        // Your own draft is not "unread" to you (IMAP/JMAP convention: drafts
        // carry \Seen). Without it the draft syncs back as unread.
        email_obj.keyword("$seen", true);
        email_obj.from([(identity.name.as_str(), identity.email.as_str())]);
        // Stamp the stable draft identity so a resumed edit replaces this draft
        // in place across provider id rotation (read back on sync).
        if let Some(draft_id) = request_data
            .draft_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            email_obj.header(
                jmap_client::email::Header::as_text(posthaste_domain_model::DRAFT_ID_HEADER, false),
                jmap_client::email::HeaderValue::AsText(draft_id.to_string()),
            );
        }
        if !request_data.to.is_empty() {
            email_obj.to(request_data.to.iter().map(recipient_to_address));
        }
        if !request_data.cc.is_empty() {
            email_obj.cc(request_data.cc.iter().map(recipient_to_address));
        }
        if !request_data.bcc.is_empty() {
            email_obj.bcc(request_data.bcc.iter().map(recipient_to_address));
        }
        email_obj.subject(request_data.subject.as_str());
        email_obj.text_body(
            jmap_client::email::EmailBodyPart::new()
                .content_type("text/plain")
                .part_id("text_part"),
        );
        email_obj.body_value("text_part".to_string(), request_data.body.as_str());
        email_obj.html_body(
            jmap_client::email::EmailBodyPart::new()
                .content_type("text/html")
                .part_id("html_part"),
        );
        email_obj.body_value("html_part".to_string(), html_body.as_str());
        if let Some(in_reply_to) = &request_data.in_reply_to {
            email_obj.in_reply_to([in_reply_to.as_str()]);
        }
        if let Some(references) = &request_data.references {
            email_obj.references(references.split_whitespace());
        }
        for attachment in uploaded_attachments {
            email_obj.attachment(
                jmap_client::email::EmailBodyPart::new()
                    .blob_id(attachment.blob_id)
                    .name(attachment.filename)
                    .content_type(attachment.mime_type),
            );
        }
    }
    // The borrow of `email_set` from `create()` has ended; the old draft can now
    // be destroyed in the same `Email/set` call.
    if let Some(replace) = replace {
        email_set.destroy([replace.as_str()]);
    }

    let mut response = gateway.send_request(request).await?;
    let mut email_set_response =
        required_method_response(response.pop_method_response(), "Email/set create")?
            .unwrap_set_email()
            .map_err(map_gateway_error)?;
    let created = email_set_response
        .created("c0")
        .map_err(map_gateway_error)?;
    let new_id = created
        .id()
        .ok_or_else(|| GatewayError::Rejected("draft create returned no id".to_string()))?
        .to_string();
    Ok(MessageId::from(new_id))
}

/// Destroy a draft message from the Drafts mailbox via `Email/set` destroy.
///
/// Idempotent (D126): a `notFound` destroy means the draft is already gone —
/// counted as destroyed, so a redelivered send settlement (whose
/// consume-the-draft effect re-enqueues the delete) settles clean instead of
/// erroring.
///
/// @spec docs/L1-outbox#operation-model
/// @spec docs/L1-jmap#methods-used
/// @spec docs/eph/RFC-L2-drafts#3-decisions-proposed
pub(crate) async fn delete_draft(
    gateway: &LiveJmapGateway,
    _account_id: &AccountId,
    message_id: &MessageId,
    idempotent_redelivery: bool,
) -> Result<(), GatewayError> {
    let mut request = gateway.client().build();
    request.set_email().destroy([message_id.as_str()]);
    let mut response = gateway.send_request(request).await?;
    let mut email_set_response =
        required_method_response(response.pop_method_response(), "Email/set destroy")?
            .unwrap_set_email()
            .map_err(map_gateway_error)?;
    match email_set_response.destroyed(message_id.as_str()) {
        Ok(()) => Ok(()),
        // D133: a `notFound` is a benign already-gone ONLY for an idempotent
        // redelivery (the send-consume settlement effect re-enqueues the
        // delete). A user-initiated discard's `notFound` surfaces as a
        // retryable failure so the client reverts the optimistic fold + shows
        // the error, rather than silently "succeeding" (the M60 regression).
        Err(jmap_client::Error::Set(error))
            if idempotent_redelivery
                && matches!(
                    error.error(),
                    jmap_client::core::set::SetErrorType::NotFound
                ) =>
        {
            Ok(())
        }
        Err(error) => Err(map_gateway_error(error)),
    }
}
