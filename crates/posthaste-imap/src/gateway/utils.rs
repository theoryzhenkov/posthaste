use super::*;

pub(crate) fn mailbox_status_proves_unchanged(
    stored: &ImapMailboxSyncState,
    local_message_count: usize,
    status: &ImapMailboxStatus,
) -> bool {
    if status.uid_validity != Some(stored.uid_validity) {
        return false;
    }
    if status.messages != Some(local_message_count as u32) {
        return false;
    }
    if let Some(stored_modseq) = stored.highest_modseq {
        return status.highest_modseq == Some(stored_modseq);
    }

    false
}

pub(crate) fn imap_error_to_gateway(error: ImapAdapterError) -> GatewayError {
    match error {
        ImapAdapterError::MissingTransport
        | ImapAdapterError::MissingSmtpTransport
        | ImapAdapterError::MissingUsername
        | ImapAdapterError::MissingSmtpSenderEmail
        | ImapAdapterError::MissingSecret
        | ImapAdapterError::InvalidMailboxName(_)
        | ImapAdapterError::MissingSelectData(_)
        | ImapAdapterError::UidValidityMismatch { .. }
        | ImapAdapterError::MissingFetchData(_)
        | ImapAdapterError::InvalidUidSequence(_)
        | ImapAdapterError::InvalidModSeq(_)
        | ImapAdapterError::InvalidKeywordFlag { .. }
        | ImapAdapterError::MissingMessageLocation(_)
        | ImapAdapterError::InvalidBlobId(_)
        | ImapAdapterError::ParseMessageHeaders
        | ImapAdapterError::ParseMessageBody
        | ImapAdapterError::MissingAttachment { .. }
        | ImapAdapterError::InvalidSmtpAddress { .. }
        | ImapAdapterError::BuildSmtpMessage(_) => GatewayError::Rejected(error.to_string()),
        ImapAdapterError::Client(message) | ImapAdapterError::Smtp(message) => {
            GatewayError::Network(message)
        }
    }
}

pub(crate) fn store_error_to_gateway(error: StoreError) -> GatewayError {
    GatewayError::Rejected(format!("IMAP local state lookup failed: {error}"))
}
