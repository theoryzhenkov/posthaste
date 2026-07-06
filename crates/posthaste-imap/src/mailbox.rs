use imap_client::client::tokio::Client as ImapClient;
use imap_client::imap_types::command::CommandBody;
use imap_client::imap_types::mailbox::Mailbox;
use imap_client::imap_types::response::{Code, Data, StatusBody, StatusKind};
use imap_client::imap_types::status::{StatusDataItem, StatusDataItemName};
use imap_client::imap_types::IntoStatic;
use imap_client::tasks::tasks::select::SelectDataUnvalidated;
use imap_client::tasks::tasks::TaskError;
use imap_client::tasks::Task;
use posthaste_domain_model::{ImapModSeq, ImapSelectedMailbox, ImapUid, ImapUidValidity};

use crate::{imap_mailbox_id, ImapAdapterError};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ImapMailboxStatus {
    pub messages: Option<u32>,
    pub uid_next: Option<ImapUid>,
    pub uid_validity: Option<ImapUidValidity>,
    pub highest_modseq: Option<ImapModSeq>,
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
    include_highest_modseq: bool,
) -> Result<ImapMailboxStatus, ImapAdapterError> {
    let mailbox = Mailbox::try_from(mailbox_name)
        .map_err(|_| ImapAdapterError::InvalidMailboxName(mailbox_name.to_string()))?
        .into_static();
    crate::timeout::with_deadline_resolve(
        "status",
        client.resolve(StatusTask::new(mailbox, include_highest_modseq)),
    )
    .await
}

/// Create a new top-level mailbox with IMAP `CREATE <name>`.
///
/// Flat create — the name is passed through as-is; hierarchy nesting (a name
/// carrying the server's delimiter) is out of scope. The name is encoded to a
/// wire mailbox by `imap-types` the same way existing mailbox names are (STATUS/
/// EXAMINE go through `Mailbox::try_from`), so no separate UTF-7 step is needed.
///
/// @spec docs/eph/RFC-L2-mailbox-management
pub(crate) async fn create_imap_mailbox(
    client: &mut ImapClient,
    mailbox_name: &str,
) -> Result<(), ImapAdapterError> {
    let mailbox = Mailbox::try_from(mailbox_name)
        .map_err(|_| ImapAdapterError::InvalidMailboxName(mailbox_name.to_string()))?
        .into_static();
    crate::timeout::with_deadline_resolve("create", client.resolve(CreateTask::new(mailbox))).await
}

pub(crate) async fn examine_selected_mailbox(
    client: &mut ImapClient,
    mailbox_name: &str,
) -> Result<ImapSelectedMailbox, ImapAdapterError> {
    let mailbox = Mailbox::try_from(mailbox_name)
        .map_err(|_| ImapAdapterError::InvalidMailboxName(mailbox_name.to_string()))?
        .into_static();
    let data = crate::timeout::with_deadline_resolve(
        "examine",
        client.resolve(ExamineStateTask::new(mailbox)),
    )
    .await?;
    selected_mailbox_from_examine_state(mailbox_name, data)
}

#[derive(Clone, Debug)]
struct StatusTask {
    mailbox: Mailbox<'static>,
    item_names: Vec<StatusDataItemName>,
    output: ImapMailboxStatus,
}

impl StatusTask {
    fn new(mailbox: Mailbox<'static>, include_highest_modseq: bool) -> Self {
        let mut item_names = vec![
            StatusDataItemName::Messages,
            StatusDataItemName::UidNext,
            StatusDataItemName::UidValidity,
        ];
        if include_highest_modseq {
            item_names.push(StatusDataItemName::HighestModSeq);
        }
        Self {
            mailbox,
            item_names,
            output: ImapMailboxStatus::default(),
        }
    }
}

impl Task for StatusTask {
    type Output = Result<ImapMailboxStatus, TaskError>;

    fn command_body(&self) -> CommandBody<'static> {
        CommandBody::Status {
            mailbox: self.mailbox.clone(),
            item_names: self.item_names.clone().into(),
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
                        StatusDataItem::HighestModSeq(modseq) if *modseq > 0 => {
                            self.output.highest_modseq = Some(ImapModSeq(*modseq));
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

/// Issues IMAP `CREATE <mailbox>` and reports success/failure from the tagged
/// response. No untagged data is expected.
#[derive(Clone, Debug)]
struct CreateTask {
    mailbox: Mailbox<'static>,
}

impl CreateTask {
    fn new(mailbox: Mailbox<'static>) -> Self {
        Self { mailbox }
    }
}

impl Task for CreateTask {
    type Output = Result<(), TaskError>;

    fn command_body(&self) -> CommandBody<'static> {
        CommandBody::Create {
            mailbox: self.mailbox.clone(),
        }
    }

    fn process_tagged(self, status_body: StatusBody<'static>) -> Self::Output {
        match status_body.kind {
            StatusKind::Ok => Ok(()),
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
mod tests;
