//! Typed vocabularies for serialized domain strings.

use serde::{Deserialize, Serialize};

/// Well-known mailbox roles used by JMAP and local IMAP SPECIAL-USE mapping.
///
/// @spec docs/L1-api#mailbox-metadata
/// @spec docs/L1-accounts#smart-mailbox-defaults
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MailboxRole {
    #[serde(rename = "inbox")]
    Inbox,
    #[serde(rename = "archive")]
    Archive,
    #[serde(rename = "drafts")]
    Drafts,
    #[serde(rename = "sent")]
    Sent,
    #[serde(rename = "junk")]
    Junk,
    #[serde(rename = "trash")]
    Trash,
}

impl MailboxRole {
    pub const ALL: [Self; 6] = [
        Self::Inbox,
        Self::Archive,
        Self::Drafts,
        Self::Sent,
        Self::Junk,
        Self::Trash,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Archive => "archive",
            Self::Drafts => "drafts",
            Self::Sent => "sent",
            Self::Junk => "junk",
            Self::Trash => "trash",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "inbox" => Some(Self::Inbox),
            "archive" => Some(Self::Archive),
            "drafts" => Some(Self::Drafts),
            "sent" => Some(Self::Sent),
            "junk" => Some(Self::Junk),
            "trash" => Some(Self::Trash),
            _ => None,
        }
    }
}

/// Well-known JMAP system keywords.
///
/// Custom keywords must not start with `$`; this enum only names the system
/// keywords the domain currently treats as first-class.
///
/// @spec docs/L1-api#navigation
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SystemKeyword {
    #[serde(rename = "$seen")]
    Seen,
    #[serde(rename = "$draft")]
    Draft,
    #[serde(rename = "$flagged")]
    Flagged,
    #[serde(rename = "$answered")]
    Answered,
    #[serde(rename = "$forwarded")]
    Forwarded,
}

impl SystemKeyword {
    pub const ALL: [Self; 5] = [
        Self::Seen,
        Self::Draft,
        Self::Flagged,
        Self::Answered,
        Self::Forwarded,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Seen => "$seen",
            Self::Draft => "$draft",
            Self::Flagged => "$flagged",
            Self::Answered => "$answered",
            Self::Forwarded => "$forwarded",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "$seen" => Some(Self::Seen),
            "$draft" => Some(Self::Draft),
            "$flagged" => Some(Self::Flagged),
            "$answered" => Some(Self::Answered),
            "$forwarded" => Some(Self::Forwarded),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_roles_preserve_serialized_strings() {
        let values = MailboxRole::ALL
            .into_iter()
            .map(MailboxRole::as_str)
            .collect::<Vec<_>>();

        assert_eq!(
            values,
            ["inbox", "archive", "drafts", "sent", "junk", "trash"]
        );
        assert_eq!(MailboxRole::parse("sent"), Some(MailboxRole::Sent));
        assert_eq!(MailboxRole::parse("Sent"), None);
        assert_eq!(MailboxRole::parse("all"), None);
        assert_eq!(
            serde_json::to_string(&MailboxRole::Inbox).expect("serialize role"),
            "\"inbox\""
        );
        assert_eq!(
            serde_json::from_str::<MailboxRole>("\"trash\"").expect("deserialize role"),
            MailboxRole::Trash
        );
    }

    #[test]
    fn system_keywords_preserve_serialized_strings() {
        let values = SystemKeyword::ALL
            .into_iter()
            .map(SystemKeyword::as_str)
            .collect::<Vec<_>>();

        assert_eq!(
            values,
            ["$seen", "$draft", "$flagged", "$answered", "$forwarded"]
        );
        assert_eq!(
            SystemKeyword::parse("$flagged"),
            Some(SystemKeyword::Flagged)
        );
        assert_eq!(SystemKeyword::parse("flagged"), None);
        assert_eq!(
            serde_json::to_string(&SystemKeyword::Seen).expect("serialize keyword"),
            "\"$seen\""
        );
        assert_eq!(
            serde_json::from_str::<SystemKeyword>("\"$draft\"").expect("deserialize keyword"),
            SystemKeyword::Draft
        );
    }
}
