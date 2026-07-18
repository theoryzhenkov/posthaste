//! The optional-field patch vocabulary for update commands. Update intents
//! are sparse patches ("absent means keep"), which leaves a plain
//! `Option<T>` field unable to express "clear the stored value" — so the
//! clearable fields carry a [`FieldPatch`] instead, the same tagged shape as
//! [`crate::command::account_secret::AccountSecretChange`].

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One clearable field's change: leave the stored value untouched, set a new
/// one, or clear it. Tagged, so a clear is always an explicit statement — a
/// caller can never wipe a field by accident, and `keep` is the serde
/// default, so an absent field still means "preserve".
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FieldPatch<T> {
    /// Leave the stored value untouched.
    #[default]
    Keep,
    /// Replace the stored value.
    Set {
        /// The new value.
        value: T,
    },
    /// Remove the stored value.
    Clear,
}

impl<T> FieldPatch<T> {
    /// Fold the patch into the stored value.
    pub fn apply(self, target: &mut Option<T>) {
        match self {
            FieldPatch::Keep => {}
            FieldPatch::Set { value } => *target = Some(value),
            FieldPatch::Clear => *target = None,
        }
    }
}
