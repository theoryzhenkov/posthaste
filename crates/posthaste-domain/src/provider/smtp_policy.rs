use super::*;

/// SMTP provider policy selected after message submission.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmtpProviderPolicy {
    sent_copy: SmtpSentCopyPolicy,
}

impl SmtpProviderPolicy {
    pub fn for_kind(kind: ProviderKind) -> Self {
        let sent_copy = match kind {
            ProviderKind::Gmail | ProviderKind::Outlook => SmtpSentCopyPolicy::ProviderManaged,
            ProviderKind::Generic | ProviderKind::Icloud => SmtpSentCopyPolicy::AppendToSentMailbox,
        };
        Self { sent_copy }
    }

    pub fn sent_copy(self) -> SmtpSentCopyPolicy {
        self.sent_copy
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmtpSentCopyPolicy {
    ProviderManaged,
    AppendToSentMailbox,
}
