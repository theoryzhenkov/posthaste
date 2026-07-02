use serde::{Deserialize, Serialize};

/// Notification policy — the user's new-mail/sound preferences, stored in
/// `[notifications]` of `app.toml` as the single source of truth. OS-level
/// delivery permission stays device-local (not a config concern).
///
/// @spec docs/eph/RFC-L2-configuration-matrix
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Notifications {
    pub new_mail: Option<bool>,
    pub sound: Option<bool>,
}
