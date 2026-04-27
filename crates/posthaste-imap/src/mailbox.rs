use imap_client::client::tokio::Client as ImapClient;
use imap_client::imap_types::command::CommandBody;
use imap_client::imap_types::mailbox::Mailbox;
use imap_client::imap_types::response::{Code, Data, StatusBody, StatusKind};
use imap_client::imap_types::status::{StatusDataItem, StatusDataItemName};
use imap_client::imap_types::IntoStatic;
use imap_client::tasks::tasks::select::SelectDataUnvalidated;
use imap_client::tasks::tasks::TaskError;
use imap_client::tasks::Task;
use posthaste_domain::{ImapModSeq, ImapSelectedMailbox, ImapUid, ImapUidValidity};

use crate::discovery::connect_authenticated_client;
use crate::{imap_mailbox_id, ImapAdapterError, ImapConnectionConfig};

/// EXAMINE one IMAP mailbox and return the server state needed by the sync planner.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
pub async fn examine_imap_mailbox(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
) -> Result<ImapSelectedMailbox, ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    client.refresh_capabilities().await?;
    examine_selected_mailbox(&mut client, mailbox_name).await
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ImapMailboxStatus {
    pub messages: Option<u32>,
    pub uid_next: Option<ImapUid>,
    pub uid_validity: Option<ImapUidValidity>,
}

/// Fetch cheap mailbox status without selecting the mailbox.
///
/// RFC 9051 STATUS is useful as a preflight, but the RFC also warns clients not
/// to expect many consecutive STATUS commands to be fast. Posthaste only uses
/// this to skip heavier reconciliation when the returned state proves the
/// mailbox cannot have changed under the current UIDVALIDITY epoch.
///
/// @spec docs/L0-providers#imap-delta-fallback
pub(crate) async fn status_imap_mailbox(
    client: &mut ImapClient,
    mailbox_name: &str,
) -> Result<ImapMailboxStatus, ImapAdapterError> {
    let mailbox = Mailbox::try_from(mailbox_name)
        .map_err(|_| ImapAdapterError::InvalidMailboxName(mailbox_name.to_string()))?
        .into_static();
    client
        .resolve(StatusTask::new(mailbox))
        .await
        .map_err(ImapAdapterError::from)?
        .map_err(|error| ImapAdapterError::Client(error.to_string()))
}

pub(crate) async fn examine_selected_mailbox(
    client: &mut ImapClient,
    mailbox_name: &str,
) -> Result<ImapSelectedMailbox, ImapAdapterError> {
    let mailbox = Mailbox::try_from(mailbox_name)
        .map_err(|_| ImapAdapterError::InvalidMailboxName(mailbox_name.to_string()))?
        .into_static();
    let data = client
        .resolve(ExamineStateTask::new(mailbox))
        .await
        .map_err(ImapAdapterError::from)?
        .map_err(|error| ImapAdapterError::Client(error.to_string()))?;
    selected_mailbox_from_examine_state(mailbox_name, data)
}

#[derive(Clone, Debug)]
struct StatusTask {
    mailbox: Mailbox<'static>,
    output: ImapMailboxStatus,
}

impl StatusTask {
    fn new(mailbox: Mailbox<'static>) -> Self {
        Self {
            mailbox,
            output: ImapMailboxStatus::default(),
        }
    }
}

impl Task for StatusTask {
    type Output = Result<ImapMailboxStatus, TaskError>;

    fn command_body(&self) -> CommandBody<'static> {
        CommandBody::Status {
            mailbox: self.mailbox.clone(),
            item_names: vec![
                StatusDataItemName::Messages,
                StatusDataItemName::UidNext,
                StatusDataItemName::UidValidity,
            ]
            .into(),
        }
    }

    fn process_data(&mut self, data: Data<'static>) -> Option<Data<'static>> {
        match data {
            Data::Status { items, .. } => {
                for item in items.iter() {
                    match item {
                        StatusDataItem::Messages(messages) => {
                            self.output.messages = Some(*messages);
                        }
                        StatusDataItem::UidNext(uid_next) => {
                            self.output.uid_next = Some(ImapUid(uid_next.get()));
                        }
                        StatusDataItem::UidValidity(uid_validity) => {
                            self.output.uid_validity = Some(ImapUidValidity(uid_validity.get()));
                        }
                        _ => {}
                    }
                }
                None
            }
            data => Some(data),
        }
    }

    fn process_tagged(self, status_body: StatusBody<'static>) -> Self::Output {
        match status_body.kind {
            StatusKind::Ok => Ok(self.output),
            StatusKind::No => Err(TaskError::UnexpectedNoResponse(status_body)),
            StatusKind::Bad => Err(TaskError::UnexpectedBadResponse(status_body)),
        }
    }
}

