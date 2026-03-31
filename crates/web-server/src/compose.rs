use jmap_client::client::Client;
use jmap_client::{email, identity};

use pulldown_cmark::{html, Options, Parser};

use crate::jmap::SyncError;

/// Render Markdown to email-safe HTML.
pub fn render_markdown(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    format!(
        r#"<!DOCTYPE html><html><head><meta charset="utf-8"><style>
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; font-size: 14px; line-height: 1.5; color: #333; }}
blockquote {{ border-left: 3px solid #ccc; margin: 8px 0; padding: 4px 12px; color: #666; }}
pre {{ background: #f5f5f5; padding: 8px; border-radius: 4px; overflow-x: auto; }}
code {{ background: #f5f5f5; padding: 1px 4px; border-radius: 3px; }}
table {{ border-collapse: collapse; }} td, th {{ border: 1px solid #ddd; padding: 6px 12px; }}
</style></head><body>{}</body></html>"#,
        html_output
    )
}

/// Fetch the user's primary JMAP identity (first in the list).
pub async fn get_identity(client: &Client) -> Result<IdentityData, SyncError> {
    let mut request = client.build();
    request.get_identity().properties([
        identity::Property::Id,
        identity::Property::Name,
        identity::Property::Email,
    ]);
    let mut identities = request.send_get_identity().await?.take_list();

    let id = identities.pop().ok_or_else(|| {
        SyncError::Jmap(jmap_client::Error::Internal("No identity found".into()))
    })?;

    Ok(IdentityData {
        id: id.id().unwrap_or_default().to_string(),
        name: id.name().unwrap_or("").to_string(),
        email: id.email().unwrap_or("").to_string(),
    })
}

pub struct IdentityData {
    pub id: String,
    pub name: String,
    pub email: String,
}

/// Send an email via JMAP (Email/set + EmailSubmission/set in one request).
pub async fn send_email(
    client: &Client,
    identity_id: &str,
    from_name: &str,
    from_email: &str,
    to: &[(Option<String>, String)],
    cc: &[(Option<String>, String)],
    bcc: &[(Option<String>, String)],
    subject: &str,
    markdown_body: &str,
    in_reply_to: Option<&str>,
    references: Option<&str>,
) -> Result<(), SyncError> {
    let html_body = render_markdown(markdown_body);

    let mut request = client.build();

    // 1. Create the email draft
    let email_obj = request.set_email().create();

    // From
    let from_addr: jmap_client::email::EmailAddress = if from_name.is_empty() {
        from_email.into()
    } else {
        (from_name, from_email).into()
    };
    email_obj.from([from_addr]);

    // To
    if !to.is_empty() {
        email_obj.to(to.iter().map(|(name, addr)| -> jmap_client::email::EmailAddress {
            match name {
                Some(n) => (n.as_str(), addr.as_str()).into(),
                None => addr.as_str().into(),
            }
        }));
    }

    // Cc
    if !cc.is_empty() {
        email_obj.cc(cc.iter().map(|(name, addr)| -> jmap_client::email::EmailAddress {
            match name {
                Some(n) => (n.as_str(), addr.as_str()).into(),
                None => addr.as_str().into(),
            }
        }));
    }

    // Bcc
    if !bcc.is_empty() {
        email_obj.bcc(bcc.iter().map(|(name, addr)| -> jmap_client::email::EmailAddress {
            match name {
                Some(n) => (n.as_str(), addr.as_str()).into(),
                None => addr.as_str().into(),
            }
        }));
    }

    email_obj.subject(subject);

    // Body: text/plain (Markdown source) + text/html (rendered)
    email_obj.text_body(
        jmap_client::email::EmailBodyPart::new()
            .content_type("text/plain")
            .part_id("text_part"),
    );
    email_obj.body_value("text_part".to_string(), markdown_body);

    email_obj.html_body(
        jmap_client::email::EmailBodyPart::new()
            .content_type("text/html")
            .part_id("html_part"),
    );
    email_obj.body_value("html_part".to_string(), html_body.as_str());

    // Threading headers
    if let Some(reply_to) = in_reply_to {
        email_obj.in_reply_to([reply_to]);
    }
    if let Some(refs) = references {
        email_obj.references(refs.split_whitespace().collect::<Vec<_>>());
    }

    // The create() call above returns create_id "c0" (first created object).
    // We need to get that ID for the submission back-reference.
    // SetObject::create_id() on the created Email<Set> returns Some("c0").

    // 2. Submit the email via EmailSubmission/set.
    //    Use the convenience helper: email_submission_create builds a single request,
    //    but we need both Email/set and EmailSubmission/set in the same request.
    //    The submission's email_id must reference the just-created email: "#c0".
    let submission = request.set_email_submission().create();
    submission.email_id("#c0");
    submission.identity_id(identity_id);

    // 3. On success, move the email to Sent (via onSuccessUpdateEmail).
    //    We can set the $seen keyword + mailbox via the SetArguments.
    //    For now, the server should handle Sent folder placement automatically
    //    per the JMAP spec when EmailSubmission succeeds.

    // Send the multi-method request
    let response = request.send().await?;

    // Check for errors by inspecting the method responses.
    // The response contains results for Email/set (index 0) and
    // EmailSubmission/set (index 1).
    let method_responses = response.unwrap_method_responses();

    // Email/set response
    if let Some(resp) = method_responses.first() {
        if resp.is_error() {
            let err_str = format!("Email/set failed: {:?}", resp);
            return Err(SyncError::Jmap(jmap_client::Error::Internal(err_str)));
        }
    }

    // EmailSubmission/set response
    if let Some(resp) = method_responses.get(1) {
        if resp.is_error() {
            let err_str = format!("EmailSubmission/set failed: {:?}", resp);
            return Err(SyncError::Jmap(jmap_client::Error::Internal(err_str)));
        }
    }

    Ok(())
}

/// Data needed to pre-populate a reply/forward compose form.
pub struct ReplyData {
    pub from: Option<(Option<String>, String)>,
    pub to_all: Vec<(Option<String>, String)>,
    pub cc_all: Vec<(Option<String>, String)>,
    pub reply_subject: String,
    pub forward_subject: String,
    pub quoted_body: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

/// Fetch reply metadata for a given email ID.
pub async fn get_reply_data(client: &Client, email_id: &str) -> Result<ReplyData, SyncError> {
    let mut request = client.build();
    let get_req = request.get_email().ids([email_id]).properties([
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
    get_req
        .arguments()
        .body_properties([email::BodyProperty::PartId, email::BodyProperty::Type])
        .fetch_all_body_values(true);

    let mut emails = request.send_get_email().await?.take_list();
    let email = emails.pop().ok_or_else(|| {
        SyncError::Jmap(jmap_client::Error::Internal("email not found".into()))
    })?;

    // Extract text body for quoting
    let text_body = email
        .text_body()
        .and_then(|parts| parts.first())
        .and_then(|part| part.part_id())
        .and_then(|part_id| email.body_value(part_id))
        .map(|v| v.value().to_string());

    let quoted = text_body.map(|body| {
        body.lines()
            .map(|line| format!("> {}", line))
            .collect::<Vec<_>>()
            .join("\n")
    });

    // From address -> reply To
    let from = email
        .from()
        .and_then(|addrs| addrs.first())
        .map(|a| (a.name().map(String::from), a.email().to_string()));

    // All To + Cc for reply-all
    let to_all: Vec<(Option<String>, String)> = email
        .to()
        .map(|addrs| {
            addrs
                .iter()
                .map(|a| (a.name().map(String::from), a.email().to_string()))
                .collect()
        })
        .unwrap_or_default();

    let cc_all: Vec<(Option<String>, String)> = email
        .cc()
        .map(|addrs| {
            addrs
                .iter()
                .map(|a| (a.name().map(String::from), a.email().to_string()))
                .collect()
        })
        .unwrap_or_default();

    // Subject with Re:/Fwd: prefix
    let subject = email.subject().unwrap_or("");
    let reply_subject = if subject.starts_with("Re: ") || subject.starts_with("RE: ") {
        subject.to_string()
    } else {
        format!("Re: {}", subject)
    };
    let forward_subject = if subject.starts_with("Fwd: ") || subject.starts_with("FWD: ") {
        subject.to_string()
    } else {
        format!("Fwd: {}", subject)
    };

    // Message-ID for In-Reply-To and References
    let message_id = email
        .message_id()
        .and_then(|ids| ids.first().cloned());
    let existing_refs = email
        .references()
        .map(|refs| refs.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" "));

    let new_references = match (&existing_refs, &message_id) {
        (Some(refs), Some(mid)) => Some(format!("{} {}", refs, mid)),
        (None, Some(mid)) => Some(mid.clone()),
        (Some(refs), None) => Some(refs.clone()),
        (None, None) => None,
    };

    Ok(ReplyData {
        from,
        to_all,
        cc_all,
        reply_subject,
        forward_subject,
        quoted_body: quoted,
        in_reply_to: message_id,
        references: new_references,
    })
}
