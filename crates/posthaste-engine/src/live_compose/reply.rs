use jmap_client::email;
use posthaste_domain::{GatewayError, MessageId, ReplyContext};

use crate::compose::{addresses_to_recipients, prefix_subject};
use crate::live::{map_gateway_error, required_method_response, LiveJmapGateway};

/// Fetch the original message metadata needed for reply/forward composition.
///
/// Retrieves subject, sender, recipients, threading headers, and quoted
/// body text. The body is `>` prefixed for reply quoting.
///
/// @spec docs/L1-compose#reply-quoting
/// @spec docs/L1-compose#forward-quoting
pub(crate) async fn fetch_reply_context(
    gateway: &LiveJmapGateway,
    message_id: &MessageId,
) -> Result<ReplyContext, GatewayError> {
    let mut request = gateway.client().build();
    let get_request = request.get_email().ids([message_id.as_str()]).properties([
        email::Property::Id,
        email::Property::Subject,
        email::Property::From,
        email::Property::To,
        email::Property::Cc,
        email::Property::MessageId,
        email::Property::References,
        email::Property::InReplyTo,
        email::Property::TextBody,
        email::Property::BodyValues,
    ]);
    get_request
        .arguments()
        .body_properties([email::BodyProperty::PartId, email::BodyProperty::Type])
        .fetch_all_body_values(true);

    let mut response = gateway.send_request(request).await?;
    let mut emails = required_method_response(response.pop_method_response(), "Email/get")?
        .unwrap_get_email()
        .map_err(map_gateway_error)?
        .take_list();
    let email = emails
        .pop()
        .ok_or_else(|| GatewayError::Rejected("message not found".to_string()))?;
    let quoted_body = email
        .text_body()
        .and_then(|parts| parts.first())
        .and_then(|part| part.part_id())
        .and_then(|part_id| email.body_value(part_id))
        .map(|value| {
            value
                .value()
                .lines()
                .map(|line| format!("> {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        });
    let to = email
        .from()
        .map(addresses_to_recipients)
        .unwrap_or_default();
    let cc = email.cc().map(addresses_to_recipients).unwrap_or_default();
    let subject = email.subject().unwrap_or("(no subject)");
    Ok(ReplyContext {
        to,
        cc,
        reply_subject: prefix_subject("Re:", subject),
        forward_subject: prefix_subject("Fwd:", subject),
        quoted_body,
        in_reply_to: email.message_id().and_then(|ids| ids.first()).cloned(),
        references: email.references().map(|refs| refs.join(" ")),
    })
}
