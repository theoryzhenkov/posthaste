//! The sender-addresses family: the compose autocomplete corpus.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Read the cached sender-address corpus (addresses seen on synced mail),
/// optionally for one account. The whole corpus is bounded; the client
/// filters as the user types.
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SenderAddressesQuery {
    #[serde(default)]
    #[ts(optional = nullable, as = "Option<crate::mirror::AccountId>")]
    pub account_id: Option<domain::AccountId>,
}

/// One cached address for compose autocomplete.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SenderAddressRow {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    pub name: Option<String>,
    pub email: String,
    /// When this address was last seen on a message (RFC 3339); drives
    /// autocomplete ranking.
    pub last_used_at: String,
}

/// The corpus in scope, most recently used first.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SenderAddressesResult {
    pub rows: Vec<SenderAddressRow>,
}
