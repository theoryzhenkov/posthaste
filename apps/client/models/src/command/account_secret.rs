//! THE dedicated secret-bearing command. Account credential material —
//! passwords, app passwords — travels in [`SetAccountSecretIntent`] and
//! nowhere else on the whole API: not in settings payloads, not in account
//! patches, not in query answers (reads surface only the redacted
//! `SecretStatus`). Keeping the secret in one marked shape is what lets
//! logging, capability scoping, and review treat it specially.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The credential change: keep (a no-op placeholder for form round-trips),
/// replace with new material, or clear. Tagged, so the material can only
/// appear under an explicit `replace`.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AccountSecretChange {
    /// Leave the stored credential untouched.
    Keep,
    /// Replace the stored credential with this material.
    Replace {
        /// The secret material (password / app password). Handled as a
        /// secret end to end: stored through the OS keychain, never logged,
        /// never echoed back.
        secret: String,
    },
    /// Remove the stored credential.
    Clear,
}

/// Target + change for [`crate::Command::SetAccountSecret`].
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SetAccountSecretIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    pub change: AccountSecretChange,
}
