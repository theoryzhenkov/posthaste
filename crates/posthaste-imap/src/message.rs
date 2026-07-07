use imap_client::imap_types::flag::Flag;
use mail_parser::MessageParser;
use posthaste_domain_model::{
    GmailLabel, ImapGmailMetadata, ImapMessageLocation, ImapModSeq, ImapSelectedMailbox, ImapUid,
    MessageRecord, Recipient, SystemKeyword, RFC3339_EPOCH,
};
use posthaste_domain_model::{MailboxId, MessageId, ThreadId};
use posthaste_domain_service::{gmail_message_id, gmail_thread_id, imap_message_id};

use crate::ImapAdapterError;

const IMAP_FLAG_FORWARDED: &str = "\\Forwarded";

/// Header-level data fetched for one IMAP message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImapFetchedHeader {
    pub mailbox_id: MailboxId,
    pub uid: ImapUid,
    pub modseq: Option<ImapModSeq>,
    pub flags: Vec<String>,
    pub rfc822_size: i64,
    pub has_attachment: bool,
    pub headers: Vec<u8>,
    pub updated_at: String,
}

/// Domain records produced from one fetched IMAP message header.
#[derive(Clone, Debug)]
pub struct ImapMappedHeader {
    pub message: MessageRecord,
    pub location: ImapMessageLocation,
    pub gmail_labels: Option<Vec<GmailLabel>>,
    pub mailbox_membership_source: ImapMailboxMembershipSource,
    pub provider_absent_mailbox_ids: Vec<MailboxId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapMailboxMembershipSource {
    SelectedMailbox,
    ProviderLabels,
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
    imap_header_message_record_with_gmail_metadata(selected, fetched, ImapGmailMetadata::default())
}

/// Convert fetched IMAP headers plus typed Gmail metadata into a Posthaste
/// message projection.
///
/// Gmail's `X-GM-MSGID` and `X-GM-THRID` are canonical when present. RFC 5322
/// identity headers remain parsed and stored, but they are fallback identity
/// inputs only when typed Gmail IDs are absent.
///
/// @spec docs/L0-providers#identity-and-threading
/// @spec docs/L1-sync#body-lazy
pub fn imap_header_message_record_with_gmail_metadata(
    selected: &ImapSelectedMailbox,
    fetched: ImapFetchedHeader,
    gmail: ImapGmailMetadata,
) -> Result<ImapMappedHeader, ImapAdapterError> {
    let gmail_labels = gmail.labels_observed.then_some(gmail.labels);
    let parsed = MessageParser::default()
        .parse(&fetched.headers)
        .ok_or(ImapAdapterError::ParseMessageHeaders)?;
    let message_id = gmail.message_id.map(gmail_message_id).unwrap_or_else(|| {
        imap_message_id(&fetched.mailbox_id, selected.uid_validity, fetched.uid)
    });
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
    let draft_id = parsed
        .header_raw(posthaste_domain_model::DRAFT_ID_HEADER)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    let source_thread_id = gmail
        .thread_id
        .map(gmail_thread_id)
        .unwrap_or_else(|| imap_thread_id(&message_id, rfc_message_id.as_deref(), &references));
    let from = parsed.from().and_then(|address| address.first());
    let to = parsed
        .to()
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
        .unwrap_or_default();
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
        to,
        preview: None,
        received_at,
        has_attachment: fetched.has_attachment,
        size: fetched.rfc822_size,
        mailbox_ids: vec![fetched.mailbox_id.clone()],
        keywords: imap_flag_keywords(&fetched.flags),
        body_html: None,
        body_text: None,
        raw_mime: None,
        rfc_message_id,
        in_reply_to,
        references,
        draft_id,
        list_unsubscribe: list_unsubscribe_from_parsed(&parsed),
    };
    let location = ImapMessageLocation {
        message_id,
        mailbox_id: fetched.mailbox_id,
        uid_validity: selected.uid_validity,
        uid: fetched.uid,
        modseq: fetched.modseq,
        updated_at: fetched.updated_at,
    };

    Ok(ImapMappedHeader {
        message,
        location,
        gmail_labels,
        mailbox_membership_source: ImapMailboxMembershipSource::SelectedMailbox,
        provider_absent_mailbox_ids: Vec::new(),
    })
}

/// Extracts and parses the RFC 2369/8058 unsubscribe headers from a parsed
/// message's raw headers (`header_raw` keeps the value undecoded so encoded-
/// word handling can never mangle a URL; the shared parser unfolds).
pub(crate) fn list_unsubscribe_from_parsed(
    parsed: &mail_parser::Message<'_>,
) -> Option<posthaste_domain_model::ListUnsubscribe> {
    let header = parsed.header_raw("List-Unsubscribe")?;
    let post = parsed.header_raw("List-Unsubscribe-Post");
    posthaste_domain_model::parse_list_unsubscribe(header, post)
}

/// Map IMAP system flags into the JMAP keyword vocabulary used by Posthaste.
pub fn imap_flag_keywords(flags: &[String]) -> Vec<String> {
    let mut keywords = flags
        .iter()
        .filter_map(|flag| {
            if let Some(keyword) = imap_system_flag_keyword(flag) {
                Some(keyword.as_str().to_string())
            } else if !flag.starts_with('\\') {
                Some(flag.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    keywords.sort();
    keywords.dedup();
    keywords
}

fn imap_system_flag_keyword(flag: &str) -> Option<SystemKeyword> {
    if flag.eq_ignore_ascii_case(IMAP_FLAG_FORWARDED) {
        return Some(SystemKeyword::Forwarded);
    }

    match Flag::try_from(flag).ok()? {
        Flag::Seen => Some(SystemKeyword::Seen),
        Flag::Flagged => Some(SystemKeyword::Flagged),
        Flag::Answered => Some(SystemKeyword::Answered),
        Flag::Draft => Some(SystemKeyword::Draft),
        _ => None,
    }
}

pub(crate) fn imap_flags_include_deleted(flags: &[String]) -> bool {
    flags
        .iter()
        .any(|flag| flag.eq_ignore_ascii_case("\\Deleted"))
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
mod tests;
