use jmap_client::email;
use posthaste_domain::{
    format_forwarded_body, recipients_to_header, GatewayError, MessageId, ReplyContext,
};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

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
        email::Property::SentAt,
        email::Property::ReceivedAt,
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
    let plain_body = email
        .text_body()
        .and_then(|parts| parts.first())
        .and_then(|part| part.part_id())
        .and_then(|part_id| email.body_value(part_id))
        .map(|value| value.value().to_string());
    let quoted_body = plain_body.as_deref().map(quote_body);
    let original_from = email
        .from()
        .map(addresses_to_recipients)
        .unwrap_or_default();
    let original_to = email.to().map(addresses_to_recipients).unwrap_or_default();
    let to = original_from.clone();
    let cc = email.cc().map(addresses_to_recipients).unwrap_or_default();
    let subject = email.subject().unwrap_or("(no subject)");
    let date = email
        .sent_at()
        .or_else(|| email.received_at())
        .and_then(format_timestamp);
    let forwarded_body = Some(format_forwarded_body(
        recipients_to_header(&original_from).as_deref(),
        date.as_deref(),
        Some(subject),
        recipients_to_header(&original_to).as_deref(),
        plain_body.as_deref().unwrap_or_default(),
    ));
    Ok(ReplyContext {
        to,
        cc,
        reply_subject: prefix_subject("Re:", subject),
        forward_subject: prefix_subject("Fwd:", subject),
        quoted_body,
        forwarded_body,
        in_reply_to: email.message_id().and_then(|ids| ids.first()).cloned(),
        references: email.references().map(|refs| refs.join(" ")),
    })
}

fn quote_body(body: &str) -> String {
    body.lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_timestamp(timestamp: i64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp(timestamp)
        .ok()
        .and_then(|value| value.format(&Rfc3339).ok())
}
