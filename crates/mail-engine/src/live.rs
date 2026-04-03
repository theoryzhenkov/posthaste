use std::sync::Arc;

use async_trait::async_trait;
use jmap_client::client::Client;
use jmap_client::core::error::MethodErrorType;
use jmap_client::{email, identity};
use mail_domain::{
    now_iso8601 as domain_now_iso8601, synthesize_plain_text_raw_mime, AccountId, FetchedBody,
    GatewayError, Identity, MailGateway, MailboxId, MessageId, MutationOutcome, PushTransport,
    ReplyContext, SendMessageRequest, SetKeywordsCommand, SyncBatch,
    SyncCursor, SyncObject,
};

use tracing::{debug, info, instrument};

use crate::compose::{addresses_to_recipients, prefix_subject, recipient_to_address, render_markdown};
use crate::sync::{fetch_email_sync, fetch_mailbox_sync};

/// Discover and connect to a JMAP server, returning a configured client.
///
/// Performs session discovery via `.well-known/jmap`, authenticates with
/// basic credentials, and follows redirects scoped to the server's host.
///
/// @spec spec/L1-jmap#session
/// @spec spec/L1-jmap#authentication
#[instrument(skip(password))]
pub async fn connect_jmap_client(
    url: &str,
    username: &str,
    password: &str,
) -> Result<Arc<Client>, GatewayError> {
    debug!("connecting to JMAP server");
    let host = url::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(String::from))
        .unwrap_or_default();
    let client = Client::new()
        .credentials((username, password))
        .follow_redirects([host])
        .connect(url)
        .await
        .map_err(map_gateway_error)?;
    let session = client.session();
    let ws_url = session
        .websocket_capabilities()
        .map(|caps| caps.url().to_string());
    let ws_push = session
        .websocket_capabilities()
        .map(|caps| caps.supports_push())
        .unwrap_or(false);
    info!(
        api_url = session.api_url(),
        event_source_url = session.event_source_url(),
        ws_url = ws_url.as_deref(),
        ws_push,
        "JMAP session established"
    );
    Ok(Arc::new(client))
}

/// Production `MailGateway` backed by a live JMAP server connection.
///
/// Holds an authenticated `jmap_client::Client` and, when the server
/// advertises WebSocket capability, a `SharedWsConnection` used for
/// interactive API calls and push notifications.
///
/// @spec spec/L1-jmap#session
/// @spec spec/L2-transport#transport-negotiation
pub struct LiveJmapGateway {
    client: Arc<Client>,
    ws: Option<Arc<crate::ws_connection::SharedWsConnection>>,
}

impl LiveJmapGateway {
    /// Wrap an already-connected client, opening a WebSocket if the server supports it.
    ///
    /// @spec spec/L2-transport#transport-negotiation
    pub fn from_client(client: Arc<Client>) -> Self {
        let ws = if client.session().websocket_capabilities().is_some() {
            debug!("WebSocket capability available, creating shared connection");
            Some(Arc::new(crate::ws_connection::SharedWsConnection::new(
                client.clone(),
            )))
        } else {
            debug!("WebSocket capability not advertised, WS transport disabled");
            None
        };
        Self { client, ws }
    }

    /// Discover, authenticate, and construct a gateway in one step.
    ///
    /// @spec spec/L1-jmap#session
    pub async fn connect(url: &str, username: &str, password: &str) -> Result<Self, GatewayError> {
        let client = connect_jmap_client(url, username, password).await?;
        Ok(Self::from_client(client))
    }

    /// Borrow the underlying JMAP client for direct access.
    pub fn client(&self) -> &Arc<Client> {
        &self.client
    }

    /// Route a JMAP request through WebSocket if connected, HTTP otherwise.
    ///
    /// Currently only interactive methods (mutations, body fetch, identity,
    /// compose) use this. Sync helpers still use HTTP convenience methods
    /// directly. TODO: once jmap-client supports transparent WS routing in
    /// Client::send(), all paths will use WS automatically and this method
    /// can be removed.
    ///
    /// @spec spec/L2-transport#jmaptransport
    /// @spec spec/L2-transport#http-fallback
    async fn send_request(
        &self,
        request: jmap_client::core::request::Request<'_>,
    ) -> Result<
        jmap_client::core::response::Response<jmap_client::core::response::TaggedMethodResponse>,
        GatewayError,
    > {
        if let Some(ref ws) = self.ws {
            if ws.is_connected().await {
                return ws.send(request).await;
            }
        }
        request.send().await.map_err(map_gateway_error)
    }
}

/// Build a `MutationOutcome` with a message-type sync cursor from the server's new state string.
///
/// @spec spec/L1-jmap#core-types
/// @spec spec/L1-sync#state-management
fn message_mutation_outcome(state: String) -> Result<MutationOutcome, GatewayError> {
    Ok(MutationOutcome {
        cursor: Some(SyncCursor {
            object_type: SyncObject::Message,
            state,
            updated_at: domain_now_iso8601().map_err(GatewayError::Rejected)?,
        }),
    })
}

