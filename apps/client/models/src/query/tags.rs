//! The tags family: keyword-derived tags with counts.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Read the tags in scope: one account's tags, or the merged set across all
/// accounts (same-named tags merged, counts summed) when `accountId` is
/// absent.
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TagsQuery {
    #[serde(default)]
    #[ts(optional = nullable, as = "Option<crate::mirror::AccountId>")]
    pub account_id: Option<domain::AccountId>,
}

/// The tags in scope with unread/total counts, in display order.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TagsResult {
    #[ts(as = "Vec<crate::mirror::TagSummary>")]
    pub rows: Vec<domain::TagSummary>,
}
