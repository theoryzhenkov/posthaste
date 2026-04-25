use mail_parser::MessageParser;
use posthaste_domain::{
    imap_message_id, ImapMessageLocation, ImapModSeq, ImapSelectedMailbox, ImapUid, MailboxId,
    MessageId, MessageRecord, ThreadId, RFC3339_EPOCH,
};

use crate::ImapAdapterError;

/// Header-level data fetched for one IMAP message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImapFetchedHeader {
    pub mailbox_id: MailboxId,
    pub uid: ImapUid,
    pub modseq: Option<ImapModSeq>,
    pub flags: Vec<String>,
    pub rfc822_size: i64,
    pub headers: Vec<u8>,
    pub updated_at: String,
}

/// Domain records produced from one fetched IMAP message header.
#[derive(Clone, Debug)]
pub struct ImapMappedHeader {
    pub message: MessageRecord,
    pub location: ImapMessageLocation,
}

/// Convert fetched IMAP RFC 822 headers into Posthaste's message projection.
///
/// This intentionally consumes header-only data. Body and attachment metadata
/// remain lazy/future work so the metadata sync loop does not fetch full
/// message bodies.
///
/// @spec docs/L1-sync#body-lazy
pub fn imap_header_message_record(
    selected: &ImapSelectedMailbox,
    fetched: ImapFetchedHeader,
) -> Result<ImapMappedHeader, ImapAdapterError> {
    let parsed = MessageParser::default()
        .parse(&fetched.headers)
        .ok_or(ImapAdapterError::ParseMessageHeaders)?;
    let message_id = imap_message_id(&fetched.mailbox_id, selected.uid_validity, fetched.uid);
    let rfc_message_id = parsed.message_id().map(str::to_string);
    let in_reply_to = parsed.in_reply_to().as_text().map(str::to_string);
    let references = parsed
        .references()
        .as_text_list()
        .map(|items| {
            items
                .iter()
                .map(|item| item.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let source_thread_id = imap_thread_id(&message_id, rfc_message_id.as_deref(), &references);
    let from = parsed.from().and_then(|address| address.first());
    let received_at = parsed
        .date()
        .map(|date| date.to_rfc3339())
        .unwrap_or_else(|| RFC3339_EPOCH.to_string());

    let message = MessageRecord {
        id: message_id.clone(),
        source_thread_id,
        remote_blob_id: None,
        subject: parsed.subject().map(str::to_string),
        from_name: from.and_then(|addr| addr.name.as_ref().map(|name| name.to_string())),
        from_email: from.and_then(|addr| addr.address.as_ref().map(|email| email.to_string())),
        preview: None,
        received_at,
        has_attachment: false,
        size: fetched.rfc822_size,
        mailbox_ids: vec![fetched.mailbox_id.clone()],
        keywords: imap_flag_keywords(&fetched.flags),
        body_html: None,
        body_text: None,
        raw_mime: None,
        rfc_message_id,
        in_reply_to,
        references,
    };
    let location = ImapMessageLocation {
        message_id,
        mailbox_id: fetched.mailbox_id,
        uid_validity: selected.uid_validity,
        uid: fetched.uid,
        modseq: fetched.modseq,
        updated_at: fetched.updated_at,
    };

    Ok(ImapMappedHeader { message, location })
}

/// Map IMAP system flags into the JMAP keyword vocabulary used by Posthaste.
pub fn imap_flag_keywords(flags: &[String]) -> Vec<String> {
    let mut keywords = flags
        .iter()
        .filter_map(|flag| match flag.as_str().to_ascii_lowercase().as_str() {
            "\\seen" => Some("$seen".to_string()),
            "\\flagged" => Some("$flagged".to_string()),
            "\\answered" => Some("$answered".to_string()),
            "\\draft" => Some("$draft".to_string()),
            "\\forwarded" => Some("$forwarded".to_string()),
            _ if !flag.starts_with('\\') => Some(flag.to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    keywords.sort();
    keywords.dedup();
    keywords
}

fn imap_thread_id(
    message_id: &MessageId,
    rfc_message_id: Option<&str>,
    references: &[String],
) -> ThreadId {
    let root = references
        .first()
        .map(String::as_str)
        .or(rfc_message_id)
        .unwrap_or_else(|| message_id.as_str());
    ThreadId(format!("imap:thread:{}", hex::encode(root.as_bytes())))
}

#[cfg(test)]
mod tests {
    use posthaste_domain::{ImapUidValidity, MailboxId};

    use super::*;

    #[test]
    fn maps_header_metadata_without_fetching_body() {
        let selected = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
            mailbox_name: "INBOX".to_string(),
            uid_validity: ImapUidValidity(9),
            uid_next: None,
            highest_modseq: None,
        };
        let mapped = imap_header_message_record(
            &selected,
            ImapFetchedHeader {
                mailbox_id: selected.mailbox_id.clone(),
                uid: ImapUid(42),
                modseq: Some(ImapModSeq(777)),
                flags: vec!["\\Seen".to_string(), "\\Flagged".to_string()],
                rfc822_size: 512,
                headers: b"From: Alice <alice@example.test>\r\nDate: Sat, 20 Nov 2021 14:22:01 -0800\r\nSubject: Hello\r\nMessage-ID: <m1@example.test>\r\nReferences: <root@example.test> <parent@example.test>\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("mapped header");

        assert_eq!(
            mapped.message.id.as_str(),
            "imap:9:42:696d61703a6d61696c626f783a34393465343234663538"
        );
        assert_eq!(mapped.message.subject.as_deref(), Some("Hello"));
        assert_eq!(mapped.message.from_name.as_deref(), Some("Alice"));
        assert_eq!(
            mapped.message.from_email.as_deref(),
            Some("alice@example.test")
        );
        assert_eq!(
            mapped.message.received_at,
            "2021-11-20T14:22:01-08:00".to_string()
        );
        assert_eq!(mapped.message.keywords, vec!["$flagged", "$seen"]);
        assert_eq!(mapped.message.body_text, None);
        assert_eq!(mapped.message.raw_mime, None);
        assert_eq!(mapped.location.uid, ImapUid(42));
        assert_eq!(mapped.location.modseq, Some(ImapModSeq(777)));
    }

    #[test]
    fn malformed_headers_return_typed_error() {
        let selected = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
            mailbox_name: "INBOX".to_string(),
            uid_validity: ImapUidValidity(9),
            uid_next: None,
            highest_modseq: None,
        };

        let error = imap_header_message_record(
            &selected,
            ImapFetchedHeader {
                mailbox_id: selected.mailbox_id.clone(),
                uid: ImapUid(42),
                modseq: None,
                flags: Vec::new(),
                rfc822_size: 0,
                headers: Vec::new(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect_err("empty headers are invalid");

        assert!(matches!(error, ImapAdapterError::ParseMessageHeaders));
    }

    #[test]
    fn maps_custom_imap_keywords_to_jmap_keywords() {
        let keywords = imap_flag_keywords(&[
            "\\Seen".to_string(),
            "project-x".to_string(),
            "\\UnknownExtension".to_string(),
        ]);

        assert_eq!(keywords, vec!["$seen", "project-x"]);
    }
}
