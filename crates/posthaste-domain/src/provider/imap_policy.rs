use super::*;

/// IMAP provider policy selected for mailbox planning and projection.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImapProviderPolicy {
    features: ImapProviderFeatures,
    required_full_sync_reason: Option<ImapFullSyncReason>,
    remote_observation: RemoteObservationPolicy,
    mailbox_role_aliases: ImapMailboxRoleAliasPolicy,
}

impl ImapProviderPolicy {
    pub fn for_kind(kind: ProviderKind) -> Self {
        match kind {
            ProviderKind::Gmail => Self {
                features: ImapProviderFeatures {
                    message_identity: ImapMessageIdentitySource::GmailMessageId,
                    thread_identity: ImapThreadIdentitySource::GmailThreadId,
                    label_source: ImapLabelSource::GmailLabels,
                },
                // Gmail's X-GM-EXT-1 extension gives every message a stable
                // X-GM-MSGID and a complete X-GM-LABELS list, so delta syncs can
                // canonicalize and project mailbox membership from any single
                // observed copy. Full snapshots are no longer required for
                // canonicalization once stored MODSEQ state is available.
                required_full_sync_reason: None,
                remote_observation: RemoteObservationPolicy::selected_mailbox_idle()
                    .with_incomplete_hints(),
                mailbox_role_aliases: ImapMailboxRoleAliasPolicy::Gmail,
            },
            ProviderKind::Generic | ProviderKind::Outlook | ProviderKind::Icloud => Self {
                features: ImapProviderFeatures {
                    message_identity: ImapMessageIdentitySource::UidValidityUid,
                    thread_identity: ImapThreadIdentitySource::Rfc5322Headers,
                    label_source: ImapLabelSource::MailboxMembership,
                },
                required_full_sync_reason: None,
                remote_observation: RemoteObservationPolicy::selected_mailbox_idle(),
                mailbox_role_aliases: ImapMailboxRoleAliasPolicy::StandardOnly,
            },
        }
    }

    pub fn features(self) -> ImapProviderFeatures {
        self.features
    }

    pub fn required_full_sync_reason(self) -> Option<ImapFullSyncReason> {
        self.required_full_sync_reason
    }

    pub fn allows_status_skip(self) -> bool {
        self.required_full_sync_reason.is_none()
    }

    pub fn canonicalizes_by_rfc5322_message_id(self) -> bool {
        self.features.message_identity == ImapMessageIdentitySource::Rfc5322MessageId
    }

    pub fn canonicalizes_by_gmail_message_id(self) -> bool {
        self.features.message_identity == ImapMessageIdentitySource::GmailMessageId
    }

    pub fn remote_observation(self) -> RemoteObservationPolicy {
        self.remote_observation
    }

    pub fn mailbox_role(
        self,
        mailbox_name: &str,
        attributes: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Option<&'static str> {
        let normalized = attributes
            .into_iter()
            .map(|attribute| attribute.as_ref().to_ascii_uppercase())
            .collect::<BTreeSet<_>>();

        crate::imap_special_use_role(mailbox_name, normalized.iter().map(String::as_str)).or_else(
            || match self.mailbox_role_aliases {
                ImapMailboxRoleAliasPolicy::StandardOnly => None,
                ImapMailboxRoleAliasPolicy::Gmail => gmail_mailbox_role_alias(&normalized),
            },
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ImapMailboxRoleAliasPolicy {
    StandardOnly,
    Gmail,
}

fn gmail_mailbox_role_alias(attributes: &BTreeSet<String>) -> Option<&'static str> {
    if attributes.contains("\\SPAM") {
        Some(MailboxRole::Junk.as_str())
    } else if attributes.contains("\\ALL") {
        Some(MailboxRole::Archive.as_str())
    } else {
        None
    }
}
