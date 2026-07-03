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
        // A UID-validity break means the provider's mailbox identity diverged
        // from ours — the folder must be resynced, not "rejected". Kept distinct
        // (audit top-10 #10) so it maps to `state_mismatch` and drives resync
        // rather than looking like a malformed request a user must fix.
        ImapAdapterError::UidValidityMismatch { .. } => GatewayError::StateMismatch,
        ImapAdapterError::MissingTransport
        | ImapAdapterError::MissingSmtpTransport
        | ImapAdapterError::MissingUsername
        | ImapAdapterError::MissingSmtpSenderEmail
        | ImapAdapterError::MissingSecret
        | ImapAdapterError::InvalidMailboxName(_)
        | ImapAdapterError::MissingSelectData(_)
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
        ImapAdapterError::Auth(_) => GatewayError::Auth,
        ImapAdapterError::Timeout { operation } => {
            GatewayError::Network(format!("{operation} timed out"))
        }
        ImapAdapterError::Client(message) | ImapAdapterError::Smtp(message) => {
            GatewayError::Network(message)
        }
    }
}

pub(crate) fn store_error_to_gateway(error: StoreError) -> GatewayError {
    // Corruption keeps its own class so the corrupt-store repair pathway
    // (`storage_corrupted`) survives the IMAP-op hop instead of masquerading as
    // a provider rejection (audit top-10 #4). Exhaustive: a new `StoreError`
    // variant must be classified here before it compiles.
    match &error {
        StoreError::Corruption(_) => {
            GatewayError::Corruption(format!("IMAP local state lookup hit a corrupt store: {error}"))
        }
        StoreError::NotFound(_) | StoreError::Conflict(_) | StoreError::Failure(_) => {
            GatewayError::Rejected(format!("IMAP local state lookup failed: {error}"))
        }
    }
}
