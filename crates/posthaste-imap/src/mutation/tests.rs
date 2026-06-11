use imap_client::imap_types::flag::Flag;
use posthaste_domain::{ImapUid, ImapUidValidity, MailboxId, MessageId};

use super::*;

#[test]
fn maps_jmap_keywords_to_imap_system_flags() {
    let flags = imap_flags_for_keywords(&[
        SystemKeyword::Seen.as_str().to_string(),
        SystemKeyword::Flagged.as_str().to_string(),
        SystemKeyword::Answered.as_str().to_string(),
        SystemKeyword::Draft.as_str().to_string(),
        SystemKeyword::Forwarded.as_str().to_string(),
    ])
    .expect("flags");

    assert_eq!(
        flags,
        vec![
            Flag::Seen,
            Flag::Flagged,
            Flag::Answered,
            Flag::Draft,
            Flag::try_from(IMAP_FLAG_FORWARDED).expect("forwarded flag"),
        ]
    );
}

#[test]
fn preserves_custom_keywords_as_imap_keywords() {
    let flags = imap_flags_for_keywords(&["project-x".to_string()]).expect("custom keyword flag");

    assert_eq!(
        flags,
        vec![Flag::try_from("project-x").expect("custom keyword")]
    );
}

#[test]
fn rejects_keywords_that_are_not_valid_imap_atoms() {
    let error = imap_flags_for_keywords(&["bad keyword".to_string()])
        .expect_err("spaces are not valid atom characters");

    assert!(matches!(
        error,
        ImapAdapterError::InvalidKeywordFlag {
            keyword,
            ..
        } if keyword == "bad keyword"
    ));
}

#[test]
fn computes_mailbox_replacement_delta() {
    let delta = imap_mailbox_replacement_delta(
        &[
            MailboxId::from("imap:mailbox:inbox"),
            MailboxId::from("imap:mailbox:archive"),
        ],
        &[
            MailboxId::from("imap:mailbox:archive"),
            MailboxId::from("imap:mailbox:trash"),
        ],
    );

    assert_eq!(
        delta,
        ImapMailboxReplacementDelta {
            add: vec![MailboxId::from("imap:mailbox:trash")],
            remove: vec![MailboxId::from("imap:mailbox:inbox")],
        }
    );
}

#[test]
fn rejects_uid_fetch_response_without_matching_uid() {
    let error = verify_message_data_contains_uid(
        &location(),
        [MessageDataItem::Uid(NonZeroU32::new(99).expect("uid"))],
        "matching UID FETCH response",
    )
    .expect_err("matching UID is required");

    assert!(matches!(
        error,
        ImapAdapterError::MissingFetchData("matching UID FETCH response")
    ));
}

#[test]
fn uid_expunge_task_uses_uid_expunge_command_body() {
    let task = UidExpungeTask::new(uid_sequence_set(&location()).expect("uid set"));

    let CommandBody::ExpungeUid { .. } = task.command_body() else {
        panic!("UID EXPUNGE command body is required");
    };
}

fn location() -> ImapMessageLocation {
    ImapMessageLocation {
        message_id: MessageId::from("message-1"),
        mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
        uid_validity: ImapUidValidity(9),
        uid: ImapUid(42),
        modseq: None,
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    }
}
