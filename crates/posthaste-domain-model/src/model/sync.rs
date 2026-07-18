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

/// Outcome of a streamed sync. When `reconciliation` is `Some`, the gateway
/// streamed upsert-only chunks and the service must run one final pass to prune
/// locals absent from the complete remote set and advance cursors. When `None`,
/// the stream already self-reconciled (e.g. a single full batch), so no final
/// pass runs.
///
#[derive(Clone, Debug, Default)]
pub struct SyncOutcome {
    pub reconciliation: Option<SyncReconciliation>,
}

impl SyncOutcome {
    /// A single self-reconciling batch: the chunk carried its own `replace_all`
    /// pruning and cursors, so no final reconciliation pass is needed.
    pub fn single_batch() -> Self {
        Self {
            reconciliation: None,
        }
    }
}

/// The final-pass reconciliation set gathered across streamed upsert-only
/// chunks: the complete remote ids to retain, which object types to prune, and
/// the cursors to commit only once the full stream succeeded.
///
#[derive(Clone, Debug, Default)]
pub struct SyncReconciliation {
    pub remote_message_ids: Vec<MessageId>,
    pub remote_mailbox_ids: Vec<MailboxId>,
    pub prune_messages: bool,
    pub prune_mailboxes: bool,
    pub cursors: Vec<SyncCursor>,
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

/// One resource touched by a sync cycle, listed on the `sync.completed`
/// payload for scripting/tap consumers.
///
/// @spec docs/L1-sync#event-propagation
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SyncResourceRef {
    /// Resource family, e.g. `sync`, `mailbox`, `message`.
    pub kind: String,
    /// What happened to it, e.g. `completed`, `refreshed`.
    pub operation: String,
    pub account_id: AccountId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<SyncMode>,
}

/// Payload of the `sync.completed` event: the cycle's counters plus the
/// resources it touched.
///
/// @spec docs/L1-sync#event-propagation
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SyncCompletedPayload {
    pub mailbox_count: usize,
    pub message_count: usize,
    pub deleted_imap_location_count: usize,
    pub deleted_message_count: usize,
    /// Automation events recorded while applying this cycle's batches.
    pub automation_event_count: usize,
    pub trigger: SyncTrigger,
    pub mode: SyncMode,
    pub resources: Vec<SyncResourceRef>,
    /// Error codes from post-commit work (e.g. the automation outbox flush)
    /// that failed after the sync batch itself committed.
    pub post_commit_errors: Vec<String>,
}
