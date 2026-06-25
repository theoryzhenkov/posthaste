mod keywords;
mod validation;

use imap_client::imap_types::flag::{Flag, StoreType};
use posthaste_domain::{ImapMessageLocation, MutationOutcome, SetKeywordsCommand};

use crate::discovery::connect_authenticated_client;
use crate::mutation::validation::{
    select_validated_mailbox, uid_expunge, uid_sequence_set, verify_uid_fetch_response,
};
use crate::{ImapAdapterError, ImapConnectionConfig};

pub use keywords::{
    imap_flags_for_keywords, imap_mailbox_replacement_delta, ImapMailboxReplacementDelta,
};

#[cfg(test)]
pub(crate) use keywords::IMAP_FLAG_FORWARDED;
#[cfg(test)]
pub(crate) use validation::{verify_message_data_contains_uid, UidExpungeTask};

/// Apply a JMAP keyword delta using UID STORE in the selected IMAP mailbox.
///
/// The command validates the stored UIDVALIDITY epoch before issuing STORE so a
/// stale UID cannot mutate a different message after provider-side UID reuse.
/// Keyword mutations use `.SILENT` because Posthaste already knows the intended
/// delta and real providers may omit or sparsely populate the untagged FETCH
/// response for accepted STORE commands.
///
/// @spec docs/L1-api#message-commands
pub async fn apply_imap_keyword_delta_by_location(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
    location: &ImapMessageLocation,
    command: &SetKeywordsCommand,
) -> Result<MutationOutcome, ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    select_validated_mailbox(&mut client, mailbox_name, location).await?;

    let uid_set = uid_sequence_set(location)?;
    let add_flags = imap_flags_for_keywords(&command.add)?;
    let remove_flags = imap_flags_for_keywords(&command.remove)?;
    if !add_flags.is_empty() {
        client
            .uid_silent_store(uid_set.clone(), StoreType::Add, add_flags)
            .await
            .map_err(ImapAdapterError::from)?;
    }
    if !remove_flags.is_empty() {
        client
            .uid_silent_store(uid_set, StoreType::Remove, remove_flags)
            .await
            .map_err(ImapAdapterError::from)?;
    }

    Ok(MutationOutcome {
        cursor: None,
        message: None,
    })
}

/// Copy one IMAP message to another mailbox.
///
/// `imap-client` currently exposes COPY success but not COPYUID output, so this
/// command validates the source UID before COPY and relies on the next sync to
/// discover the destination UID location.
///
/// @spec docs/L1-api#message-commands
pub async fn copy_imap_message_to_mailbox_by_location(
    config: &ImapConnectionConfig,
    source_mailbox_name: &str,
    location: &ImapMessageLocation,
    target_mailbox_name: &str,
) -> Result<(), ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    select_validated_mailbox(&mut client, source_mailbox_name, location).await?;
    verify_uid_fetch_response(&mut client, location).await?;
    client
        .uid_copy(uid_sequence_set(location)?, target_mailbox_name)
        .await
        .map_err(ImapAdapterError::from)
}

/// Move one IMAP message to another mailbox with UID MOVE.
///
/// `imap-client` exposes MOVE success but not COPYUID output. Even when the
/// server supports UIDPLUS, the adapter validates the source UID before MOVE
/// and relies on the next sync to discover the destination UID location.
///
/// @spec docs/L1-api#message-commands
pub async fn move_imap_message_to_mailbox_by_location(
    config: &ImapConnectionConfig,
    source_mailbox_name: &str,
    location: &ImapMessageLocation,
    target_mailbox_name: &str,
) -> Result<(), ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    select_validated_mailbox(&mut client, source_mailbox_name, location).await?;
    verify_uid_fetch_response(&mut client, location).await?;
    client
        .uid_move(uid_sequence_set(location)?, target_mailbox_name)
        .await
        .map_err(ImapAdapterError::from)
}

/// Mark one IMAP message as `\Deleted` without issuing broad EXPUNGE.
///
/// This avoids the RFC 6851/RFC 4315 footgun where plain EXPUNGE can remove
/// other clients' deleted messages. A later UID EXPUNGE wrapper can make this
/// a true permanent delete when the dependency exposes it.
///
/// @spec docs/L1-api#message-commands
pub async fn mark_imap_message_deleted_by_location(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
    location: &ImapMessageLocation,
) -> Result<MutationOutcome, ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    select_validated_mailbox(&mut client, mailbox_name, location).await?;
    verify_uid_fetch_response(&mut client, location).await?;
    client
        .uid_silent_store(uid_sequence_set(location)?, StoreType::Add, [Flag::Deleted])
        .await
        .map_err(ImapAdapterError::from)?;

    Ok(MutationOutcome {
        cursor: None,
        message: None,
    })
}

/// Mark and permanently expunge one IMAP message using UID EXPUNGE.
///
/// Only call this when the server advertises UIDPLUS or IMAP4rev2 support.
///
/// @spec docs/L1-api#message-commands
pub async fn expunge_imap_message_by_location(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
    location: &ImapMessageLocation,
) -> Result<MutationOutcome, ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    select_validated_mailbox(&mut client, mailbox_name, location).await?;
    verify_uid_fetch_response(&mut client, location).await?;
    client
        .uid_silent_store(uid_sequence_set(location)?, StoreType::Add, [Flag::Deleted])
        .await
        .map_err(ImapAdapterError::from)?;
    let _expunged = uid_expunge(&mut client, location).await?;

    Ok(MutationOutcome {
        cursor: None,
        message: None,
    })
}

#[cfg(test)]
mod tests;
