use std::borrow::Cow;
use std::num::{NonZeroU32, NonZeroU64};

use imap_client::imap_types::body::{BasicFields, Body, BodyStructure, SpecificFields};
use imap_client::imap_types::core::IString;
use imap_client::imap_types::core::{NString, Text};
use imap_client::imap_types::flag::{Flag, FlagFetch};
use posthaste_domain_model::ImapUidValidity;
use posthaste_domain_model::MailboxId;

use super::*;

fn selected_mailbox() -> ImapSelectedMailbox {
    ImapSelectedMailbox {
        mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
        mailbox_name: "INBOX".to_string(),
        uid_validity: ImapUidValidity(9),
        uid_next: None,
        highest_modseq: None,
    }
}

#[test]
fn fetched_header_extracts_required_items_and_modseq() {
    let fetched = fetched_header_from_items(
        &selected_mailbox(),
        [
            MessageDataItem::Flags(vec![
                FlagFetch::Flag(Flag::Seen),
                FlagFetch::Flag(Flag::Flagged),
                FlagFetch::Recent,
            ]),
            MessageDataItem::Rfc822Header(
                NString::try_from(
                    b"From: Alice <alice@example.test>\r\nSubject: Hello\r\n\r\n".as_slice(),
                )
                .expect("header nstring"),
            ),
            MessageDataItem::Rfc822Size(512),
            MessageDataItem::Uid(NonZeroU32::new(42).expect("uid")),
            MessageDataItem::ModSeq(NonZeroU64::new(777).expect("modseq")),
        ],
        "2026-04-25T00:00:00Z".to_string(),
    )
    .expect("fetched header");

    assert_eq!(fetched.uid, ImapUid(42));
    assert_eq!(fetched.modseq, Some(ImapModSeq(777)));
    assert_eq!(
        fetched.flags,
        vec![
            "\\Seen".to_string(),
            "\\Flagged".to_string(),
            "\\Recent".to_string()
        ]
    );
    assert_eq!(fetched.rfc822_size, 512);
    assert!(!fetched.has_attachment);
    assert!(fetched.headers.starts_with(b"From: Alice"));
}

#[test]
fn fetched_header_extracts_typed_gmail_metadata() {
    let fetched = fetched_header_from_items_with_metadata(
        &selected_mailbox(),
        [
            MessageDataItem::Rfc822Header(
                NString::try_from(b"Subject: Gmail\r\n\r\n".as_slice()).expect("header nstring"),
            ),
            MessageDataItem::Rfc822Size(512),
            MessageDataItem::Uid(NonZeroU32::new(42).expect("uid")),
            MessageDataItem::GmailMessageId(1278455344230334865),
            MessageDataItem::GmailThreadId(1266894439832287888),
            MessageDataItem::GmailLabels(vec![
                Cow::from("INBOX"),
                Cow::from("\\Important"),
            ]),
        ],
        "2026-04-25T00:00:00Z".to_string(),
    )
    .expect("fetched header");

    assert_eq!(
        fetched.gmail.message_id,
        Some(GmailMessageId(1278455344230334865))
    );
    assert_eq!(
        fetched.gmail.thread_id,
        Some(GmailThreadId(1266894439832287888))
    );
    assert_eq!(
        fetched
            .gmail
            .labels
            .iter()
            .map(GmailLabel::as_str)
            .collect::<Vec<_>>(),
        vec!["INBOX", "\\Important"]
    );
    assert!(fetched.gmail.labels_observed);
    assert_eq!(fetched.header.uid, ImapUid(42));
}

#[test]
fn fetched_header_requires_uid() {
    let error = fetched_header_from_items(
        &selected_mailbox(),
        [
            MessageDataItem::Rfc822Header(
                NString::try_from(b"Subject: Hello\r\n\r\n".as_slice()).expect("header nstring"),
            ),
            MessageDataItem::Rfc822Size(512),
        ],
        "2026-04-25T00:00:00Z".to_string(),
    )
    .expect_err("UID is required");

    assert!(matches!(error, ImapAdapterError::MissingFetchData("UID")));
}

#[test]
fn fetch_items_only_include_modseq_when_condstore_is_available() {
    assert_eq!(
        fetch_item_names(false, false),
        MacroOrMessageDataItemNames::MessageDataItemNames(vec![
            MessageDataItemName::Flags,
            MessageDataItemName::BodyStructure,
            MessageDataItemName::Rfc822Header,
            MessageDataItemName::Rfc822Size,
            MessageDataItemName::Uid,
        ])
    );
    assert_eq!(
        fetch_item_names(true, false),
        MacroOrMessageDataItemNames::MessageDataItemNames(vec![
            MessageDataItemName::Flags,
            MessageDataItemName::BodyStructure,
            MessageDataItemName::Rfc822Header,
            MessageDataItemName::Rfc822Size,
            MessageDataItemName::Uid,
            MessageDataItemName::ModSeq,
        ])
    );
}

