use jmap_client::{email, identity, mailbox};
use posthaste_domain::{
    AccountId, GatewayError, Identity, MailGateway, MessageId, ReplyContext, SendMessageRequest,
};

use crate::compose::{
    addresses_to_recipients, prefix_subject, recipient_to_address, render_markdown,
};
use crate::live::{map_gateway_error, required_method_response, LiveJmapGateway};

/// Fetch the primary sender identity for an account via `Identity/get`.
///
/// @spec docs/L1-jmap#methods-used
/// @spec docs/L1-compose#composesession-interface
pub(crate) async fn fetch_identity(gateway: &LiveJmapGateway) -> Result<Identity, GatewayError> {
    let mut request = gateway.client().build();
    request.get_identity().properties([
        identity::Property::Id,
        identity::Property::Name,
        identity::Property::Email,
    ]);
    let mut response = gateway.send_request(request).await?;
    let mut identities = required_method_response(response.pop_method_response(), "Identity/get")?
        .unwrap_get_identity()
        .map_err(map_gateway_error)?
        .take_list();
    let identity = identities
        .pop()
        .ok_or_else(|| GatewayError::Rejected("no identity available".to_string()))?;

    Ok(Identity {
        id: identity.id().unwrap_or_default().to_string(),
        name: identity.name().unwrap_or_default().to_string(),
        email: identity.email().unwrap_or_default().to_string(),
    })
}

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
    let identity = fetch_send_identity(gateway, account_id).await?;
    let drafts_mailbox_id = gateway
        .fetch_mailbox_id_by_role(mailbox::Role::Drafts)
        .await?;
    let sent_mailbox_id = gateway
        .fetch_mailbox_id_by_role(mailbox::Role::Sent)
        .await?;
    let html_body = render_markdown(&request_data.body);

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
        email_obj.references(references.split_whitespace().collect::<Vec<_>>());
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

/// Resolve the sender identity for an account before composing or sending.
///
/// @spec docs/L1-jmap#methods-used
/// @spec docs/L1-compose#composesession-interface
async fn fetch_send_identity(
    gateway: &impl MailGateway,
    account_id: &AccountId,
) -> Result<Identity, GatewayError> {
    gateway.fetch_identity(account_id).await
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use posthaste_domain::{
        BlobId, FetchedBody, MailboxId, MutationOutcome, PushTransport, SetKeywordsCommand,
        SyncBatch, SyncCursor,
    };

    use super::*;

    struct RecordingGateway {
        seen_account_ids: Mutex<Vec<AccountId>>,
    }

    #[async_trait]
    impl MailGateway for RecordingGateway {
        async fn sync(
            &self,
            _account_id: &AccountId,
            _cursors: &[SyncCursor],
        ) -> Result<SyncBatch, GatewayError> {
            Err(GatewayError::Rejected("not implemented".to_string()))
        }

        async fn fetch_message_body(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
        ) -> Result<FetchedBody, GatewayError> {
            Err(GatewayError::Rejected("not implemented".to_string()))
        }

        async fn download_blob(
            &self,
            _account_id: &AccountId,
            _blob_id: &BlobId,
        ) -> Result<Vec<u8>, GatewayError> {
            Err(GatewayError::Rejected("not implemented".to_string()))
        }

        async fn set_keywords(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            _expected_state: Option<&str>,
            _command: &SetKeywordsCommand,
        ) -> Result<MutationOutcome, GatewayError> {
            Err(GatewayError::Rejected("not implemented".to_string()))
        }

        async fn replace_mailboxes(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            _expected_state: Option<&str>,
            _mailbox_ids: &[MailboxId],
        ) -> Result<MutationOutcome, GatewayError> {
            Err(GatewayError::Rejected("not implemented".to_string()))
        }

        async fn destroy_message(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            _expected_state: Option<&str>,
        ) -> Result<MutationOutcome, GatewayError> {
            Err(GatewayError::Rejected("not implemented".to_string()))
        }

        async fn fetch_identity(&self, account_id: &AccountId) -> Result<Identity, GatewayError> {
            self.seen_account_ids
                .lock()
                .expect("recording lock poisoned")
                .push(account_id.clone());
            Ok(Identity {
                id: "identity-1".to_string(),
                name: "Alice".to_string(),
                email: "alice@example.com".to_string(),
            })
        }

        async fn fetch_reply_context(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
        ) -> Result<ReplyContext, GatewayError> {
            Err(GatewayError::Rejected("not implemented".to_string()))
        }

        async fn send_message(
            &self,
            _account_id: &AccountId,
            _request: &SendMessageRequest,
        ) -> Result<(), GatewayError> {
            Err(GatewayError::Rejected("not implemented".to_string()))
        }

        fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
            vec![]
        }
    }

    #[tokio::test]
    async fn send_identity_lookup_uses_requested_account_id() {
        let gateway = RecordingGateway {
            seen_account_ids: Mutex::new(Vec::new()),
        };
        let requested_account_id = AccountId::from("secondary");

        let identity = fetch_send_identity(&gateway, &requested_account_id)
            .await
            .expect("identity lookup should succeed");

        assert_eq!(identity.email, "alice@example.com");
        assert_eq!(
            gateway
                .seen_account_ids
                .lock()
                .expect("recording lock poisoned")
                .as_slice(),
            &[requested_account_id]
        );
    }
}
