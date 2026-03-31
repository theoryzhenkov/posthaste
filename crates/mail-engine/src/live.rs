use async_trait::async_trait;
use futures_util::StreamExt;
use jmap_client::client::Client;
use jmap_client::core::error::MethodErrorType;
use jmap_client::{email, identity, mailbox};
use mail_domain::{
    AccountId, BlobId, FetchedBody, GatewayError, Identity, MailGateway, MailboxId, MailboxRecord,
    MessageId, MessageRecord, PushNotification, PushStream, Recipient, ReplyContext,
    SendMessageRequest, SetKeywordsCommand, SyncBatch, SyncCursor, SyncObject,
};
use pulldown_cmark::{html, Options, Parser};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub struct LiveJmapGateway {
    client: Client,
}

impl LiveJmapGateway {
    pub async fn connect(url: &str, username: &str, password: &str) -> Result<Self, GatewayError> {
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
        Ok(Self { client })
    }
}

#[async_trait]
impl MailGateway for LiveJmapGateway {
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

        let mailbox_sync = fetch_mailbox_sync(&self.client, mailbox_cursor).await?;
        let email_sync = fetch_email_sync(&self.client, message_cursor).await?;

        Ok(SyncBatch {
            mailboxes: mailbox_sync.mailboxes,
            messages: email_sync.messages,
            deleted_mailbox_ids: mailbox_sync.deleted_mailbox_ids,
            deleted_message_ids: email_sync.deleted_message_ids,
            replace_all_mailboxes: mailbox_sync.replace_all_mailboxes,
            cursors: vec![mailbox_sync.cursor, email_sync.cursor],
        })
    }

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

        let mut emails = request
            .send_get_email()
            .await
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
        let raw_mime = synthesize_raw_mime(
            email.subject().unwrap_or("(no subject)"),
            email
                .from()
                .and_then(|addresses| addresses.first())
                .map(|address| address.email().to_string())
                .unwrap_or_else(|| "unknown@example.invalid".to_string())
                .as_str(),
            body_text.as_deref(),
        );

        Ok(FetchedBody {
            body_html,
            body_text,
            raw_mime: Some(raw_mime),
        })
    }

    async fn set_keywords(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        command: &SetKeywordsCommand,
    ) -> Result<(), GatewayError> {
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
        request.send_set_email().await.map_err(map_gateway_error)?;
        Ok(())
    }

    async fn replace_mailboxes(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<(), GatewayError> {
        let mut request = self.client.build();
        let set = request.set_email();
        if let Some(expected_state) = expected_state {
            set.if_in_state(expected_state);
        }
        set.update(message_id.as_str())
            .mailbox_ids(mailbox_ids.iter().map(MailboxId::as_str));
        request.send_set_email().await.map_err(map_gateway_error)?;
        Ok(())
    }

    async fn destroy_message(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
    ) -> Result<(), GatewayError> {
        let mut request = self.client.build();
        let set = request.set_email();
        if let Some(expected_state) = expected_state {
            set.if_in_state(expected_state);
        }
        set.destroy([message_id.as_str()]);
        request.send_set_email().await.map_err(map_gateway_error)?;
        Ok(())
    }

    async fn fetch_identity(&self, _account_id: &AccountId) -> Result<Identity, GatewayError> {
        let mut request = self.client.build();
        request.get_identity().properties([
            identity::Property::Id,
            identity::Property::Name,
            identity::Property::Email,
        ]);
        let mut identities = request
            .send_get_identity()
            .await
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

        let mut emails = request
            .send_get_email()
            .await
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

    async fn send_message(
        &self,
        _account_id: &AccountId,
        request_data: &SendMessageRequest,
    ) -> Result<(), GatewayError> {
        let identity = self.fetch_identity(&AccountId::from("primary")).await?;
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
        request.send().await.map_err(map_gateway_error)?;
        Ok(())
    }

    async fn open_push_stream(
        &self,
        account_id: &AccountId,
        last_event_id: Option<&str>,
    ) -> Result<Option<PushStream>, GatewayError> {
        let stream = self
            .client
            .event_source(
                [
                    jmap_client::DataType::Email,
                    jmap_client::DataType::Mailbox,
                    jmap_client::DataType::EmailDelivery,
                    jmap_client::DataType::EmailSubmission,
                ]
                .into_iter()
                .collect::<Vec<_>>()
                .into_iter()
                .into(),
                false,
                Some(60),
                last_event_id,
            )
            .await
            .map_err(map_gateway_error)?;
        let account_id = account_id.clone();
        Ok(Some(Box::pin(stream.filter_map(move |event| {
            let account_id = account_id.clone();
            async move {
                match event {
                    Ok(jmap_client::event_source::PushNotification::StateChange(changes)) => {
                        let changed = changes
                            .changes(account_id.as_str())
                            .map(|entries| {
                                entries
                                    .map(|(data_type, _)| data_type.to_string())
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        Some(Ok(PushNotification {
                            account_id,
                            changed,
                            received_at: now_iso8601().ok()?,
                            checkpoint: changes.id().map(str::to_string),
                        }))
                    }
                    Ok(jmap_client::event_source::PushNotification::CalendarAlert(_)) => None,
                    Err(error) => Some(Err(map_gateway_error(error))),
                }
            }
        }))))
    }
}

struct MailboxSync {
    mailboxes: Vec<MailboxRecord>,
    deleted_mailbox_ids: Vec<MailboxId>,
    replace_all_mailboxes: bool,
    cursor: SyncCursor,
}

struct MessageSync {
    messages: Vec<MessageRecord>,
    deleted_message_ids: Vec<MessageId>,
    cursor: SyncCursor,
}

async fn fetch_mailbox_sync(
    client: &Client,
    since_state: Option<&str>,
) -> Result<MailboxSync, GatewayError> {
    match since_state {
        Some(state) => match fetch_mailbox_delta(client, state).await {
            Ok(sync) => Ok(sync),
            Err(GatewayError::CannotCalculateChanges) => fetch_mailbox_full(client).await,
            Err(err) => Err(err),
        },
        None => fetch_mailbox_full(client).await,
    }
}

async fn fetch_email_sync(
    client: &Client,
    since_state: Option<&str>,
) -> Result<MessageSync, GatewayError> {
    match since_state {
        Some(state) => match fetch_email_delta(client, state).await {
            Ok(sync) => Ok(sync),
            Err(GatewayError::CannotCalculateChanges) => fetch_email_full(client).await,
            Err(err) => Err(err),
        },
        None => fetch_email_full(client).await,
    }
}

async fn fetch_mailbox_delta(
    client: &Client,
    since_state: &str,
) -> Result<MailboxSync, GatewayError> {
    let mut current_state = since_state.to_string();
    let mut upsert = Vec::new();
    let mut deleted = Vec::new();
    loop {
        let changes = client
            .mailbox_changes(&current_state, 500)
            .await
            .map_err(map_gateway_error)?;
        deleted.extend(changes.destroyed().iter().cloned().map(MailboxId));
        let fetch_ids: Vec<&str> = changes
            .created()
            .iter()
            .chain(changes.updated().iter())
            .map(String::as_str)
            .collect();
        if !fetch_ids.is_empty() {
            let mut request = client.build();
            request.get_mailbox().ids(fetch_ids).properties([
                mailbox::Property::Id,
                mailbox::Property::Name,
                mailbox::Property::Role,
                mailbox::Property::UnreadEmails,
                mailbox::Property::TotalEmails,
            ]);
            for mailbox in request
                .send_get_mailbox()
                .await
                .map_err(map_gateway_error)?
                .take_list()
            {
                upsert.push(to_mailbox_record(&mailbox));
            }
        }
        current_state = changes.new_state().to_string();
        if !changes.has_more_changes() {
            break;
        }
    }
    Ok(MailboxSync {
        mailboxes: upsert,
        deleted_mailbox_ids: deleted,
        replace_all_mailboxes: false,
        cursor: SyncCursor {
            object_type: SyncObject::Mailbox,
            state: current_state,
            updated_at: now_iso8601()?,
        },
    })
}

async fn fetch_email_delta(
    client: &Client,
    since_state: &str,
) -> Result<MessageSync, GatewayError> {
    let mut current_state = since_state.to_string();
    let mut upsert = Vec::new();
    let mut deleted = Vec::new();
    loop {
        let changes = client
            .email_changes(&current_state, Some(500))
            .await
            .map_err(map_gateway_error)?;
        deleted.extend(changes.destroyed().iter().cloned().map(MessageId));
        let fetch_ids: Vec<String> = changes
            .created()
            .iter()
            .chain(changes.updated().iter())
            .cloned()
            .collect();
        for chunk in fetch_ids.chunks(100) {
            let mut request = client.build();
            request
                .get_email()
                .ids(chunk.iter().map(String::as_str))
                .properties([
                    email::Property::Id,
                    email::Property::ThreadId,
                    email::Property::BlobId,
                    email::Property::MailboxIds,
                    email::Property::Keywords,
                    email::Property::Subject,
                    email::Property::From,
                    email::Property::Preview,
                    email::Property::ReceivedAt,
                    email::Property::HasAttachment,
                    email::Property::Size,
                ]);
            for email in request
                .send_get_email()
                .await
                .map_err(map_gateway_error)?
                .take_list()
            {
                upsert.push(to_message_record(&email));
            }
        }
        current_state = changes.new_state().to_string();
        if !changes.has_more_changes() {
            break;
        }
    }
    Ok(MessageSync {
        messages: upsert,
        deleted_message_ids: deleted,
        cursor: SyncCursor {
            object_type: SyncObject::Message,
            state: current_state,
            updated_at: now_iso8601()?,
        },
    })
}

async fn fetch_mailbox_full(client: &Client) -> Result<MailboxSync, GatewayError> {
    let mailbox_ids = client
        .mailbox_query(None::<mailbox::query::Filter>, None::<Vec<_>>)
        .await
        .map_err(map_gateway_error)?
        .take_ids();
    let mut request = client.build();
    request
        .get_mailbox()
        .ids(mailbox_ids.iter().map(String::as_str))
        .properties([
            mailbox::Property::Id,
            mailbox::Property::Name,
            mailbox::Property::Role,
            mailbox::Property::UnreadEmails,
            mailbox::Property::TotalEmails,
        ]);
    let mut response = request
        .send_get_mailbox()
        .await
        .map_err(map_gateway_error)?;
    let state = response.take_state();
    Ok(MailboxSync {
        mailboxes: response.take_list().iter().map(to_mailbox_record).collect(),
        deleted_mailbox_ids: Vec::new(),
        replace_all_mailboxes: true,
        cursor: SyncCursor {
            object_type: SyncObject::Mailbox,
            state,
            updated_at: now_iso8601()?,
        },
    })
}

async fn fetch_email_full(client: &Client) -> Result<MessageSync, GatewayError> {
    let email_ids = client
        .email_query(
            None::<email::query::Filter>,
            [email::query::Comparator::received_at().descending()].into(),
        )
        .await
        .map_err(map_gateway_error)?
        .take_ids();
    let mut messages = Vec::new();
    let mut state = None;
    for chunk in email_ids.chunks(100) {
        let mut request = client.build();
        request
            .get_email()
            .ids(chunk.iter().map(String::as_str))
            .properties([
                email::Property::Id,
                email::Property::ThreadId,
                email::Property::BlobId,
                email::Property::MailboxIds,
                email::Property::Keywords,
                email::Property::Subject,
                email::Property::From,
                email::Property::Preview,
                email::Property::ReceivedAt,
                email::Property::HasAttachment,
                email::Property::Size,
                email::Property::MessageId,
                email::Property::References,
                email::Property::InReplyTo,
            ]);
        let mut response = request.send_get_email().await.map_err(map_gateway_error)?;
        if state.is_none() {
            state = Some(response.take_state());
        }
        messages.extend(response.take_list().iter().map(to_message_record));
    }
    Ok(MessageSync {
        messages,
        deleted_message_ids: Vec::new(),
        cursor: SyncCursor {
            object_type: SyncObject::Message,
            state: state.unwrap_or_default(),
            updated_at: now_iso8601()?,
        },
    })
}

fn to_mailbox_record(mailbox: &jmap_client::mailbox::Mailbox) -> MailboxRecord {
    let role = match mailbox.role() {
        mailbox::Role::Inbox => Some("inbox".to_string()),
        mailbox::Role::Drafts => Some("drafts".to_string()),
        mailbox::Role::Sent => Some("sent".to_string()),
        mailbox::Role::Trash => Some("trash".to_string()),
        mailbox::Role::Junk => Some("junk".to_string()),
        mailbox::Role::Archive => Some("archive".to_string()),
        mailbox::Role::None => None,
        other => Some(format!("{other:?}").to_lowercase()),
    };
    MailboxRecord {
        id: MailboxId(mailbox.id().unwrap_or_default().to_string()),
        name: mailbox.name().unwrap_or("(unnamed)").to_string(),
        role,
        unread_emails: mailbox.unread_emails() as i64,
        total_emails: mailbox.total_emails() as i64,
    }
}

fn to_message_record(email: &jmap_client::email::Email) -> MessageRecord {
    let (from_name, from_email) = email
        .from()
        .and_then(|addresses| addresses.first())
        .map(|address| {
            (
                address.name().map(String::from),
                Some(address.email().to_string()),
            )
        })
        .unwrap_or((None, None));
    MessageRecord {
        id: MessageId(email.id().unwrap_or_default().to_string()),
        source_thread_id: mail_domain::ThreadId(email.thread_id().unwrap_or_default().to_string()),
        remote_blob_id: email.blob_id().map(|blob_id| BlobId(blob_id.to_string())),
        subject: email.subject().map(String::from),
        from_name,
        from_email,
        preview: email.preview().map(String::from),
        received_at: email
            .received_at()
            .and_then(timestamp_to_iso8601)
            .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string()),
        has_attachment: email.has_attachment(),
        size: email.size() as i64,
        mailbox_ids: email
            .mailbox_ids()
            .into_iter()
            .map(|id| MailboxId(id.to_string()))
            .collect(),
        keywords: email.keywords().into_iter().map(String::from).collect(),
        body_html: None,
        body_text: None,
        raw_mime: None,
        rfc_message_id: email.message_id().and_then(|ids| ids.first()).cloned(),
        in_reply_to: email.in_reply_to().and_then(|ids| ids.first()).cloned(),
        references: email
            .references()
            .map(|references| references.to_vec())
            .unwrap_or_default(),
    }
}

fn render_markdown(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(markdown, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body>{html_output}</body></html>"
    )
}

fn recipient_to_address(recipient: &Recipient) -> jmap_client::email::EmailAddress {
    match &recipient.name {
        Some(name) if !name.is_empty() => (name.as_str(), recipient.email.as_str()).into(),
        _ => recipient.email.as_str().into(),
    }
}

fn addresses_to_recipients(addresses: &[jmap_client::email::EmailAddress]) -> Vec<Recipient> {
    addresses
        .iter()
        .map(|address| Recipient {
            name: address.name().map(String::from),
            email: address.email().to_string(),
        })
        .collect()
}

fn prefix_subject(prefix: &str, subject: &str) -> String {
    if subject
        .to_ascii_lowercase()
        .starts_with(&prefix.to_ascii_lowercase())
    {
        subject.to_string()
    } else {
        format!("{prefix} {subject}")
    }
}

fn timestamp_to_iso8601(timestamp: i64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp(timestamp)
        .ok()
        .and_then(|value| value.format(&Rfc3339).ok())
}

fn now_iso8601() -> Result<String, GatewayError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|err| GatewayError::Rejected(err.to_string()))
}

fn synthesize_raw_mime(subject: &str, from_email: &str, body_text: Option<&str>) -> String {
    format!(
        "From: {from_email}\r\nSubject: {subject}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}\r\n",
        body_text.unwrap_or("")
    )
}

fn map_gateway_error(error: jmap_client::Error) -> GatewayError {
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
