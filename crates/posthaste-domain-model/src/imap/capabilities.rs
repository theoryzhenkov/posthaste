use super::*;

/// Normalized IMAP server capabilities used by the sync planner.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImapCapabilities {
    tokens: BTreeSet<String>,
}

impl ImapCapabilities {
    pub fn from_tokens(tokens: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let tokens = tokens
            .into_iter()
            .map(|token| token.as_ref().to_ascii_uppercase())
            .collect();
        Self { tokens }
    }

    pub fn contains(&self, token: &str) -> bool {
        self.tokens.contains(&token.to_ascii_uppercase())
    }

    /// The normalized capability tokens as a space-joined string, for
    /// diagnostics (e.g. to confirm whether a server actually advertised
    /// `CONDSTORE`/`QRESYNC` rather than inferring from the derived flags).
    pub fn joined(&self) -> String {
        self.tokens
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn supports_enable(&self) -> bool {
        self.contains("ENABLE")
    }

    pub fn supports_idle(&self) -> bool {
        self.contains("IDLE")
    }

    pub fn supports_special_use(&self) -> bool {
        self.contains("SPECIAL-USE") || self.contains("IMAP4REV2")
    }

    pub fn supports_uidplus(&self) -> bool {
        self.contains("UIDPLUS") || self.contains("IMAP4REV2")
    }

    pub fn supports_move(&self) -> bool {
        self.contains("MOVE") || self.contains("IMAP4REV2")
    }

    pub fn supports_condstore(&self) -> bool {
        self.contains("CONDSTORE") || self.supports_qresync()
    }

    pub fn supports_qresync(&self) -> bool {
        self.contains("QRESYNC")
    }

    pub fn supports_gmail_extensions(&self) -> bool {
        self.contains("X-GM-EXT-1")
    }
}

/// Remote identity source used to deduplicate IMAP messages.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapMessageIdentitySource {
    UidValidityUid,
    Rfc5322MessageId,
    GmailMessageId,
}

/// Remote thread source used when projecting conversations.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapThreadIdentitySource {
    Rfc5322Headers,
    GmailThreadId,
}

/// Source for mailbox/tag membership on IMAP accounts.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapLabelSource {
    MailboxMembership,
    GmailLabels,
}

/// Provider features inferred from IMAP capabilities.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImapProviderFeatures {
    pub message_identity: ImapMessageIdentitySource,
    pub thread_identity: ImapThreadIdentitySource,
    pub label_source: ImapLabelSource,
}

impl ImapProviderFeatures {
    pub fn from_capabilities(capabilities: &ImapCapabilities) -> Self {
        ProviderProfile::from_imap_capabilities(capabilities)
            .imap()
            .features()
    }

    pub fn for_provider_kind(kind: ProviderKind) -> Self {
        ProviderProfile::from_kind(kind).imap().features()
    }
}
