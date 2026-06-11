use super::*;

/// Per-type, per-account JMAP state string used for delta sync.
///
/// @spec docs/L1-sync#state-management
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncCursor {
    pub object_type: SyncObject,
    pub state: String,
    pub updated_at: String,
}

impl SyncCursor {
    /// Return the provider state token stored in this cursor.
    ///
    /// Most cursors store the provider token directly. Some JMAP email cursors
    /// wrap the provider token with local metadata versioning so Posthaste can
    /// force a full metadata refresh when its projection changes.
    pub fn provider_state(&self) -> String {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&self.state) else {
            return self.state.clone();
        };
        if value.get("kind").and_then(serde_json::Value::as_str) != Some("jmap-email") {
            return self.state.clone();
        }
        value
            .get("state")
            .and_then(serde_json::Value::as_str)
            .map(String::from)
            .unwrap_or_else(|| self.state.clone())
    }
}

/// JMAP object type that participates in delta sync.
///
/// @spec docs/L1-sync#state-management
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncObject {
    Mailbox,
    Message,
}

impl SyncObject {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mailbox => "mailbox",
            Self::Message => "message",
        }
    }
}

/// User-requested sync mode.
///
/// @spec docs/L1-sync#sync-loop
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SyncMode {
    #[default]
    Incremental,
    FullMetadata,
}

impl SyncMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Incremental => "incremental",
            Self::FullMetadata => "fullMetadata",
        }
    }

    pub fn requires_full_message_metadata(self) -> bool {
        matches!(self, Self::FullMetadata)
    }
}
