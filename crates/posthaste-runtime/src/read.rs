//! The runtime's read path as a read-through cache over the authority server.
//!
//! Reads ([replication authority-server-link L3](../replication/authority-server-link/L3.md))
//! go through a [`ReadCache`] over an [`AuthorityServerApi`]: the query engine lives at
//! the authority (the far node), and a near node retains the data that flowed
//! back under a **policy** chosen from link cost. The primitive is read-through;
//! caching is the optimization.
//!
//! There is no separate read-source abstraction — the cache holds the **Api
//! half** of the D33 seam over the one config-selected transport (the
//! in-process `LocalAuthorityServer` co-located, the `RemoteAuthorityServer`
//! over the link); the replication channels are the same transport's
//! `AuthorityServerLink` half. Co-located
//! the policy is **passthrough** (retain nothing, read straight through), so the
//! deployment behaves exactly as before (`colocated-unchanged`); a split runtime
//! gets the **retaining** policy kept coherent by the down-channel.
//!
//! @spec docs/replication/authority-server-link/L3#2-retaining-policy-and-coherence-by-eviction

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use std::collections::BTreeMap;

use posthaste_domain_model::{
    now_iso8601, AccountId, AccountOverview, AppSettings, CachedSenderAddress, ConversationId,
    ConversationView, DomainEvent, DraftContent, EventFilter, Identity, MailboxSummary,
    MessageDetail, MessageId, MessageSummary, Operation, ReplyContext, RevLogSnapshot,
    SmartMailbox, SmartMailboxId, SmartMailboxSummary, TagSummary, EVENT_TOPIC_MESSAGE_UPDATED,
};
use posthaste_authority_server_link::{
    AuthorityServerApi, AuthorityServerFrame, AuthorityServerLinkHandle, BaseAssertion,
    BaseUpdate, LinkCoverage,
};
use posthaste_contract_core::{
    AccountScopeRequest, MailQueryPage, MailQueryRequest, MessageResourceKind, RuntimeAccountList,
    RuntimeError, RuntimeResourceBytes,
};
use tokio::sync::broadcast;

/// A read-through cache over the [`AuthorityServerApi`], parameterized by policy.
///
/// - **Passthrough** (co-located): every read delegates straight to the authority server,
///   retaining nothing — no redundant storage, behavior-preserving.
/// - **Retaining** (remote): point reads are served from a coherent summary
///   cache of the messages that flowed back (from point reads *and* query
///   pages), and read through on a miss. The cache is kept correct by the
///   down-channel ([`run_authority_server_down_channel`]): a message's entry is evicted when
///   the authority server says it changed, so it is never stale — the next read
///   re-fetches. (Mail-list pages are not cached; the authority computes
///   membership/order, so a list is always a fresh read-through that warms the
///   message cache.)
///
/// The summary cache is keyed by message id alone (as the outbox is): a
/// down-channel assertion carries the message id, so eviction is by id and may
/// over-evict across accounts — which is safe (a cache miss), only less
/// efficient. Carrying the account id on the assertion is a later refinement.
pub struct ReadCache {
    authority_server: Arc<dyn AuthorityServerApi>,
    summaries: Option<Mutex<HashMap<String, MessageSummary>>>,
}

impl ReadCache {
    /// The passthrough cache: read straight through, retain nothing.
    pub fn passthrough(authority_server: Arc<dyn AuthorityServerApi>) -> Self {
        Self {
            authority_server,
            summaries: None,
        }
    }

