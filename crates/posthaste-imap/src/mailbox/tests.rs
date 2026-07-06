use std::num::{NonZeroU32, NonZeroU64};

use imap_client::imap_types::core::Text;
use imap_client::imap_types::flag::{Flag, FlagPerm};
use posthaste_domain_model::MailboxId;

use super::*;

#[test]
fn selected_mailbox_requires_uidvalidity() {
    let error = selected_mailbox_from_examine("INBOX", SelectDataUnvalidated::default())
        .expect_err("UIDVALIDITY is required");

    assert!(matches!(
        error,
        ImapAdapterError::MissingSelectData("UIDVALIDITY")
    ));
}

#[test]
fn selected_mailbox_maps_uidvalidity_and_uidnext() {
    let selected = selected_mailbox_from_examine(
        "INBOX",
        SelectDataUnvalidated {
            uid_validity: Some(NonZeroU32::new(42).expect("nonzero")),
            uid_next: Some(NonZeroU32::new(100).expect("nonzero")),
            ..Default::default()
        },
    )
    .expect("selected mailbox");

    assert_eq!(
        selected.mailbox_id,
        MailboxId::from("imap:mailbox:494e424f58")
    );
    assert_eq!(selected.uid_validity, ImapUidValidity(42));
    assert_eq!(selected.uid_next, Some(ImapUid(100)));
    assert_eq!(selected.highest_modseq, None);
}

#[test]
fn examine_state_task_captures_highest_modseq() {
    let mut task =
        ExamineStateTask::new(Mailbox::try_from("INBOX").expect("mailbox").into_static());

    assert!(task.process_data(Data::Flags(vec![Flag::Seen])).is_none());
    assert!(task.process_data(Data::Exists(1)).is_none());
    assert!(task.process_data(Data::Recent(0)).is_none());
    for code in [
        Code::PermanentFlags(vec![FlagPerm::Flag(Flag::Seen)]),
        Code::UidNext(NonZeroU32::new(100).expect("uidnext")),
        Code::UidValidity(NonZeroU32::new(42).expect("uidvalidity")),
        Code::HighestModSeq(NonZeroU64::new(777).expect("modseq")),
    ] {
        assert!(task
            .process_untagged(StatusBody {
                kind: StatusKind::Ok,
                code: Some(code),
                text: Text::unvalidated("ok"),
            })
            .is_none());
    }

    let state = task
        .process_tagged(StatusBody {
            kind: StatusKind::Ok,
            code: None,
            text: Text::unvalidated("EXAMINE completed"),
        })
        .expect("examine state");
    let selected = selected_mailbox_from_examine_state("INBOX", state).expect("selected");

    assert_eq!(selected.highest_modseq, Some(ImapModSeq(777)));
}

#[test]
fn delete_task_issues_delete_command_for_the_mailbox() {
    let task = DeleteTask::new(
        Mailbox::try_from("Receipts")
            .expect("mailbox")
            .into_static(),
    );
    match task.command_body() {
        CommandBody::Delete { mailbox } => {
            assert_eq!(mailbox, Mailbox::try_from("Receipts").expect("mailbox"));
        }
        other => panic!("DELETE task must issue a Delete command, got {other:?}"),
    }
}

#[test]
fn delete_task_reports_ok_and_no() {
    let ok = DeleteTask::new(
        Mailbox::try_from("Receipts")
            .expect("mailbox")
            .into_static(),
    )
    .process_tagged(StatusBody {
        kind: StatusKind::Ok,
        code: None,
        text: Text::unvalidated("DELETE completed"),
    });
    assert!(ok.is_ok(), "a tagged OK is success");

    let refused = DeleteTask::new(
        Mailbox::try_from("Receipts")
            .expect("mailbox")
            .into_static(),
    )
    .process_tagged(StatusBody {
        kind: StatusKind::No,
        code: None,
        text: Text::unvalidated("cannot delete"),
    });
    assert!(refused.is_err(), "a tagged NO surfaces as an error");
}