/// Resolve the sender identity for an account before composing or sending.
///
/// @spec spec/L1-jmap#methods-used
/// @spec spec/L1-compose#composesession-interface
async fn fetch_send_identity(
    gateway: &impl MailGateway,
    account_id: &AccountId,
) -> Result<Identity, GatewayError> {
    gateway.fetch_identity(account_id).await
}

/// @spec spec/L1-jmap#method-calls
/// @spec spec/L1-sync#sync-loop
/// @spec spec/L2-transport#gateway-unchanged
#[async_trait]
impl MailGateway for LiveJmapGateway {
    /// Perform a full sync cycle: mailbox state then email state.
    ///
    /// Falls back from delta to full sync on `cannotCalculateChanges`.
    ///
    /// @spec spec/L1-sync#sync-loop
    /// @spec spec/L1-sync#state-management
    async fn sync(
        &self,
        _account_id: &AccountId,
        cursors: &[SyncCursor],
    ) -> Result<SyncBatch, GatewayError> {
        let mailbox_cursor = cursors
            .iter()
            .find(|cursor| cursor.object_type == SyncObject::Mailbox)
            .map(|cursor| cursor.state.as_str());
        let message_cursor = cursors
            .iter()
            .find(|cursor| cursor.object_type == SyncObject::Message)
            .map(|cursor| cursor.state.as_str());

        debug!(
            has_mailbox_state = mailbox_cursor.is_some(),
            has_message_state = message_cursor.is_some(),
            "fetching JMAP changes"
        );
        let mailbox_sync = fetch_mailbox_sync(&self.client, mailbox_cursor).await?;
        let email_sync = fetch_email_sync(&self.client, message_cursor).await?;
        debug!(
            mailboxes = mailbox_sync.mailboxes.len(),
            messages = email_sync.messages.len(),
            deleted_mailboxes = mailbox_sync.deleted_mailbox_ids.len(),
            deleted_messages = email_sync.deleted_message_ids.len(),
            replace_all_mailboxes = mailbox_sync.replace_all_mailboxes,
            "JMAP sync batch fetched"
        );

        Ok(SyncBatch {
            mailboxes: mailbox_sync.mailboxes,
            messages: email_sync.messages,
            deleted_mailbox_ids: mailbox_sync.deleted_mailbox_ids,
            deleted_message_ids: email_sync.deleted_message_ids,
            replace_all_mailboxes: mailbox_sync.replace_all_mailboxes,
            cursors: vec![mailbox_sync.cursor, email_sync.cursor],
        })
    }

