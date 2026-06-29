use super::*;

pub(super) type AppliedBodyRecord = (MessageId, Option<String>, Option<String>);

pub(super) struct TestStore {
    pub(super) smart_mailbox_counts_error: Option<String>,
    pub(super) list_mailboxes_error: Option<String>,
    pub(super) messages_error: Option<String>,
    pub(super) projection_calls: Mutex<Vec<String>>,
    pub(super) projection_deletes: Mutex<Vec<String>>,
    pub(super) source_data_deletes: Mutex<Vec<String>>,
    pub(super) automation_backfill_jobs: Mutex<Vec<AutomationBackfillJob>>,
    pub(super) cache_candidates: Mutex<Vec<CacheCandidate>>,
    pub(super) cache_signal_updates: Mutex<Vec<CacheSignalUpdate>>,
    pub(super) cache_rescore_candidates: Mutex<Vec<CacheRescoreCandidate>>,
    pub(super) stale_cache_rescore_requests: Mutex<Vec<(AccountId, String, usize)>>,
    pub(super) stale_cache_rescore_result: usize,
    pub(super) cache_priority_updates: Mutex<Vec<CachePriorityUpdate>>,
    pub(super) cache_fetch_candidates: Mutex<Vec<CacheFetchCandidate>>,
    pub(super) cache_state_changes: Mutex<Vec<(MessageId, CacheObjectState, Option<String>)>>,
    pub(super) cache_used_bytes: Mutex<u64>,
    pub(super) applied_bodies: Mutex<Vec<AppliedBodyRecord>>,
    pub(super) apply_body_error: Option<String>,
    pub(super) keyword_adds: Mutex<Vec<(MessageId, Vec<String>)>>,
    pub(super) rule_page: Mutex<Vec<MessageSummary>>,
    pub(super) conversation_view: Mutex<Option<ConversationView>>,
    pub(super) mutation_state: Mutex<MutationStoreState>,
    pub(super) outbox_operations: Mutex<Vec<Operation>>,
    /// (account_id, draft_key, entity_id) alias rows.
    pub(super) draft_aliases: Mutex<Vec<(String, String, String)>>,
    /// Snooze return rows for the scheduler: (message_id, until_unix_secs).
    pub(super) snoozes: Mutex<Vec<(MessageId, i64)>>,
}

impl Default for TestStore {
    fn default() -> Self {
        Self {
            smart_mailbox_counts_error: None,
            list_mailboxes_error: None,
            messages_error: None,
            projection_calls: Mutex::new(Vec::new()),
            projection_deletes: Mutex::new(Vec::new()),
            source_data_deletes: Mutex::new(Vec::new()),
            automation_backfill_jobs: Mutex::new(Vec::new()),
            cache_candidates: Mutex::new(Vec::new()),
            cache_signal_updates: Mutex::new(Vec::new()),
            cache_rescore_candidates: Mutex::new(Vec::new()),
            stale_cache_rescore_requests: Mutex::new(Vec::new()),
            stale_cache_rescore_result: 0,
            cache_priority_updates: Mutex::new(Vec::new()),
            cache_fetch_candidates: Mutex::new(Vec::new()),
            cache_state_changes: Mutex::new(Vec::new()),
            cache_used_bytes: Mutex::new(0),
            applied_bodies: Mutex::new(Vec::new()),
            apply_body_error: None,
            keyword_adds: Mutex::new(Vec::new()),
            rule_page: Mutex::new(Vec::new()),
            conversation_view: Mutex::new(None),
            mutation_state: Mutex::new(MutationStoreState::default()),
            outbox_operations: Mutex::new(Vec::new()),
            draft_aliases: Mutex::new(Vec::new()),
            snoozes: Mutex::new(Vec::new()),
        }
    }
}

#[derive(Default)]
pub(super) struct MutationStoreState {
    pub(super) cursor: Option<SyncCursor>,
    pub(super) mailbox_ids: Vec<MailboxId>,
    /// Message count of each `apply_sync_batch` call, in order — lets streaming
    /// tests assert chunks are applied progressively rather than as one batch.
    pub(super) applied_message_chunks: Vec<usize>,
    /// Number of `reconcile_sync` calls.
    pub(super) reconcile_calls: usize,
}

impl TestStore {
    pub(super) fn with_message_state(cursor_state: &str, mailbox_ids: &[&str]) -> Self {
        Self {
            mutation_state: Mutex::new(MutationStoreState {
                cursor: Some(SyncCursor {
                    object_type: SyncObject::Message,
                    state: cursor_state.to_string(),
                    updated_at: crate::RFC3339_EPOCH.to_string(),
                }),
                mailbox_ids: mailbox_ids.iter().map(|id| MailboxId::from(*id)).collect(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
}
