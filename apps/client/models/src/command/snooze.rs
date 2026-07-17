//! Snooze intents: park a message until a time, or return it now.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Target + wake time for [`crate::Command::Snooze`]. The backend moves the
/// message out of the inbox and returns it when `until` passes (the
/// auto-return scheduler runs server-side).
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SnoozeIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::MessageId")]
    pub message_id: domain::MessageId,
    /// When the message returns to the inbox (RFC 3339, wall time).
    pub until: String,
}

/// Target for [`crate::Command::Unsnooze`]: return the message to the inbox
/// now and clear its snooze.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UnsnoozeIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::MessageId")]
    pub message_id: domain::MessageId,
}