    /// Lazily fetch the body content of a single message via `Email/get`.
    ///
    /// Bodies are not synced during metadata sync; they are fetched on first
    /// view and cached locally.
    ///
    /// @spec spec/L1-sync#sync-granularity
    /// @spec spec/L1-jmap#methods-used
    async fn fetch_message_body(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError> {
        let mut request = self.client.build();
        let get_request = request.get_email().ids([message_id.as_str()]).properties([
            email::Property::Id,
            email::Property::BodyValues,
            email::Property::HtmlBody,
            email::Property::TextBody,
        ]);
        get_request
            .arguments()
            .body_properties([email::BodyProperty::PartId, email::BodyProperty::Type])
            .fetch_all_body_values(true);

        let mut emails = self
            .send_request(request)
            .await?
            .pop_method_response()
            .unwrap()
            .unwrap_get_email()
            .map_err(map_gateway_error)?
            .take_list();
        let email = emails
            .pop()
            .ok_or_else(|| GatewayError::Rejected("message not found".to_string()))?;

        let body_html = email.html_body().and_then(|parts| {
            parts
                .first()
                .and_then(|part| part.part_id())
                .and_then(|part_id| email.body_value(part_id))
                .map(|value| value.value().to_string())
        });
        let body_text = email.text_body().and_then(|parts| {
            parts
                .first()
                .and_then(|part| part.part_id())
                .and_then(|part_id| email.body_value(part_id))
                .map(|value| value.value().to_string())
        });
        let from_header = email
            .from()
            .and_then(|addresses| addresses.first())
            .map(|address| address.email().to_string())
            .unwrap_or_else(|| "unknown@example.invalid".to_string());
        let raw_mime = synthesize_plain_text_raw_mime(
            from_header.as_str(),
            email.subject().unwrap_or("(no subject)"),
            body_text.as_deref(),
        );

        Ok(FetchedBody {
            body_html,
            body_text,
            raw_mime: Some(raw_mime),
        })
    }

    /// Add or remove keywords (flags) on a message via `Email/set`.
    ///
    /// Uses `ifInState` for optimistic concurrency when `expected_state` is provided.
    ///
    /// @spec spec/L1-jmap#methods-used
    /// @spec spec/L1-sync#conflict-model
    async fn set_keywords(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError> {
        let mut request = self.client.build();
        let set = request.set_email();
        if let Some(expected_state) = expected_state {
            set.if_in_state(expected_state);
        }
        let update = set.update(message_id.as_str());
        for keyword in &command.add {
            update.keyword(keyword.as_str(), true);
        }
        for keyword in &command.remove {
            update.keyword(keyword.as_str(), false);
        }
        let response = self
            .send_request(request)
            .await?
            .pop_method_response()
            .unwrap()
            .unwrap_set_email()
            .map_err(map_gateway_error)?;
        message_mutation_outcome(response.new_state().to_string())
    }

    /// Replace a message's mailbox membership via `Email/set`.
    ///
    /// Used for move and archive operations. Supports optimistic concurrency.
    ///
    /// @spec spec/L1-jmap#methods-used
    /// @spec spec/L1-sync#conflict-model
    async fn replace_mailboxes(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError> {
        let mut request = self.client.build();
        let set = request.set_email();
        if let Some(expected_state) = expected_state {
            set.if_in_state(expected_state);
        }
        set.update(message_id.as_str())
            .mailbox_ids(mailbox_ids.iter().map(MailboxId::as_str));
        let response = self
            .send_request(request)
            .await?
            .pop_method_response()
            .unwrap()
            .unwrap_set_email()
            .map_err(map_gateway_error)?;
        message_mutation_outcome(response.new_state().to_string())
    }

    /// Permanently destroy a message via `Email/set`.
    ///
    /// @spec spec/L1-jmap#methods-used
    /// @spec spec/L1-sync#conflict-model
    async fn destroy_message(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError> {
        let mut request = self.client.build();
        let set = request.set_email();
        if let Some(expected_state) = expected_state {
            set.if_in_state(expected_state);
        }
        set.destroy([message_id.as_str()]);
        let response = self
            .send_request(request)
            .await?
            .pop_method_response()
            .unwrap()
            .unwrap_set_email()
            .map_err(map_gateway_error)?;
        message_mutation_outcome(response.new_state().to_string())
    }

    /// Fetch the primary sender identity for an account via `Identity/get`.
    ///
    /// @spec spec/L1-jmap#methods-used
    /// @spec spec/L1-compose#composesession-interface
    async fn fetch_identity(&self, _account_id: &AccountId) -> Result<Identity, GatewayError> {
        let mut request = self.client.build();
        request.get_identity().properties([
            identity::Property::Id,
            identity::Property::Name,
            identity::Property::Email,
        ]);
        let mut identities = self
            .send_request(request)
            .await?
            .pop_method_response()
            .unwrap()
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
    /// @spec spec/L1-compose#reply-quoting
    /// @spec spec/L1-compose#forward-quoting
    async fn fetch_reply_context(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<ReplyContext, GatewayError> {
        let mut request = self.client.build();
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

        let mut emails = self
            .send_request(request)
            .await?
            .pop_method_response()
            .unwrap()
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
    /// @spec spec/L1-compose#mime-structure
    /// @spec spec/L1-jmap#methods-used
    async fn send_message(
        &self,
        account_id: &AccountId,
        request_data: &SendMessageRequest,
    ) -> Result<(), GatewayError> {
        let identity = fetch_send_identity(self, account_id).await?;
        let html_body = render_markdown(&request_data.body);

        let mut request = self.client.build();
        let email_obj = request.set_email().create();
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

        let submission = request.set_email_submission().create();
        submission.email_id("#c0");
        submission.identity_id(identity.id.as_str());
        self.send_request(request).await?;
        Ok(())
    }

    /// Return available push transports, preferring WebSocket over SSE.
    ///
    /// @spec spec/L2-transport#pushtransport
    fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
        let mut transports: Vec<Box<dyn PushTransport>> = Vec::new();
        if let Some(ref ws) = self.ws {
            transports.push(Box::new(crate::push_ws::WsPushTransport::new(ws.clone())));
        }
        transports.push(Box::new(crate::push_sse::SsePushTransport::new(
            self.client.clone(),
        )));
        transports
    }
}

/// Map `jmap_client::Error` into the typed `GatewayError` enum.
///
/// Distinguishes auth errors (401), state mismatches, `cannotCalculateChanges`,
/// and generic network/method errors.
///
/// @spec spec/L1-jmap#error-model
pub(crate) fn map_gateway_error(error: jmap_client::Error) -> GatewayError {
    match error {
        jmap_client::Error::Problem(problem) => {
            if problem.status == Some(401) {
                GatewayError::Auth
            } else {
                GatewayError::Network(problem.to_string())
            }
        }
        jmap_client::Error::Method(method) => match method.p_type {
            MethodErrorType::StateMismatch => GatewayError::StateMismatch,
            MethodErrorType::CannotCalculateChanges => GatewayError::CannotCalculateChanges,
            _ => GatewayError::Rejected(format!("{:?}", method.p_type)),
        },
        other => GatewayError::Network(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use mail_domain::{FetchedBody, PushTransport, SetKeywordsCommand, SyncBatch};

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

        async fn fetch_identity(
            &self,
            account_id: &AccountId,
        ) -> Result<Identity, GatewayError> {
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

    #[test]
    fn message_mutation_outcome_wraps_message_cursor() {
        let outcome =
            super::message_mutation_outcome("message-9".to_string()).expect("cursor should build");
        let cursor = outcome.cursor.expect("cursor should be present");
        assert_eq!(cursor.object_type, SyncObject::Message);
        assert_eq!(cursor.state, "message-9");
        assert!(!cursor.updated_at.is_empty());
    }
}
