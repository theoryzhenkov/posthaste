//! Typed vocabularies for serialized domain strings.

use serde::{Deserialize, Serialize};

/// Well-known mailbox roles used by JMAP and local IMAP SPECIAL-USE mapping.
///
/// @spec docs/L1-api#mailbox-metadata
/// @spec docs/L1-accounts#smart-mailbox-defaults
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
    #[serde(rename = "snooze")]
    Snooze,
}

impl MailboxRole {
    pub const ALL: [Self; 7] = [
        Self::Inbox,
        Self::Archive,
        Self::Drafts,
        Self::Sent,
        Self::Junk,
        Self::Trash,
        Self::Snooze,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Archive => "archive",
            Self::Drafts => "drafts",
            Self::Sent => "sent",
            Self::Junk => "junk",
            Self::Trash => "trash",
            Self::Snooze => "snooze",
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
            "snooze" => Some(Self::Snooze),
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
/// @spec docs/L1-api#application-settings
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
mod tests;
