//! The one-click unsubscribe intent.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Target for [`crate::Command::Unsubscribe`]: execute the message's
/// RFC 8058 one-click unsubscribe. The POST to the list server happens
/// backend-side (https-only, credential-free, no redirect downgrade); the
/// mailto: path is ordinary compose. Send this only after explicit user
/// confirmation — the outcome surfaces as an event, not an HTTP reply.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::MessageId")]
    pub message_id: domain::MessageId,
}
