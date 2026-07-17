//! Draft intents: create, update, discard.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Content for [`crate::Command::CreateDraft`]. The draft's stable id
/// travels in `draft.draftId`.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreateDraftIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::SendMessageRequest")]
    pub draft: domain::SendMessageRequest,
}

/// Target + content for [`crate::Command::UpdateDraft`].
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDraftIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    /// The stable draft id (survives provider id rotation).
    pub draft_id: String,
    #[ts(as = "crate::mirror::SendMessageRequest")]
    pub draft: domain::SendMessageRequest,
}

/// Target for [`crate::Command::DiscardDraft`].
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DiscardDraftIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    /// The stable draft id (survives provider id rotation).
    pub draft_id: String,
}
