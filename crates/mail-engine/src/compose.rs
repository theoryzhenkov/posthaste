use mail_domain::Recipient;
use pulldown_cmark::{html, Options, Parser};

/// Render a Markdown string to a self-contained HTML document for email.
///
/// Enables tables, strikethrough, and task lists from GFM.
/// The output wraps body content in a minimal HTML skeleton with UTF-8 charset.
///
/// @spec docs/L1-compose#supported-markdown-subset
/// @spec docs/L1-compose#html-output-rules
pub(crate) fn render_markdown(markdown: &str) -> String {
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

/// Convert a domain `Recipient` to a `jmap_client` email address for JMAP requests.
pub(crate) fn recipient_to_address(
    recipient: &Recipient,
) -> jmap_client::email::EmailAddress {
    match &recipient.name {
        Some(name) if !name.is_empty() => (name.as_str(), recipient.email.as_str()).into(),
        _ => recipient.email.as_str().into(),
    }
}

/// Convert JMAP email addresses to domain `Recipient` values.
///
/// @spec docs/L1-compose#reply-quoting
pub(crate) fn addresses_to_recipients(
    addresses: &[jmap_client::email::EmailAddress],
) -> Vec<Recipient> {
    addresses
        .iter()
        .map(|address| Recipient {
            name: address.name().map(String::from),
            email: address.email().to_string(),
        })
        .collect()
}

/// Prepend a subject prefix (`Re:` or `Fwd:`) if not already present.
///
/// @spec docs/L1-compose#reply-quoting
/// @spec docs/L1-compose#forward-quoting
pub(crate) fn prefix_subject(prefix: &str, subject: &str) -> String {
    if subject
        .to_ascii_lowercase()
        .starts_with(&prefix.to_ascii_lowercase())
    {
        subject.to_string()
    } else {
        format!("{prefix} {subject}")
    }
}
