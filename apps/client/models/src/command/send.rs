//! The send intent.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Content for [`crate::Command::Send`]. Hold semantics (undo-send window,
/// send-later time, originating draft) travel inside the request itself.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SendIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::SendMessageRequest")]
    pub request: domain::SendMessageRequest,
}
