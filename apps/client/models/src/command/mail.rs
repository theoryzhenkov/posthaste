//! Mail-mutation intents: keywords, mailbox membership, destroy.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Target + keyword change for [`crate::Command::SetKeywords`].
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SetKeywordsIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::MessageId")]
    pub message_id: domain::MessageId,
    #[ts(as = "crate::mirror::SetKeywordsCommand")]
    pub change: domain::SetKeywordsCommand,
}

/// Target + mailbox replacement for [`crate::Command::ReplaceMailboxes`].
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceMailboxesIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::MessageId")]
    pub message_id: domain::MessageId,
    #[ts(as = "crate::mirror::ReplaceMailboxesCommand")]
    pub change: domain::ReplaceMailboxesCommand,
}

/// Target for [`crate::Command::Destroy`].
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DestroyMessageIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::MessageId")]
    pub message_id: domain::MessageId,
}
