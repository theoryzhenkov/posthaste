//! The runtime's read path as a read-through cache over the backend.
//!
//! Reads ([replication backend-link L3](../replication/backend-link/L3.md))
//! go through a [`ReadCache`] over a [`BackendApi`]: the query engine lives at
//! the authority (the far node), and a near node retains the data that flowed
//! back under a **policy** chosen from link cost. The primitive is read-through;
//! caching is the optimization.
//!
//! There is no separate read-source abstraction — the cache wraps the one
//! `BackendApi` (the in-process `LocalBackend` co-located, the `RemoteBackend`
//! over the link), the same trait the write/subscribe channels use. Co-located
//! the policy is **passthrough** (retain nothing, read straight through), so the
//! deployment behaves exactly as before (`colocated-unchanged`); a split runtime
//! gets the **retaining** policy kept coherent by the down-channel.
//!
//! @spec docs/replication/backend-link/L3#2-retaining-policy-and-coherence-by-eviction

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use std::collections::BTreeMap;

use posthaste_domain::{
    now_iso8601, AccountId, AccountOverview, AppSettings, CachedSenderAddress, ConversationId,
    ConversationView, DomainEvent, DraftContent, EventFilter, Identity, MailboxSummary,
    MessageDetail, MessageId, MessageSummary, Operation, ReplyContext, SmartMailbox, SmartMailboxId,
    SmartMailboxSummary, TagSummary, EVENT_TOPIC_MESSAGE_UPDATED,
};
use posthaste_link_contract::{
    BackendApi, BackendLink, BaseAssertion, BaseUpdate, DownFrame, LinkCoverage,
};
use tokio::sync::broadcast;
use posthaste_runtime_contract::{
    AccountScopeRequest, MailQueryPage, MailQueryRequest, MessageResourceKind, RuntimeAccountList,
    RuntimeError, RuntimeResourceBytes,
};

/// A read-through cache over the [`BackendApi`], parameterized by policy.
///
/// - **Passthrough** (co-located): every read delegates straight to the backend,
///   retaining nothing — no redundant storage, behavior-preserving.
/// - **Retaining** (remote): point reads are served from a coherent summary
///   cache of the messages that flowed back (from point reads *and* query
///   pages), and read through on a miss. The cache is kept correct by the
///   down-channel ([`run_backend_down_channel`]): a message's entry is evicted when
///   the backend says it changed, so it is never stale — the next read
///   re-fetches. (Mail-list pages are not cached; the authority computes
///   membership/order, so a list is always a fresh read-through that warms the
///   message cache.)
///
/// The summary cache is keyed by message id alone (as the outbox is): a
/// down-channel assertion carries the message id, so eviction is by id and may
/// over-evict across accounts — which is safe (a cache miss), only less
/// efficient. Carrying the account id on the assertion is a later refinement.
pub struct ReadCache {
    backend: Arc<dyn BackendApi>,
    summaries: Option<Mutex<HashMap<String, MessageSummary>>>,
}

impl ReadCache {
    /// The passthrough cache: read straight through, retain nothing.
    pub fn passthrough(backend: Arc<dyn BackendApi>) -> Self {
        Self {
            backend,
            summaries: None,
        }
    }