/// Convert an IMAP EXAMINE/SELECT response into Posthaste's selected-mailbox state.
pub fn selected_mailbox_from_examine(
    mailbox_name: &str,
    data: SelectDataUnvalidated,
) -> Result<ImapSelectedMailbox, ImapAdapterError> {
    selected_mailbox_from_examine_state(
        mailbox_name,
        ExamineState {
            select: data,
            highest_modseq: None,
        },
    )
}

fn selected_mailbox_from_examine_state(
    mailbox_name: &str,
    data: ExamineState,
) -> Result<ImapSelectedMailbox, ImapAdapterError> {
    let uid_validity = data
        .select
        .uid_validity
        .ok_or(ImapAdapterError::MissingSelectData("UIDVALIDITY"))?;
    Ok(ImapSelectedMailbox {
        mailbox_id: imap_mailbox_id(mailbox_name),
        mailbox_name: mailbox_name.to_string(),
        uid_validity: ImapUidValidity(uid_validity.get()),
        uid_next: data.select.uid_next.map(|uid| ImapUid(uid.get())),
        highest_modseq: data.highest_modseq,
    })
}

#[derive(Clone, Debug, Default)]
struct ExamineState {
    select: SelectDataUnvalidated,
    highest_modseq: Option<ImapModSeq>,
}

#[derive(Clone, Debug)]
struct ExamineStateTask {
    mailbox: Mailbox<'static>,
    output: ExamineState,
}

impl ExamineStateTask {
    fn new(mailbox: Mailbox<'static>) -> Self {
        Self {
            mailbox,
            output: ExamineState::default(),
        }
    }
}

impl Task for ExamineStateTask {
    type Output = Result<ExamineState, TaskError>;

    fn command_body(&self) -> CommandBody<'static> {
        CommandBody::Examine {
            mailbox: self.mailbox.clone(),
            parameters: Default::default(),
        }
    }

    fn process_data(&mut self, data: Data<'static>) -> Option<Data<'static>> {
        match data {
            Data::Flags(flags) => {
                self.output.select.flags = Some(flags);
                None
            }
            Data::Exists(count) => {
                self.output.select.exists = Some(count);
                None
            }
            Data::Recent(count) => {
                self.output.select.recent = Some(count);
                None
            }
            data => Some(data),
        }
    }

    fn process_untagged(
        &mut self,
        status_body: StatusBody<'static>,
    ) -> Option<StatusBody<'static>> {
        if let StatusKind::Ok = status_body.kind {
            match status_body.code {
                Some(Code::Unseen(seq)) => {
                    self.output.select.unseen = Some(seq);
                    None
                }
                Some(Code::PermanentFlags(flags)) => {
                    self.output.select.permanent_flags = Some(flags);
                    None
                }
                Some(Code::UidNext(uid)) => {
                    self.output.select.uid_next = Some(uid);
                    None
                }
                Some(Code::UidValidity(uid)) => {
                    self.output.select.uid_validity = Some(uid);
                    None
                }
                Some(Code::HighestModSeq(modseq)) => {
                    self.output.highest_modseq = Some(ImapModSeq(modseq.get()));
                    None
                }
                _ => Some(status_body),
            }
        } else {
            Some(status_body)
        }
    }

    fn process_tagged(self, status_body: StatusBody<'static>) -> Self::Output {
        match status_body.kind {
            StatusKind::Ok => {
                self.output.select.clone().validate()?;
                Ok(self.output)
            }
            StatusKind::No => Err(TaskError::UnexpectedNoResponse(status_body)),
            StatusKind::Bad => Err(TaskError::UnexpectedBadResponse(status_body)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroU64};

    use imap_client::imap_types::core::Text;
    use imap_client::imap_types::flag::{Flag, FlagPerm};
    use posthaste_domain::MailboxId;

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
}
