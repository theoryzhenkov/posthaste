//! The thread family: all messages of one provider thread.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Read all messages of one provider thread — answered with the thread view
/// (`ThreadView`).
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadQuery {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::ThreadId")]
    pub thread_id: domain::ThreadId,
}
