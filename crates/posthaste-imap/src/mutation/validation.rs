use std::num::NonZeroU32;

use imap_client::client::tokio::Client as ImapClient;
use imap_client::imap_types::{
    command::CommandBody,
    fetch::MessageDataItem,
    fetch::{MacroOrMessageDataItemNames, MessageDataItemName},
    response::{Data, StatusBody, StatusKind},
    sequence::{SeqOrUid, SequenceSet},
};
use imap_client::tasks::tasks::TaskError;
use imap_client::tasks::Task;
use posthaste_domain_service::ImapMessageLocation;

use crate::{selected_mailbox_from_examine, ImapAdapterError};

pub(crate) fn uid_sequence_set(
    location: &ImapMessageLocation,
) -> Result<SequenceSet, ImapAdapterError> {
    let uid = NonZeroU32::new(location.uid.0)
        .ok_or_else(|| ImapAdapterError::InvalidUidSequence("UID 0".to_string()))?;
    Ok(SequenceSet::from(SeqOrUid::from(uid)))
}

pub(crate) async fn select_validated_mailbox(
    client: &mut ImapClient,
    mailbox_name: &str,
    location: &ImapMessageLocation,
) -> Result<(), ImapAdapterError> {
    let selected = selected_mailbox_from_examine(mailbox_name, client.select(mailbox_name).await?)?;
    if selected.uid_validity != location.uid_validity {
        return Err(ImapAdapterError::UidValidityMismatch {
            mailbox_name: mailbox_name.to_string(),
            expected: location.uid_validity.0,
            actual: selected.uid_validity.0,
        });
    }
    Ok(())
}

pub(crate) async fn verify_uid_fetch_response(
    client: &mut ImapClient,
    location: &ImapMessageLocation,
) -> Result<(), ImapAdapterError> {
    let items = client
        .uid_fetch_first(uid(location)?, uid_fetch_item_names())
        .await
        .map_err(ImapAdapterError::from)?;
    verify_message_data_contains_uid(location, items, "matching UID FETCH response")
}

fn uid(location: &ImapMessageLocation) -> Result<NonZeroU32, ImapAdapterError> {
    NonZeroU32::new(location.uid.0)
        .ok_or_else(|| ImapAdapterError::InvalidUidSequence("UID 0".to_string()))
}

fn uid_fetch_item_names() -> MacroOrMessageDataItemNames<'static> {
    MacroOrMessageDataItemNames::MessageDataItemNames(vec![MessageDataItemName::Uid])
}

pub(crate) fn verify_message_data_contains_uid(
    location: &ImapMessageLocation,
    items: impl IntoIterator<Item = MessageDataItem<'static>>,
    missing_label: &'static str,
) -> Result<(), ImapAdapterError> {
    let found_matching_uid = items.into_iter().any(|item| match item {
        MessageDataItem::Uid(uid) => uid.get() == location.uid.0,
        _ => false,
    });
    if found_matching_uid {
        Ok(())
    } else {
        Err(ImapAdapterError::MissingFetchData(missing_label))
    }
}

pub(crate) async fn uid_expunge(
    client: &mut ImapClient,
    location: &ImapMessageLocation,
) -> Result<Vec<NonZeroU32>, ImapAdapterError> {
    client
        .resolve(UidExpungeTask::new(uid_sequence_set(location)?))
        .await
        .map_err(ImapAdapterError::from)?
        .map_err(|error| ImapAdapterError::Client(error.to_string()))
}

#[derive(Clone, Debug)]
pub(crate) struct UidExpungeTask {
    sequence_set: SequenceSet,
    output: Vec<NonZeroU32>,
}

impl UidExpungeTask {
    pub(crate) fn new(sequence_set: SequenceSet) -> Self {
        Self {
            sequence_set,
            output: Vec::new(),
        }
    }
}

impl Task for UidExpungeTask {
    type Output = Result<Vec<NonZeroU32>, TaskError>;

    fn command_body(&self) -> CommandBody<'static> {
        CommandBody::ExpungeUid {
            sequence_set: self.sequence_set.clone(),
        }
    }

    fn process_data(&mut self, data: Data<'static>) -> Option<Data<'static>> {
        if let Data::Expunge(seq) = data {
            self.output.push(seq);
            None
        } else {
            Some(data)
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