#[test]
fn fetch_items_include_typed_gmail_metadata_when_requested() {
    assert_eq!(
        fetch_item_names(false, true),
        MacroOrMessageDataItemNames::MessageDataItemNames(vec![
            MessageDataItemName::Flags,
            MessageDataItemName::BodyStructure,
            MessageDataItemName::Rfc822Header,
            MessageDataItemName::Rfc822Size,
            MessageDataItemName::Uid,
            MessageDataItemName::GmailMessageId,
            MessageDataItemName::GmailThreadId,
            MessageDataItemName::GmailLabels,
        ])
    );
}

#[test]
fn fetched_header_marks_bodystructure_attachment_metadata() {
    let fetched = fetched_header_from_items(
        &selected_mailbox(),
        [
            MessageDataItem::Rfc822Header(
                NString::try_from(b"Subject: With attachment\r\n\r\n".as_slice())
                    .expect("header nstring"),
            ),
            MessageDataItem::Rfc822Size(512),
            MessageDataItem::Uid(NonZeroU32::new(42).expect("uid")),
            MessageDataItem::BodyStructure(attachment_body_structure()),
        ],
        "2026-04-25T00:00:00Z".to_string(),
    )
    .expect("fetched header");

    assert!(fetched.has_attachment);
}

#[test]
fn changed_since_fetch_task_uses_uid_fetch_modifiers() {
    let task = ChangedSinceFetchTask::new(
        SequenceSet::try_from("1:*").expect("sequence set"),
        fetch_item_names(true, true),
        NonZeroU64::new(777).expect("modseq"),
        true,
    );

    let CommandBody::Fetch {
        uid,
        modifiers,
        macro_or_item_names,
        ..
    } = task.command_body()
    else {
        panic!("expected FETCH");
    };

    assert!(uid);
    assert_eq!(
        modifiers,
        vec![
            FetchModifier::ChangedSince(NonZeroU64::new(777).expect("modseq")),
            FetchModifier::Vanished,
        ]
    );
    assert_eq!(macro_or_item_names, fetch_item_names(true, true));
}

#[test]
fn changed_since_fetch_task_collects_fetch_rows_and_vanished_uids() {
    let mut task = ChangedSinceFetchTask::new(
        SequenceSet::try_from("1:*").expect("sequence set"),
        fetch_item_names(true, false),
        NonZeroU64::new(777).expect("modseq"),
        true,
    );

    assert!(task
        .process_data(Data::Fetch {
            seq: NonZeroU32::new(2).expect("seq"),
            items: vec![MessageDataItem::Uid(NonZeroU32::new(42).expect("uid"))]
                .try_into()
                .expect("fetch items"),
        })
        .is_none());
    assert!(task
        .process_data(Data::Vanished {
            earlier: true,
            known_uids: SequenceSet::try_from("7:8,10").expect("vanished uids"),
        })
        .is_none());

    let snapshot = task
        .process_tagged(StatusBody {
            kind: StatusKind::Ok,
            code: None,
            text: Text::unvalidated("FETCH completed"),
        })
        .expect("fetch snapshot");

    assert_eq!(snapshot.headers.len(), 1);
    assert_eq!(
        snapshot.vanished_uids,
        vec![ImapUid(7), ImapUid(8), ImapUid(10)]
    );
}

fn attachment_body_structure() -> BodyStructure<'static> {
    BodyStructure::Single {
        body: Body {
            basic: BasicFields {
                parameter_list: vec![(
                    IString::try_from("name").expect("name key"),
                    IString::try_from("notes.txt").expect("name value"),
                )],
                id: NString::NIL,
                description: NString::NIL,
                content_transfer_encoding: IString::try_from("base64").expect("encoding"),
                size: 12,
            },
            specific: SpecificFields::Basic {
                r#type: IString::try_from("application").expect("type"),
                subtype: IString::try_from("pdf").expect("subtype"),
            },
        },
        extension_data: None,
    }
}

/// Real Gmail (per RFC 7162) sends the `VANISHED (EARLIER)` response with NO
/// leading sequence number: `* VANISHED (EARLIER) <known-uids>`. The QRESYNC
/// delta sync depends on the forked `imap-codec` decoding this so deletions are
/// observed; if it can't, the delta path breaks and sync degrades to repeated
/// full re-fetches. The mock Gmail fixture sidesteps this by emitting a
/// non-standard `* 1 VANISHED ...` (a fabricated leading number), so this is
/// the faithful-real-format decode test the mock can't provide.
#[test]
fn real_gmail_vanished_earlier_decodes_without_a_leading_sequence_number() {
    use imap_client::imap_types::response::{Data, Response};
    use imap_codec::decode::Decoder;
    use imap_codec::ResponseCodec;

    let bytes = b"* VANISHED (EARLIER) 300:310,405,411\r\n";
    let (_remaining, response) = ResponseCodec::default()
        .decode(bytes)
        .expect("real Gmail `* VANISHED (EARLIER) ...` must decode");
    match response {
        Response::Data(Data::Vanished { earlier, .. }) => {
            assert!(earlier, "the EARLIER flag should be set");
        }
        other => panic!("expected Data::Vanished, got {other:?}"),
    }
}
