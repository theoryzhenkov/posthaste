use super::*;

pub(crate) fn fetch_item_names(
    fetch_modseq: bool,
    fetch_gmail_metadata: bool,
) -> MacroOrMessageDataItemNames<'static> {
    let mut items = vec![
        MessageDataItemName::Flags,
        MessageDataItemName::BodyStructure,
        MessageDataItemName::Rfc822Header,
        MessageDataItemName::Rfc822Size,
        MessageDataItemName::Uid,
    ];
    if fetch_modseq {
        items.push(MessageDataItemName::ModSeq);
    }
    if fetch_gmail_metadata {
        items.push(MessageDataItemName::GmailMessageId);
        items.push(MessageDataItemName::GmailThreadId);
        items.push(MessageDataItemName::GmailLabels);
    }

    MacroOrMessageDataItemNames::MessageDataItemNames(items)
}

pub fn fetched_header_from_items(
    selected: &ImapSelectedMailbox,
    items: impl IntoIterator<Item = MessageDataItem<'static>>,
    updated_at: String,
) -> Result<ImapFetchedHeader, ImapAdapterError> {
    Ok(fetched_header_from_items_with_metadata(selected, items, updated_at)?.header)
}

/// Extract Posthaste header data plus typed provider metadata from one FETCH
/// response.
///
/// Gmail fields are optional because generic IMAP fetches do not request them
/// and because RFC identity remains the fallback when Gmail omits them.
pub fn fetched_header_from_items_with_metadata(
    selected: &ImapSelectedMailbox,
    items: impl IntoIterator<Item = MessageDataItem<'static>>,
    updated_at: String,
) -> Result<ImapFetchedHeaderWithMetadata, ImapAdapterError> {
    let mut uid = None;
    let mut modseq = None;
    let mut gmail = ImapGmailMetadata::default();
    let mut flags = Vec::new();
    let mut rfc822_size = None;
    let mut has_attachment = false;
    let mut headers = None;

    for item in items {
        match item {
            MessageDataItem::Flags(next_flags) => {
                flags = next_flags.into_iter().map(imap_flag_fetch_name).collect();
            }
            MessageDataItem::Rfc822Header(nstring) => {
                headers = Some(
                    nstring
                        .into_option()
                        .map(|header| header.into_owned())
                        .unwrap_or_default(),
                );
            }
            MessageDataItem::Rfc822Size(size) => {
                rfc822_size = Some(i64::from(size));
            }
            MessageDataItem::BodyStructure(body_structure) => {
                has_attachment = body_structure_has_attachment(&body_structure);
            }
            MessageDataItem::Uid(next_uid) => {
                uid = Some(ImapUid(next_uid.get()));
            }
            MessageDataItem::ModSeq(next_modseq) => {
                modseq = Some(ImapModSeq(next_modseq.get()));
            }
            MessageDataItem::GmailMessageId(gmail_message_id) => {
                gmail.message_id = Some(GmailMessageId(gmail_message_id));
            }
            MessageDataItem::GmailThreadId(gmail_thread_id) => {
                gmail.thread_id = Some(GmailThreadId(gmail_thread_id));
            }
            MessageDataItem::GmailLabels(labels) => {
                gmail.labels_observed = true;
                gmail.labels = labels
                    .into_iter()
                    .map(|label| GmailLabel::from(label.as_ref()))
                    .collect();
            }
            _ => {}
        }
    }

    Ok(ImapFetchedHeaderWithMetadata {
        header: ImapFetchedHeader {
            mailbox_id: selected.mailbox_id.clone(),
            uid: uid.ok_or(ImapAdapterError::MissingFetchData("UID"))?,
            modseq,
            flags,
            rfc822_size: rfc822_size.ok_or(ImapAdapterError::MissingFetchData("RFC822.SIZE"))?,
            has_attachment,
            headers: headers.ok_or(ImapAdapterError::MissingFetchData("RFC822.HEADER"))?,
            updated_at,
        },
        gmail,
    })
}

pub(crate) fn body_structure_has_attachment(body_structure: &BodyStructure<'_>) -> bool {
    match body_structure {
        BodyStructure::Single {
            body,
            extension_data,
        } => {
            let has_attachment_disposition = extension_data
                .as_ref()
                .and_then(|extension| extension.tail.as_ref())
                .and_then(|disposition| disposition.disposition.as_ref())
                .is_some_and(|(kind, _)| kind.as_ref().eq_ignore_ascii_case(b"attachment"));
            let has_name_parameter = body
                .basic
                .parameter_list
                .iter()
                .any(|(key, _)| key.as_ref().eq_ignore_ascii_case(b"name"));
            let is_basic_non_text = matches!(
                &body.specific,
                SpecificFields::Basic { r#type, .. }
                    if !r#type.as_ref().eq_ignore_ascii_case(b"text")
            );

            has_attachment_disposition || has_name_parameter || is_basic_non_text
        }
        BodyStructure::Multi { bodies, .. } => {
            bodies.as_ref().iter().any(body_structure_has_attachment)
        }
    }
}

pub(crate) fn imap_flag_fetch_name(flag: FlagFetch<'static>) -> String {
    match flag {
        FlagFetch::Flag(flag) => flag.to_string(),
        FlagFetch::Recent => "\\Recent".to_string(),
    }
}
