use imap_client::imap_types::{flag::Flag, IntoStatic};
use posthaste_domain_model::MailboxId;
use posthaste_domain_model::SystemKeyword;

use crate::ImapAdapterError;

pub(crate) const IMAP_FLAG_FORWARDED: &str = "\\Forwarded";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImapMailboxReplacementDelta {
    pub add: Vec<MailboxId>,
    pub remove: Vec<MailboxId>,
}

pub fn imap_mailbox_replacement_delta(
    current_mailbox_ids: &[MailboxId],
    target_mailbox_ids: &[MailboxId],
) -> ImapMailboxReplacementDelta {
    let current = current_mailbox_ids
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let target = target_mailbox_ids
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();

    ImapMailboxReplacementDelta {
        add: target.difference(&current).cloned().collect(),
        remove: current.difference(&target).cloned().collect(),
    }
}

pub fn imap_flags_for_keywords(
    keywords: &[String],
) -> Result<Vec<Flag<'static>>, ImapAdapterError> {
    keywords
        .iter()
        .map(|keyword| imap_flag_for_keyword(keyword))
        .collect()
}

fn imap_flag_for_keyword(keyword: &str) -> Result<Flag<'static>, ImapAdapterError> {
    let normalized_keyword = keyword.to_ascii_lowercase();
    let flag = match SystemKeyword::parse(&normalized_keyword) {
        Some(system_keyword) => imap_flag_for_system_keyword(system_keyword),
        None => Flag::try_from(keyword)
            .map_err(|error| ImapAdapterError::InvalidKeywordFlag {
                keyword: keyword.to_string(),
                reason: error.to_string(),
            })?
            .into_static(),
    };

    Ok(flag)
}

fn imap_flag_for_system_keyword(keyword: SystemKeyword) -> Flag<'static> {
    match keyword {
        SystemKeyword::Seen => Flag::Seen,
        SystemKeyword::Flagged => Flag::Flagged,
        SystemKeyword::Answered => Flag::Answered,
        SystemKeyword::Draft => Flag::Draft,
        SystemKeyword::Forwarded => {
            Flag::try_from(IMAP_FLAG_FORWARDED).expect("static IMAP flag is valid")
        }
    }
}