    /// The retaining cache: hold the summaries that flow back, kept coherent by
    /// the down-channel.
    pub fn retaining(backend: Arc<dyn BackendApi>) -> Self {
        Self {
            backend,
            summaries: Some(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        let page = self.backend.query_mail_page(request).await?;
        // A list read-through warms the message cache with what it returned.
        if let (Some(summaries), MailQueryPage::Messages(messages)) = (&self.summaries, &page) {
            let mut cache = summaries.lock().expect("summary cache poisoned");
            for message in &messages.items {
                cache.insert(message.id.as_str().to_string(), message.clone());
            }
        }
        Ok(page)
    }

    /// Read a message's detail through the backend (passthrough; the detail view
    /// is recomputed per open, so it is not cached here).
    pub(crate) async fn message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        self.backend
            .message_detail(account_id.clone(), message_id.clone())
            .await
    }

    /// Read a conversation through the backend (passthrough).
    pub(crate) async fn conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<ConversationView, RuntimeError> {
        self.backend.conversation(conversation_id.clone()).await
    }

    /// Read the backend's live account count (passthrough; status metadata).
    pub(crate) async fn account_count(&self) -> Result<Option<usize>, RuntimeError> {
        self.backend.account_count().await
    }

    /// Account/config reads through the backend (passthrough; not cached — these
    /// are config metadata, re-read on demand).
    pub(crate) async fn list_accounts(&self) -> Result<RuntimeAccountList, RuntimeError> {
        self.backend.list_accounts().await
    }

    pub(crate) async fn get_account(
        &self,
        account_id: AccountId,
    ) -> Result<Option<AccountOverview>, RuntimeError> {
        self.backend.get_account(account_id).await
    }

    pub(crate) async fn app_settings(&self) -> Result<AppSettings, RuntimeError> {
        self.backend.app_settings().await
    }

    pub(crate) async fn resolve_account_scope(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<Vec<AccountId>, RuntimeError> {
        self.backend.resolve_account_scope(scope).await
    }

    pub(crate) async fn list_mailboxes(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<BTreeMap<AccountId, Vec<MailboxSummary>>, RuntimeError> {
        self.backend.list_mailboxes(scope).await
    }

    pub(crate) async fn list_smart_mailboxes(
        &self,
    ) -> Result<Vec<SmartMailboxSummary>, RuntimeError> {
        self.backend.list_smart_mailboxes().await
    }

    pub(crate) async fn get_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<SmartMailbox, RuntimeError> {
        self.backend.get_smart_mailbox(smart_mailbox_id).await
    }

    pub(crate) async fn list_tags(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<Vec<TagSummary>, RuntimeError> {
        self.backend.list_tags(scope).await
    }

    /// Provider/account reads through the backend (passthrough; not cached —
    /// these resolve a live gateway or read config/outbox state on demand).
    pub(crate) async fn get_identity(
        &self,
        account_id: AccountId,
    ) -> Result<Identity, RuntimeError> {
        self.backend.get_identity(account_id).await
    }

    pub(crate) async fn get_reply_context(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<ReplyContext, RuntimeError> {
        self.backend.get_reply_context(account_id, message_id).await
    }

    pub(crate) async fn list_sender_addresses(
        &self,
    ) -> Result<Vec<CachedSenderAddress>, RuntimeError> {
        self.backend.list_sender_addresses().await
    }

    pub(crate) async fn list_pending_operations(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<Operation>, RuntimeError> {
        self.backend.list_pending_operations(account_id).await
    }

    pub(crate) async fn replay_events(
        &self,
        filter: EventFilter,
    ) -> Result<Vec<DomainEvent>, RuntimeError> {
        self.backend.replay_events(filter).await
    }

    pub(crate) async fn get_draft_content(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<DraftContent, RuntimeError> {
        self.backend.get_draft_content(account_id, message_id).await
    }

    pub(crate) async fn get_message_resource(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        kind: MessageResourceKind,
    ) -> Result<RuntimeResourceBytes, RuntimeError> {
        self.backend
            .get_message_resource(account_id, message_id, kind)
            .await
    }

    pub(crate) async fn current_summary(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageSummary>, RuntimeError> {
        if let Some(summaries) = &self.summaries {
            // Bind (and drop) the guard before the branch so the lock is not
            // held across the `if let` body.
            let hit = summaries
                .lock()
                .expect("summary cache poisoned")
                .get(message_id.as_str())
                .cloned();
            if let Some(hit) = hit {
                return Ok(Some(hit));
            }
        }
        let result = self
            .backend
            .current_summary(account_id.clone(), message_id.clone())
            .await?;
        if let (Some(summaries), Some(summary)) = (&self.summaries, &result) {
            summaries
                .lock()
                .expect("summary cache poisoned")
                .insert(message_id.as_str().to_string(), summary.clone());
        }
        Ok(result)
    }

    /// Drop a message's cached summary (it changed authoritatively). No-op for a
    /// passthrough cache.
    pub(crate) fn evict(&self, message_id: &str) {
        if let Some(summaries) = &self.summaries {
            summaries
                .lock()
                .expect("summary cache poisoned")
                .remove(message_id);
        }
    }

    /// Apply one down-channel frame for coherence: evict the messages a base
    /// assertion changed (present or removed), so the next read re-fetches.
    pub(crate) fn apply_coherence_frame(&self, frame: &DownFrame) {
        if let DownFrame::Base { assertions } = frame {
            for assertion in assertions {
                self.evict(&assertion.message_id);
            }
        }
    }
}

/// Consume the backend's down-channel and drive the near node from it: evict the
/// read cache (so the next read re-fetches) AND republish each base assertion as
/// a domain event on the runtime's event bus, so the existing view machinery
/// (`ViewRegistry`'s event pump + `subscribe_events`) recomputes and pushes
/// frames to clients. This is the runtime↔backend half of the recursive
/// down-channel; the runtime→client half is the shipped view-frame protocol it
/// feeds ([replication backend-link L3](../replication/backend-link/L3.md)). In-process the runtime
/// shares the backend's event bus directly, so this is spawned only for a split
/// (remote) runtime; it returns when the stream closes.
pub(crate) async fn run_backend_down_channel(
    link: BackendLink,
    reads: Arc<ReadCache>,
    events: broadcast::Sender<DomainEvent>,
) {
    let Ok(mut stream) = link.subscribe(LinkCoverage::Complete).await else {
        return;
    };
    // A monotonic local sequence for the synthesized events. These do NOT match
    // the backend's authoritative seqs (those come via `replay_events` over the
    // link); they only order the live stream for fresh subscribers.
    let mut seq: i64 = 0;
    while let Some(frame) = stream.next().await {
        reads.apply_coherence_frame(&frame);
        if let DownFrame::Base { assertions } = &frame {
            for assertion in assertions {
                seq += 1;
                // A send error means there are no live subscribers yet; the next
                // read still re-fetches (the cache was evicted above).
                let _ = events.send(down_assertion_to_event(assertion, seq));
            }
        }
    }
}

/// Map a down-channel base assertion to the domain event the near node's view
/// machinery already understands: a `message.updated` over the asserted message
/// (or a deletion). The change flags are broad — the view layer re-reads through
/// the cache and suppresses no-op recomputes — and the account id (carried on the
/// assertion) scopes per-account views like `messageDetail`.
fn down_assertion_to_event(assertion: &BaseAssertion, seq: i64) -> DomainEvent {
    let payload = if matches!(assertion.update, BaseUpdate::Removed) {
        serde_json::json!({ "messageId": assertion.message_id, "deleted": true })
    } else {
        serde_json::json!({
            "messageId": assertion.message_id,
            "changes": { "keywords": true, "mailboxes": true },
        })
    };
    DomainEvent {
        seq,
        account_id: AccountId(assertion.account_id.clone()),
        topic: EVENT_TOPIC_MESSAGE_UPDATED.to_string(),
        occurred_at: now_iso8601().unwrap_or_default(),
        mailbox_id: None,
        message_id: Some(MessageId(assertion.message_id.clone())),
        payload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    use async_trait::async_trait;
    use posthaste_domain::MessagePage;
    use posthaste_link_contract::{BaseAssertion, BaseUpdate, DownStream};
    use posthaste_link_core::MessageFoldState;
    use posthaste_runtime_contract::{MutationReceipt, MutationRequest};
    use serde_json::json;

    fn summary(id: &str) -> MessageSummary {
        serde_json::from_value(json!({
            "id": id, "sourceId": "acct", "sourceName": "Acct", "sourceThreadId": "t1",
            "conversationId": "c1", "subject": "S", "fromName": null, "fromEmail": null,
            "to": [], "preview": null, "receivedAt": "2026-06-24T00:00:00Z",
            "hasAttachment": false, "isRead": false, "isFlagged": false,
            "mailboxIds": ["inbox"], "keywords": []
        }))
        .unwrap()
    }

    /// A backend that counts how often each read reaches it. The write/subscribe
    /// channels are inert (these tests exercise reads only).
    struct CountingBackend {
        summary_calls: AtomicU64,
        query_calls: AtomicU64,
        page: MailQueryPage,
    }

    impl CountingBackend {
        fn new(items: Vec<MessageSummary>) -> Self {
            Self {
                summary_calls: AtomicU64::new(0),
                query_calls: AtomicU64::new(0),
                page: MailQueryPage::Messages(MessagePage {
                    items,
                    next_cursor: None,
                }),
            }
        }
    }

    #[async_trait]
    impl BackendApi for CountingBackend {
        async fn forward_mutation(
            &self,
            _mutation: MutationRequest,
        ) -> Result<MutationReceipt, RuntimeError> {
            Err(RuntimeError::internal("counting backend is read-only", None))
        }

        async fn subscribe(&self, _coverage: LinkCoverage) -> Result<DownStream, RuntimeError> {
            Ok(Box::pin(futures_util::stream::empty()))
        }

        async fn query_mail_page(
            &self,
            _request: MailQueryRequest,
        ) -> Result<MailQueryPage, RuntimeError> {
            self.query_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.page.clone())
        }

        async fn current_summary(
            &self,
            _account_id: AccountId,
            message_id: MessageId,
        ) -> Result<Option<MessageSummary>, RuntimeError> {
            self.summary_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Some(summary(message_id.as_str())))
        }
    }

    fn base_frame(message_id: &str) -> DownFrame {
        DownFrame::Base {
            assertions: vec![BaseAssertion {
                account_id: "acct".to_string(),
                message_id: message_id.to_string(),
                update: BaseUpdate::Present(MessageFoldState::default()),
            }],
        }
    }

    #[tokio::test]
    async fn passthrough_never_retains() {
        let backend = Arc::new(CountingBackend::new(vec![]));
        let cache = ReadCache::passthrough(backend.clone());
        let account = AccountId("acct".into());
        let message = MessageId("m1".into());
        cache.current_summary(&account, &message).await.unwrap();
        cache.current_summary(&account, &message).await.unwrap();
        assert_eq!(backend.summary_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn retaining_serves_a_point_read_from_cache_after_the_first() {
        let backend = Arc::new(CountingBackend::new(vec![]));
        let cache = ReadCache::retaining(backend.clone());
        let account = AccountId("acct".into());
        let message = MessageId("m1".into());
        cache.current_summary(&account, &message).await.unwrap();
        cache.current_summary(&account, &message).await.unwrap();
        // Second read is a cache hit.
        assert_eq!(backend.summary_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_query_warms_the_message_cache() {
        let backend = Arc::new(CountingBackend::new(vec![summary("m1")]));
        let cache = ReadCache::retaining(backend.clone());
        let request: MailQueryRequest = serde_json::from_value(json!({
            "query": "in:acct/inbox",
            "presentation": {
                "kind": "messages", "limit": 10, "cursor": null,
                "sortField": "date", "sortDirection": "desc"
            }
        }))
        .unwrap();
        cache.query_mail_page(request).await.unwrap();
        // The point read is now served from the warmed cache, never reaching the backend.
        cache
            .current_summary(&AccountId("acct".into()), &MessageId("m1".into()))
            .await
            .unwrap();
        assert_eq!(backend.summary_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_coherence_frame_evicts_so_the_next_read_refetches() {
        let backend = Arc::new(CountingBackend::new(vec![]));
        let cache = ReadCache::retaining(backend.clone());
        let account = AccountId("acct".into());
        let message = MessageId("m1".into());
        cache.current_summary(&account, &message).await.unwrap();
        // The backend asserts m1 changed; the cached summary is evicted.
        cache.apply_coherence_frame(&base_frame("m1"));
        cache.current_summary(&account, &message).await.unwrap();
        assert_eq!(backend.summary_calls.load(Ordering::SeqCst), 2);
    }

    // A backend whose down-channel emits one Base assertion then closes, with a
    // counting point read so eviction is observable.
    struct BridgeBackend {
        summary_calls: AtomicU64,
    }

    #[async_trait]
    impl BackendApi for BridgeBackend {
        async fn forward_mutation(
            &self,
            _mutation: MutationRequest,
        ) -> Result<MutationReceipt, RuntimeError> {
            Err(RuntimeError::internal("bridge backend is read-only", None))
        }

        async fn subscribe(&self, _coverage: LinkCoverage) -> Result<DownStream, RuntimeError> {
            Ok(Box::pin(futures_util::stream::iter([DownFrame::Base {
                assertions: vec![BaseAssertion {
                    account_id: "acct".into(),
                    message_id: "m1".into(),
                    update: BaseUpdate::Present(MessageFoldState::default()),
                }],
            }])))
        }

        async fn current_summary(
            &self,
            _account_id: AccountId,
            message_id: MessageId,
        ) -> Result<Option<MessageSummary>, RuntimeError> {
            self.summary_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Some(summary(message_id.as_str())))
        }
    }

    #[tokio::test]
    async fn the_down_channel_bridge_evicts_the_cache_and_republishes_an_event() {
        let backend = Arc::new(BridgeBackend {
            summary_calls: AtomicU64::new(0),
        });
        let cache = Arc::new(ReadCache::retaining(backend.clone()));
        let account = AccountId("acct".into());
        let message = MessageId("m1".into());

        // Warm the cache (one fetch), then run the bridge over a one-frame link.
        cache.current_summary(&account, &message).await.unwrap();
        assert_eq!(backend.summary_calls.load(Ordering::SeqCst), 1);

        let (events, mut rx) = broadcast::channel(16);
        let link = BackendLink::new(backend.clone());
        // The stub stream closes after one frame, so the bridge returns.
        run_backend_down_channel(link, cache.clone(), events).await;

        // It republished the assertion as a `message.updated` domain event
        // scoped to the right account + message, flagged so views recompute.
        let event = rx.try_recv().expect("an event was republished");
        assert_eq!(event.topic, EVENT_TOPIC_MESSAGE_UPDATED);
        assert_eq!(event.account_id.as_str(), "acct");
        assert_eq!(event.message_id.as_ref().map(MessageId::as_str), Some("m1"));
        assert_eq!(event.payload["changes"]["keywords"], true);

        // And it evicted the cache: the next read re-fetches.
        cache.current_summary(&account, &message).await.unwrap();
        assert_eq!(backend.summary_calls.load(Ordering::SeqCst), 2);
    }
}
