//! The one derivation of message metadata from raw MIME.
//!
//! Every field here is a header the store keeps a column for but that a
//! *metadata* sync may never have parsed — either because the provider path
//! that ingested the row predates the column, or because the header block was
//! only ever fetched in full at body time. That makes the derivation
//! re-runnable: the raw MIME the body cache already retains is the same source
//! the at-open fill reads, so a row missing one of these fields can be
//! repaired offline, with no provider round trip.
//!
//! It lives in the domain core rather than in either adapter because both
//! consume it: `posthaste-imap` derives at body-fetch time (the at-open fill)
//! and `posthaste-store` re-derives from its own cached `.eml` files. Adapters
//! never depend on each other, and two copies of this parse would be two
//! chances to disagree about what a message's Cc is.
//!
//! ADDING A FIELD: add it to [`DerivedMessageMetadata`] and to
//! [`derive_message_metadata_from_parsed`]; the store maps fields to columns in
//! its own one-line table and bumps its derivation revision so already-cached
//! mail is re-derived once.

use mail_parser::MessageParser;
use posthaste_domain_model::{FetchedBody, ListUnsubscribe, Recipient};

/// Message metadata recoverable from a message's raw MIME alone.
///
/// Every field is *absence-tolerant*: an empty vector / `None` means "this
/// message carries no such header", never "we could not read it". Consumers
/// must therefore never write an empty value over a stored one — these headers
/// are immutable per message, so a stored value is always at least as good as
/// a freshly derived empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DerivedMessageMetadata {
    /// Carbon-copy recipients.
    pub cc: Vec<Recipient>,
    /// Blind-carbon-copy recipients — present only on mail the user sent (a
    /// delivering MTA strips the header) or on an unsent draft.
    pub bcc: Vec<Recipient>,
    /// `Reply-To` addresses, when the sender nominated somewhere other than
    /// `From`.
    pub reply_to: Vec<Recipient>,
    /// Parsed RFC 2369/8058 unsubscribe targets.
    pub list_unsubscribe: Option<ListUnsubscribe>,
}

impl DerivedMessageMetadata {
    /// Whether the derivation found nothing at all — the ordinary shape for a
    /// plain one-to-one message, and the signal to a repair pass that this row
    /// had nothing to gain.
    pub fn is_empty(&self) -> bool {
        self.cc.is_empty()
            && self.bcc.is_empty()
            && self.reply_to.is_empty()
            && self.list_unsubscribe.is_none()
    }
}

impl From<&FetchedBody> for DerivedMessageMetadata {
    /// The same fields as a provider already carried them on a fetched body.
    ///
    /// This is what lets the at-open fill and the store's offline re-derive
    /// share one write path: a gateway that parsed the raw MIME itself hands
    /// over its result in the shape the repair produces from disk, and the
    /// store cannot tell (or need to tell) which one it is writing.
    fn from(body: &FetchedBody) -> Self {
        Self {
            cc: body.cc.clone(),
            bcc: body.bcc.clone(),
            reply_to: body.reply_to.clone(),
            list_unsubscribe: body.list_unsubscribe.clone(),
        }
    }
}

/// Parses raw RFC822 bytes and derives [`DerivedMessageMetadata`].
///
/// `None` only when the bytes are not a parseable message at all; a message
/// with none of these headers parses fine and derives empty.
pub fn derive_message_metadata(raw_mime: &[u8]) -> Option<DerivedMessageMetadata> {
    let parsed = MessageParser::default().parse(raw_mime)?;
    Some(derive_message_metadata_from_parsed(&parsed))
}

/// [`derive_message_metadata`] against an already-parsed message, so a caller
/// that parses the raw bytes anyway (the body-fetch path, which also needs the
/// HTML/text parts and attachments) pays for one parse rather than two.
pub fn derive_message_metadata_from_parsed(
    parsed: &mail_parser::Message<'_>,
) -> DerivedMessageMetadata {
    DerivedMessageMetadata {
        cc: recipients_from(parsed.cc()),
        bcc: recipients_from(parsed.bcc()),
        reply_to: recipients_from(parsed.reply_to()),
        list_unsubscribe: list_unsubscribe_from_parsed(parsed),
    }
}

/// Projects one of `mail_parser`'s address headers into the domain recipient
/// shape. An address with no `addr-spec` is dropped rather than stored under an
/// empty email — a display-name-only group construct is not a recipient
/// anything can act on.
pub fn recipients_from(addresses: Option<&mail_parser::Address<'_>>) -> Vec<Recipient> {
    addresses
        .map(|addresses| {
            addresses
                .iter()
                .filter_map(|address| {
                    Some(Recipient {
                        name: address.name.as_ref().map(|name| name.to_string()),
                        email: address.address.as_ref()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Extracts and parses the RFC 2369/8058 unsubscribe headers (`header_raw`
/// keeps the value undecoded so encoded-word handling can never mangle a URL;
/// the shared parser unfolds).
pub fn list_unsubscribe_from_parsed(parsed: &mail_parser::Message<'_>) -> Option<ListUnsubscribe> {
    let header = parsed.header_raw("List-Unsubscribe")?;
    let post = parsed.header_raw("List-Unsubscribe-Post");
    posthaste_domain_model::parse_list_unsubscribe(header, post)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITH_HEADERS: &str = concat!(
        "From: Alice <alice@example.test>\r\n",
        "To: Bob <bob@example.test>\r\n",
        "Cc: Carol <carol@example.test>, dave@example.test\r\n",
        "Bcc: eve@example.test\r\n",
        "Reply-To: replies@example.test\r\n",
        "List-Unsubscribe: <https://lists.example.test/u/1>\r\n",
        "Subject: Hello\r\n",
        "\r\n",
        "Body.\r\n",
    );

    #[test]
    fn derives_every_field_from_raw_mime() {
        let derived = derive_message_metadata(WITH_HEADERS.as_bytes()).expect("parses");
        assert_eq!(
            derived
                .cc
                .iter()
                .map(|recipient| recipient.email.as_str())
                .collect::<Vec<_>>(),
            vec!["carol@example.test", "dave@example.test"]
        );
        assert_eq!(derived.bcc.len(), 1);
        assert_eq!(derived.reply_to[0].email, "replies@example.test");
        assert!(derived.list_unsubscribe.is_some());
        assert!(!derived.is_empty());
    }

    #[test]
    fn a_message_without_the_headers_derives_empty_rather_than_failing() {
        let raw = "From: a@example.test\r\nSubject: Plain\r\n\r\nHi.\r\n";
        let derived = derive_message_metadata(raw.as_bytes()).expect("parses");
        assert!(derived.is_empty());
    }

    #[test]
    fn a_parse_of_the_same_bytes_twice_derives_the_same_values() {
        // The property the store's repair pass rests on: the derivation is a
        // pure function of the cached bytes, so re-running it is a no-op.
        let first = derive_message_metadata(WITH_HEADERS.as_bytes()).expect("parses");
        let second = derive_message_metadata(WITH_HEADERS.as_bytes()).expect("parses");
        assert_eq!(first, second);
    }
}