    /// The retaining cache: hold the summaries that flow back, kept coherent by
    /// the down-channel.
    pub fn retaining(authority_server: Arc<dyn AuthorityServerApi>) -> Self {
        Self {
            authority_server,
            summaries: Some(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        let page = self.authority_server.query_mail_page(request).await?;
        // A list read-through warms the message cache with what it returned.
        if let (Some(summaries), MailQueryPage::Messages(messages)) = (&self.summaries, &page) {
            let mut cache = summaries.lock().expect("summary cache poisoned");
            for message in &messages.items {
                cache.insert(message.id.as_str().to_string(), message.clone());
            }
        }
        Ok(page)
    }

    /// Read a message's detail through the authority server (passthrough; the detail view
    /// is recomputed per open, so it is not cached here).
    pub(crate) async fn message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        self.authority_server
            .message_detail(account_id.clone(), message_id.clone())
            .await
    }

    /// Read a conversation through the authority server (passthrough).
    pub(crate) async fn conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<ConversationView, RuntimeError> {
        self.authority_server.conversation(conversation_id.clone()).await
    }

    /// Read the authority server's live account count (passthrough; status metadata).
    pub(crate) async fn account_count(&self) -> Result<Option<usize>, RuntimeError> {
        self.authority_server.account_count().await
    }

    /// Read channel: the per-account undo/redo `rev_log` + cursor (passthrough).
    /// Serves the `RevLog` synced view. @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
    pub(crate) async fn rev_log_snapshot(
        &self,
        account_id: &AccountId,
    ) -> Result<RevLogSnapshot, RuntimeError> {
        self.authority_server.rev_log_snapshot(account_id.clone()).await
    }

    /// Account/config reads through the authority server (passthrough; not cached — these
    /// are config metadata, re-read on demand).
    pub(crate) async fn list_accounts(&self) -> Result<RuntimeAccountList, RuntimeError> {
        self.authority_server.list_accounts().await
    }

    pub(crate) async fn get_account(
        &self,
        account_id: AccountId,
    ) -> Result<Option<AccountOverview>, RuntimeError> {
        self.authority_server.get_account(account_id).await
    }

    pub(crate) async fn app_settings(&self) -> Result<AppSettings, RuntimeError> {
        self.authority_server.app_settings().await
    }

    pub(crate) async fn resolve_account_scope(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<Vec<AccountId>, RuntimeError> {
        self.authority_server.resolve_account_scope(scope).await
    }

    pub(crate) async fn list_mailboxes(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<BTreeMap<AccountId, Vec<MailboxSummary>>, RuntimeError> {
        self.authority_server.list_mailboxes(scope).await
    }

    pub(crate) async fn list_smart_mailboxes(
        &self,
    ) -> Result<Vec<SmartMailboxSummary>, RuntimeError> {
        self.authority_server.list_smart_mailboxes().await
    }

    pub(crate) async fn get_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<SmartMailbox, RuntimeError> {
        self.authority_server.get_smart_mailbox(smart_mailbox_id).await
    }

    pub(crate) async fn list_tags(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<Vec<TagSummary>, RuntimeError> {
        self.authority_server.list_tags(scope).await
    }

    /// Provider/account reads through the authority server (passthrough; not cached —
    /// these resolve a live gateway or read config/outbox state on demand).
    pub(crate) async fn get_identity(
        &self,
        account_id: AccountId,
    ) -> Result<Identity, RuntimeError> {
        self.authority_server.get_identity(account_id).await
    }

    pub(crate) async fn get_reply_context(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<ReplyContext, RuntimeError> {
        self.authority_server.get_reply_context(account_id, message_id).await
    }

    pub(crate) async fn list_sender_addresses(
        &self,
    ) -> Result<Vec<CachedSenderAddress>, RuntimeError> {
        self.authority_server.list_sender_addresses().await
    }

    pub(crate) async fn list_pending_operations(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<Operation>, RuntimeError> {
        self.authority_server.list_pending_operations(account_id).await
    }

    pub(crate) async fn replay_events(
        &self,
        filter: EventFilter,
    ) -> Result<Vec<DomainEvent>, RuntimeError> {
        self.authority_server.replay_events(filter).await
    }

    pub(crate) async fn get_draft_content(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<DraftContent, RuntimeError> {
        self.authority_server.get_draft_content(account_id, message_id).await
    }

    pub(crate) async fn get_message_resource(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        kind: MessageResourceKind,
    ) -> Result<RuntimeResourceBytes, RuntimeError> {
        self.authority_server
            .get_message_resource(account_id, message_id, kind)
            .await
    }

    // Summary-tier read fast-path: caches `MessageSummary`s and is invalidated by
    // `evict`/`apply_coherence_frame`. The production caller is not wired yet
    // (reads currently go through `message_detail`), so this is exercised only by
    // the unit tests below. Kept rather than removed because deleting it would
    // leave the sibling `evict` + `summaries` cache half-wired.
    #[allow(dead_code)]
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
            .authority_server
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
    pub(crate) fn apply_coherence_frame(&self, frame: &AuthorityServerFrame) {
        if let AuthorityServerFrame::Base { assertions } = frame {
            for assertion in assertions {
                self.evict(&assertion.message_id);
            }
        }
    }
}

/// Consume the authority server's down-channel and drive the near node from it: evict the
/// read cache (so the next read re-fetches), retire any outbox op the asserted
/// base now absorbs (the absorption-gated retire — a confirmed op held since its
/// receipt outran this assertion), AND republish each base assertion as a domain
/// event on the runtime's event bus, so the existing view machinery
/// (`ViewRegistry`'s event pump + `subscribe_events`) recomputes and pushes
/// frames to clients. This is the runtime↔authority-server half of the recursive
/// down-channel; the runtime→client half is the shipped view-frame protocol it
/// feeds ([replication authority-server-link L3](../replication/authority-server-link/L3.md)). In-process the runtime
/// shares the authority server's event bus directly, so this is spawned only for a split
/// (remote) runtime; it returns when the stream closes.
pub(crate) async fn run_authority_server_down_channel(
    link: AuthorityServerLinkHandle,
    reads: Arc<ReadCache>,
    events: broadcast::Sender<DomainEvent>,
    outbox: Arc<crate::near_node::RuntimeAuthorityServerOutbox>,
) {
    // Initial subscribe is fresh (`after_seq = None`); the reconnect engine
    // (M9b) will resume from `last_down_seq` below. Coverage says WHAT to stream,
    // the seq says WHERE to resume (D46).
    let Ok(mut stream) = link.subscribe(LinkCoverage::Complete, None).await else {
        return;
    };
    // A monotonic local sequence for the synthesized events. These do NOT match
    // the authority server's authoritative seqs (those come via `replay_events` over the
    // link); they only order the live stream for fresh subscribers.
    let mut seq: i64 = 0;
    // The last down-channel seq observed — the resume cursor a reconnect would
    // pass as `after_seq` (D46). Tracked here; the reconnect loop lands at M9b.
    let mut last_down_seq: u64 = 0;
    while let Some(sequenced) = stream.next().await {
        last_down_seq = sequenced.seq;
        let frame = &sequenced.frame;
        reads.apply_coherence_frame(frame);
        if let AuthorityServerFrame::Base { assertions } = frame {
            for assertion in assertions {
                // The authoritative base now carries the asserted state, so a
                // confirmed outbox op it absorbs is retired here — NOT on the
                // mutation's receipt, which can outrun this assertion when the
                // authority server is remote.
                outbox.apply_base(&assertion.message_id, &assertion.update);
                seq += 1;
                // A send error means there are no live subscribers yet; the next
                // read still re-fetches (the cache was evicted above).
                let _ = events.send(down_assertion_to_event(assertion, seq));
            }
        }
    }
    // The stream closed. `last_down_seq` is the resume cursor a reconnect passes
    // as `after_seq`; the reconnect loop that consumes it lands at M9b.
    tracing::debug!(
        last_down_seq,
        "authority-server down-channel closed; resume cursor recorded"
    );
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
    use posthaste_domain_model::MessagePage;
    use posthaste_authority_server_link::{
        AuthorityServerLink, BaseAssertion, BaseUpdate, DownStream, SequencedFrame,
    };
    use posthaste_link_core::MessageFoldState;
    use posthaste_contract_core::{MutationReceipt, MutationRequest};
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

    /// An authority server that counts how often each read reaches it. Reads
    /// only, so it implements just the Api half (the cache holds no Link half).
    struct CountingAuthorityServerLink {
        summary_calls: AtomicU64,
        query_calls: AtomicU64,
        page: MailQueryPage,
    }

    impl CountingAuthorityServerLink {
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
    impl AuthorityServerApi for CountingAuthorityServerLink {
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

    fn base_frame(message_id: &str) -> AuthorityServerFrame {
        AuthorityServerFrame::Base {
            assertions: vec![BaseAssertion {
                account_id: "acct".to_string(),
                message_id: message_id.to_string(),
                update: BaseUpdate::Present(MessageFoldState::default()),
            }],
        }
    }

    #[tokio::test]
    async fn passthrough_never_retains() {
        let authority_server = Arc::new(CountingAuthorityServerLink::new(vec![]));
        let cache = ReadCache::passthrough(authority_server.clone());
        let account = AccountId("acct".into());
        let message = MessageId("m1".into());
        cache.current_summary(&account, &message).await.unwrap();
        cache.current_summary(&account, &message).await.unwrap();
        assert_eq!(authority_server.summary_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn retaining_serves_a_point_read_from_cache_after_the_first() {
        let authority_server = Arc::new(CountingAuthorityServerLink::new(vec![]));
        let cache = ReadCache::retaining(authority_server.clone());
        let account = AccountId("acct".into());
        let message = MessageId("m1".into());
        cache.current_summary(&account, &message).await.unwrap();
        cache.current_summary(&account, &message).await.unwrap();
        // Second read is a cache hit.
        assert_eq!(authority_server.summary_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_query_warms_the_message_cache() {
        let authority_server = Arc::new(CountingAuthorityServerLink::new(vec![summary("m1")]));
        let cache = ReadCache::retaining(authority_server.clone());
        let request: MailQueryRequest = serde_json::from_value(json!({
            "query": "in:acct/inbox",
            "presentation": {
                "kind": "messages", "limit": 10, "cursor": null,
                "sortField": "date", "sortDirection": "desc"
            }
        }))
        .unwrap();
        cache.query_mail_page(request).await.unwrap();
        // The point read is now served from the warmed cache, never reaching the authority server.
        cache
            .current_summary(&AccountId("acct".into()), &MessageId("m1".into()))
            .await
            .unwrap();
        assert_eq!(authority_server.summary_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_coherence_frame_evicts_so_the_next_read_refetches() {
        let authority_server = Arc::new(CountingAuthorityServerLink::new(vec![]));
        let cache = ReadCache::retaining(authority_server.clone());
        let account = AccountId("acct".into());
        let message = MessageId("m1".into());
        cache.current_summary(&account, &message).await.unwrap();
        // The authority server asserts m1 changed; the cached summary is evicted.
        cache.apply_coherence_frame(&base_frame("m1"));
        cache.current_summary(&account, &message).await.unwrap();
        assert_eq!(authority_server.summary_calls.load(Ordering::SeqCst), 2);
    }

    // An authority server whose down-channel emits one Base assertion then closes, with a
    // counting point read so eviction is observable. It is consumed both as a
    // ReadCache source (Api half) and via the handle's down-channel (Link
    // half), so it implements the pair — the shape every real transport has.
    struct BridgeAuthorityServerLink {
        summary_calls: AtomicU64,
    }

    #[async_trait]
    impl AuthorityServerLink for BridgeAuthorityServerLink {
        async fn forward_mutation(
            &self,
            _mutation: MutationRequest,
        ) -> Result<MutationReceipt, RuntimeError> {
            Err(RuntimeError::internal("bridge authority-server link is read-only", None))
        }

        async fn subscribe(
            &self,
            _coverage: LinkCoverage,
            _after_seq: Option<u64>,
        ) -> Result<DownStream, RuntimeError> {
            Ok(Box::pin(futures_util::stream::iter([SequencedFrame::new(
                1,
                AuthorityServerFrame::Base {
                    assertions: vec![BaseAssertion {
                        account_id: "acct".into(),
                        message_id: "m1".into(),
                        // Carries the flag so a pending flag op on m1 is absorbed.
                        update: BaseUpdate::Present(MessageFoldState {
                            keywords: vec!["$flagged".into()],
                            mailbox_ids: vec![],
                        }),
                    }],
                },
            )])))
        }
    }

    #[async_trait]
    impl AuthorityServerApi for BridgeAuthorityServerLink {
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
        let authority_server = Arc::new(BridgeAuthorityServerLink {
            summary_calls: AtomicU64::new(0),
        });
        let cache = Arc::new(ReadCache::retaining(authority_server.clone()));
        let account = AccountId("acct".into());
        let message = MessageId("m1".into());

        // Warm the cache (one fetch), then run the bridge over a one-frame link.
        cache.current_summary(&account, &message).await.unwrap();
        assert_eq!(authority_server.summary_calls.load(Ordering::SeqCst), 1);

        // A confirmed flag op on m1, held pending because its receipt outran the
        // base assertion (the remote seam). The bridge's assertion below carries
        // the flag, so the absorption-gated retire drops it.
        let outbox = Arc::new(crate::near_node::RuntimeAuthorityServerOutbox::new(true));
        outbox.accept(posthaste_link_core::PendingMessageMutation {
            id: posthaste_link_core::MutationId("op1".into()),
            key: "m1".into(),
            effect: posthaste_link_core::MessageAssertion::SetKeywords {
                add: vec!["$flagged".into()],
                remove: vec![],
            },
        });
        outbox.settle_receipt(&posthaste_link_core::MutationId("op1".into()), true);
        assert_eq!(
            outbox.snapshot().len(),
            1,
            "op held until the base absorbs it"
        );

        let (events, mut rx) = broadcast::channel(16);
        let link = AuthorityServerLinkHandle::new(authority_server.clone());
        // The stub stream closes after one frame, so the bridge returns.
        run_authority_server_down_channel(link, cache.clone(), events, outbox.clone()).await;

        // The base now carries the flag: the held op is retired by absorption.
        assert!(
            outbox.snapshot().is_empty(),
            "absorbed op retired by the bridge"
        );

        // It republished the assertion as a `message.updated` domain event
        // scoped to the right account + message, flagged so views recompute.
        let event = rx.try_recv().expect("an event was republished");
        assert_eq!(event.topic, EVENT_TOPIC_MESSAGE_UPDATED);
        assert_eq!(event.account_id.as_str(), "acct");
        assert_eq!(event.message_id.as_ref().map(MessageId::as_str), Some("m1"));
        assert_eq!(event.payload["changes"]["keywords"], true);

        // And it evicted the cache: the next read re-fetches.
        cache.current_summary(&account, &message).await.unwrap();
        assert_eq!(authority_server.summary_calls.load(Ordering::SeqCst), 2);
    }
}
