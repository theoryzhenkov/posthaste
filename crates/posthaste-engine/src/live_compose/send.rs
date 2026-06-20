use jmap_client::mailbox;
use posthaste_domain::{AccountId, GatewayError, SendMessageRequest};

use crate::compose::{recipient_to_address, render_markdown};
use crate::live::{map_gateway_error, required_method_response, LiveJmapGateway};
use crate::live_compose::attachments::upload_send_attachments;
use crate::live_compose::identity::fetch_send_identity;

/// Send a message via `Email/set` + `EmailSubmission/set` in a single JMAP request.
///
/// Renders the Markdown body to HTML and constructs a multipart/alternative
/// MIME structure. The server handles Sent folder placement.
///
/// @spec docs/L1-compose#mime-structure
/// @spec docs/L1-jmap#methods-used
pub(crate) async fn send_message(
    gateway: &LiveJmapGateway,
    account_id: &AccountId,
    request_data: &SendMessageRequest,
) -> Result<(), GatewayError> {
    let identity = fetch_send_identity(gateway, request_data.from.as_ref()).await?;
    let drafts_mailbox_id = gateway
        .fetch_mailbox_id_by_role(mailbox::Role::Drafts)
        .await?;
    let sent_mailbox_id = gateway
        .fetch_mailbox_id_by_role(mailbox::Role::Sent)
        .await?;
    let html_body = render_markdown(&request_data.body);
    let uploaded_attachments =
        upload_send_attachments(gateway, account_id, &request_data.attachments).await?;

    let mut request = gateway.client().build();
    let email_obj = request.set_email().create();
    email_obj.mailbox_ids([drafts_mailbox_id.as_str()]);
    email_obj.from([(identity.name.as_str(), identity.email.as_str())]);
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

    let submission_set = request.set_email_submission();
    let submission = submission_set.create();
    submission.email_id("#c0");
    submission.identity_id(identity.id.as_str());
    submission_set
        .arguments()
        .on_success_update_email("c0")
        .mailbox_id(drafts_mailbox_id.as_str(), false)
        .mailbox_id(sent_mailbox_id.as_str(), true);
    let response = gateway.send_request(request).await?;
    let mut responses = response.unwrap_method_responses();
    let mut email_set = required_method_response(
        (!responses.is_empty()).then(|| responses.remove(0)),
        "Email/set create",
    )?
    .unwrap_set_email()
    .map_err(map_gateway_error)?;
    email_set.created("c0").map_err(map_gateway_error)?;

    let mut submission_set = required_method_response(
        (!responses.is_empty()).then(|| responses.remove(0)),
        "EmailSubmission/set create",
    )?
    .unwrap_set_email_submission()
    .map_err(map_gateway_error)?;
    submission_set.created("c0").map_err(map_gateway_error)?;

    let sent_update = required_method_response(
        (!responses.is_empty()).then(|| responses.remove(0)),
        "Email/set sent update",
    )?
    .unwrap_set_email()
    .map_err(map_gateway_error)?;
    sent_update
        .unwrap_update_errors()
        .map_err(map_gateway_error)?;
    Ok(())
}
