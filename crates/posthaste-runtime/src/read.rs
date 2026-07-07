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

use std::collections::BTreeMap;

use async_trait::async_trait;
use posthaste_authority_server_link::{
    AuthorityServerApi, AuthorityServerFrame, BaseAssertion, BaseUpdate,
};
use posthaste_contract_core::{
    AccountScopeRequest, MailQueryPage, MailQueryRequest, MessageResourceKind, RuntimeAccountList,
    RuntimeError, RuntimeResourceBytes,
};
use posthaste_domain_model::{
    now_iso8601, AccountId, AccountOverview, AppSettings, CachedSenderAddress, ConversationId,
    ConversationView, DomainEvent, DraftContent, EventFilter, EventLogBounds, Identity,
    MailboxSummary, MessageDetail, MessageId, MessageSummary, Operation, ReplyContext,
    RevLogSnapshot, SmartMailbox, SmartMailboxId, SmartMailboxSummary, TagSummary,
    EVENT_TOPIC_MESSAGE_UPDATED,
};
use posthaste_link_far_end::down::{FactLog, FactLogError, Sequenced};
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
/// The summary cache is keyed by message id alone (as the pending set is): a
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
        self.authority_server
            .conversation(conversation_id.clone())
            .await
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
        self.authority_server
            .rev_log_snapshot(account_id.clone())
            .await
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
        self.authority_server
            .get_smart_mailbox(smart_mailbox_id)
            .await
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
        self.authority_server
            .get_reply_context(account_id, message_id)
            .await
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
        self.authority_server
            .list_pending_operations(account_id)
            .await
    }

    pub(crate) async fn replay_events(
        &self,
        filter: EventFilter,
    ) -> Result<Vec<DomainEvent>, RuntimeError> {
        self.authority_server.replay_events(filter).await
    }

    /// The cheap `event_log` seq bounds for the fact-carrying tap's
    /// head/truncation queries (RFC-L2-scripting S2). Passthrough; `None` when the
    /// log is empty. Errors (e.g. a transport without the read channel) are
    /// surfaced so [`EventLogFactLog`] can fall back to a replay scan.
    pub(crate) async fn event_log_bounds(&self) -> Result<Option<EventLogBounds>, RuntimeError> {
        self.authority_server.event_log_bounds().await
    }

    pub(crate) async fn get_draft_content(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<DraftContent, RuntimeError> {
        self.authority_server
            .get_draft_content(account_id, message_id)
            .await
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

    /// Evict the entire summary cache — the D49 reset reaction: a down-channel
    /// gap the far-end could not replay means the near node's coverage is no
    /// longer trustworthy, so drop it all and re-read through on the next read.
    /// No-op for a passthrough cache.
    pub(crate) fn evict_all(&self) {
        if let Some(summaries) = &self.summaries {
            summaries.lock().expect("summary cache poisoned").clear();
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
/// read cache (so the next read re-fetches), retire any pending-set op the asserted
/// base now absorbs (the absorption-gated retire — a confirmed op held since its
/// receipt outran this assertion), AND republish each base assertion as a domain
/// event on the runtime's event bus, so the existing view machinery
/// (`ViewRegistry`'s event pump + `subscribe_events`) recomputes and pushes
/// frames to clients. This is the runtime↔authority-server half of the recursive
/// down-channel; the runtime→client half is the shipped view-frame protocol it
/// feeds ([replication authority-server-link L3](../replication/authority-server-link/L3.md)). In-process the runtime
/// shares the authority server's event bus directly, so this is spawned only for a split
/// (remote) runtime.
///
/// The connection lifecycle lives in the shared `LinkNearEnd` engine (M9b2,
/// D40): `frames` is the engine's down-channel — the engine subscribes,
/// reconnects with jittered backoff, and resumes from its own `last_down_seq`
/// cursor (`afterSeq`, D46). This consumer holds ONLY the near node's frame
/// semantics (eviction/absorption/republish — unchanged); it returns when the
/// engine side is dropped.
pub(crate) async fn run_authority_server_down_channel(
    mut frames: tokio::sync::mpsc::UnboundedReceiver<
        posthaste_authority_server_link::SequencedFrame,
    >,
    reads: Arc<ReadCache>,
    events: broadcast::Sender<DomainEvent>,
    pending_set: Arc<crate::near_node::AuthorityServerPendingSet>,
) {
    // A monotonic local sequence for the synthesized events. These do NOT match
    // the authority server's authoritative seqs (those come via `replay_events` over the
    // link); they only order the live stream for fresh subscribers.
    let mut seq: i64 = 0;
    while let Some(sequenced) = frames.recv().await {
        let Some(frame) = sequenced.frame() else {
            // A `Reset` control element (D49): the near node's incremental base
            // view is broken — evict the whole read cache and re-read through on
            // the next read.
            reads.evict_all();
            continue;
        };
        reads.apply_coherence_frame(frame);
        if let AuthorityServerFrame::Base { assertions } = frame {
            for assertion in assertions {
                // The authoritative base now carries the asserted state, so a
                // confirmed pending-set op it absorbs is retired here — NOT on the
                // mutation's receipt, which can outrun this assertion when the
                // authority server is remote.
                pending_set.apply_base(&assertion.message_id, &assertion.update);
                seq += 1;
                // A send error means there are no live subscribers yet; the next
                // read still re-fetches (the cache was evicted above).
                let _ = events.send(down_assertion_to_event(assertion, seq));
            }
        }
    }
    tracing::debug!("authority-server down-channel consumer stopped (engine side dropped)");
}

/// Map a down-channel base assertion to the domain event the near node's view
/// machinery already understands: a `message.updated` over the asserted message
/// (or a deletion).
///
/// The assertion normally carries its authoritative source event whole
/// (`BaseAssertion::event`, attached where the far node derives the assertion
/// from the event) — republish THAT, restamped with the local live-stream seq,
/// so the split runtime's clients receive the SAME enriched payload
/// (`payload.projection`, the body-free `MessageSummary` —
/// `posthaste-store` `mutations/commands.rs`) the co-located bus delivers, and
/// their entity store can self-maintain mail-list ROWS on their own mutations
/// instead of waiting for a re-serve. Counts no longer ride any event
/// (RFC-L2-count-unification): a client reacts to this republished event by
/// invalidating its mailbox-count query and refetching over the link. The
/// local seq only orders the live stream for fresh subscribers; authoritative
/// seqs come via `replay_events` over the link (as before).
///
/// Without a carried event (an older far node), fall back to synthesizing a
/// bare event: broad change flags — the view layer re-reads through the cache
/// and suppresses no-op recomputes — and the account id (carried on the
/// assertion) scoping per-account views like `messageDetail`. The bare shape
/// still triggers the client's count invalidation; only the projection-fed row
/// fold waits for the next re-serve.
fn down_assertion_to_event(assertion: &BaseAssertion, seq: i64) -> DomainEvent {
    if let Some(event) = &assertion.event {
        let mut event = event.clone();
        event.seq = seq;
        return event;
    }
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

/// The runtime's [`FactLog`] binding (RFC-L2-scripting D52 / S1): the
/// fact-carrying tap's durable replay backed by the authority server's
/// authoritative `event_log`, reached through this runtime's read-through
/// [`ReadCache`]. Facts are [`DomainEvent`]s; the seam filter is [`EventFilter`]
/// (topic / account / mailbox scope, composed with the resume cursor).
///
/// **Read-only** (D52 — the tap is a read-only far-end): events are authored by
/// the authority server's write path (a mutation or a sync commits them into the
/// `event_log`, seq AUTOINCREMENT); the runtime tap replays and tails them but
/// never appends. The authority server's own `FactLog` binding — append included,
/// over its store — is S3's. This is the machinery S2 mounts on `/v1/events`
/// (via the runtime's [`crate::handle`] `subscribe_events` → [`Tap`]).
pub(crate) struct EventLogFactLog {
    reads: Arc<ReadCache>,
}

impl EventLogFactLog {
    pub(crate) fn new(reads: Arc<ReadCache>) -> Self {
        Self { reads }
    }

    /// The whole-log filter resuming after `after` (no seam narrowing).
    fn filter_after(after: u64) -> EventFilter {
        EventFilter {
            account_id: None,
            topic: None,
            mailbox_id: None,
            after_seq: Some(after as i64),
        }
    }
}

#[async_trait]
impl FactLog for EventLogFactLog {
    type Fact = DomainEvent;
    type Filter = EventFilter;

    async fn append(&self, _fact: DomainEvent) -> Result<u64, FactLogError> {
        // The runtime tap is a read-only view of the authority-authored log
        // (D52). Appends belong to the authority server's write path (S3).
        Err(FactLogError::ReadOnly)
    }

    async fn replay(
        &self,
        after_seq: u64,
        filter: Option<EventFilter>,
    ) -> Result<Vec<Sequenced<DomainEvent>>, FactLogError> {
        // The subscriber's seam filter (topic/account/mailbox scope) composed
        // with the resume cursor: the cursor always occupies the `after_seq` slot.
        let mut filter = filter.unwrap_or_else(|| Self::filter_after(after_seq));
        filter.after_seq = Some(after_seq as i64);
        let events = self
            .reads
            .replay_events(filter)
            .await
            .map_err(|error| FactLogError::Backing(error.to_string()))?;
        Ok(events
            .into_iter()
            .map(|event| Sequenced::new(event.seq.max(0) as u64, event))
            .collect())
    }

    async fn highest_seq(&self) -> Result<u64, FactLogError> {
        // The live head: the newest assigned seq. Served by the cheap
        // `MAX(seq)` bounds query (S2), not a full replay scan; on a transport
        // that does not carry the bounds query the scan is the fallback.
        match self.reads.event_log_bounds().await {
            Ok(bounds) => Ok(bounds.map(|b| b.newest.max(0) as u64).unwrap_or(0)),
            Err(_) => Ok(self.scan_events().await?.last().map(seq_of).unwrap_or(0)),
        }
    }

    async fn truncation_point(&self) -> Result<u64, FactLogError> {
        // The oldest retained seq — the gap-frame threshold (a resume from before
        // it cannot be served from durable history). Served by the cheap
        // `MIN(seq)` bounds query (S2); the `event_log` is append-only and not yet
        // truncated, so today this is the first event's seq (or 0 when empty).
        match self.reads.event_log_bounds().await {
            Ok(bounds) => Ok(bounds.map(|b| b.oldest.max(0) as u64).unwrap_or(0)),
            Err(_) => Ok(self.scan_events().await?.first().map(seq_of).unwrap_or(0)),
        }
    }
}

/// The whole-log scan fallback for [`EventLogFactLog::highest_seq`]/
/// [`truncation_point`] when the transport does not carry the cheap bounds query.
impl EventLogFactLog {
    async fn scan_events(&self) -> Result<Vec<DomainEvent>, FactLogError> {
        self.reads
            .replay_events(Self::filter_after(0))
            .await
            .map_err(|error| FactLogError::Backing(error.to_string()))
    }
}

/// The non-negative seq of an event as a `u64` (seqs are AUTOINCREMENT ≥ 1).
fn seq_of(event: &DomainEvent) -> u64 {
    event.seq.max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    use async_trait::async_trait;
    use posthaste_authority_server_link::{BaseAssertion, BaseUpdate, SequencedFrame};
    use posthaste_domain_model::MessagePage;
    use posthaste_replica_core::MessageFoldState;
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
                event: None,
            }],
        }
    }

    /// The ENRICHED `message.updated` the store command authors in-tx
    /// (`posthaste-store` `mutations/commands.rs`) and the far node attaches to
    /// its base assertion: `payload.projection` (the body-free summary) — the
    /// field the client's `storeUpdatesFromEvent` reads to self-maintain the
    /// mail-list ROWS. No counts ride the event (RFC-L2-count-unification).
    fn enriched_event(seq: i64, payload: serde_json::Value) -> DomainEvent {
        DomainEvent {
            seq,
            account_id: AccountId("acct".into()),
            topic: EVENT_TOPIC_MESSAGE_UPDATED.to_string(),
            occurred_at: "2026-06-24T00:00:00Z".into(),
            mailbox_id: Some(posthaste_domain_model::MailboxId("inbox".into())),
            message_id: Some(MessageId("m1".into())),
            payload,
        }
    }

    /// A mark-read echo (`setKeywords` add `$seen`): the projection flips
    /// `isRead`. Counts ride no event (RFC-L2-count-unification).
    fn mark_read_event(seq: i64) -> DomainEvent {
        let mut projection = serde_json::to_value(summary("m1")).unwrap();
        projection["isRead"] = json!(true);
        projection["keywords"] = json!(["$seen"]);
        enriched_event(
            seq,
            json!({
                "messageId": "m1",
                "changes": { "keywords": true },
                "keywords": ["$seen"],
                "projection": projection,
            }),
        )
    }

    /// A move echo (`replaceMailboxes` inbox → archive): the projection carries
    /// the new membership.
    fn move_event(seq: i64) -> DomainEvent {
        let mut projection = serde_json::to_value(summary("m1")).unwrap();
        projection["mailboxIds"] = json!(["archive"]);
        enriched_event(
            seq,
            json!({
                "messageId": "m1",
                "changes": { "mailboxes": true, "arrived": true },
                "mailboxIds": ["archive"],
                "arrivedMailboxIds": ["archive"],
                "projection": projection,
            }),
        )
    }

    // The split republish forwards the carried enriched event — same topic and
    // scope, `projection` intact — restamped with the local live-stream seq.
    // This is the split-mode twin of the bundled enriched echo
    // (`queue_then_emit_message_operation`): the carried projection keeps the
    // split client's mail-list ROWS self-maintaining, and the republished event
    // is the trigger the client's count invalidation fires on (the counts
    // themselves are refetched over the link, not carried).
    #[test]
    fn a_mark_read_assertion_republishes_the_enriched_event_with_the_local_seq() {
        let assertion = BaseAssertion {
            account_id: "acct".into(),
            message_id: "m1".into(),
            update: BaseUpdate::Present(MessageFoldState {
                keywords: vec!["$seen".into()],
                mailbox_ids: vec!["inbox".into()],
            }),
            event: Some(mark_read_event(41)),
        };
        let event = down_assertion_to_event(&assertion, 7);
        assert_eq!(event.topic, EVENT_TOPIC_MESSAGE_UPDATED);
        assert_eq!(event.seq, 7, "restamped with the local live-stream seq");
        assert_eq!(event.account_id.as_str(), "acct");
        assert_eq!(event.message_id.as_ref().map(MessageId::as_str), Some("m1"));
        // The row's food: the projection reflects the mark-read.
        assert_eq!(event.payload["projection"]["isRead"], true);
        assert_eq!(event.payload["changes"]["keywords"], true);
        // No counts on the wire — the client invalidates + refetches instead.
        assert!(event.payload.get("countDeltas").is_none());
    }

    #[test]
    fn a_move_assertion_republishes_the_post_move_projection() {
        let assertion = BaseAssertion {
            account_id: "acct".into(),
            message_id: "m1".into(),
            update: BaseUpdate::Present(MessageFoldState {
                keywords: vec![],
                mailbox_ids: vec!["archive".into()],
            }),
            event: Some(move_event(42)),
        };
        let event = down_assertion_to_event(&assertion, 9);
        assert_eq!(event.seq, 9);
        assert_eq!(event.payload["changes"]["mailboxes"], true);
        // The projection carries the post-move membership (row liveness).
        assert_eq!(
            event.payload["projection"]["mailboxIds"],
            json!(["archive"])
        );
        assert!(event.payload.get("countDeltas").is_none());
    }

    // An assertion without a carried event (an older far node / synthetic frame)
    // still republishes — the bare synthesized shape, as before the enrichment.
    #[test]
    fn an_assertion_without_a_carried_event_falls_back_to_the_bare_shape() {
        let assertion = BaseAssertion {
            account_id: "acct".into(),
            message_id: "m1".into(),
            update: BaseUpdate::Present(MessageFoldState::default()),
            event: None,
        };
        let event = down_assertion_to_event(&assertion, 3);
        assert_eq!(event.topic, EVENT_TOPIC_MESSAGE_UPDATED);
        assert_eq!(event.payload["changes"]["keywords"], true);
        assert_eq!(event.payload["changes"]["mailboxes"], true);

        let removed = BaseAssertion {
            account_id: "acct".into(),
            message_id: "m1".into(),
            update: BaseUpdate::Removed,
            event: None,
        };
        let event = down_assertion_to_event(&removed, 4);
        assert_eq!(event.payload["deleted"], true);
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

    // A counting point read so eviction is observable through the ReadCache's
    // Api half; the down-channel frames now arrive over the engine's channel
    // (the engine owns subscribe/reconnect — see `link_near_end`), so the test
    // feeds the consumer directly.
    struct BridgeAuthorityServerLink {
        summary_calls: AtomicU64,
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
        // base assertion (the remote seam). The assertion fed below carries
        // the flag, so the absorption-gated retire drops it.
        let pending_set = Arc::new(crate::near_node::AuthorityServerPendingSet::new(true));
        pending_set.accept(posthaste_replica_core::PendingMessageMutation {
            id: posthaste_replica_core::MutationId("op1".into()),
            key: "m1".into(),
            effect: posthaste_replica_core::MessageAssertion::SetKeywords {
                add: vec!["$flagged".into()],
                remove: vec![],
            },
        });
        pending_set.settle_receipt(&posthaste_replica_core::MutationId("op1".into()), true);
        assert_eq!(
            pending_set.snapshot().len(),
            1,
            "op held until the base absorbs it"
        );

        let (events, mut rx) = broadcast::channel(16);
        // One engine-delivered frame, then the engine side drops — the consumer
        // applies the frame's semantics and returns.
        let (frames_tx, frames_rx) = tokio::sync::mpsc::unbounded_channel();
        frames_tx
            .send(SequencedFrame::new(
                1,
                AuthorityServerFrame::Base {
                    assertions: vec![BaseAssertion {
                        account_id: "acct".into(),
                        message_id: "m1".into(),
                        // Carries the flag so the pending flag op on m1 is absorbed.
                        update: BaseUpdate::Present(MessageFoldState {
                            keywords: vec!["$flagged".into(), "$seen".into()],
                            mailbox_ids: vec![],
                        }),
                        // The far node attaches the store command's enriched
                        // source event; the bridge republishes it verbatim.
                        event: Some(mark_read_event(41)),
                    }],
                },
            ))
            .expect("frame enqueues");
        drop(frames_tx);
        run_authority_server_down_channel(frames_rx, cache.clone(), events, pending_set.clone())
            .await;

        // The base now carries the flag: the held op is retired by absorption.
        assert!(
            pending_set.snapshot().is_empty(),
            "absorbed op retired by the bridge"
        );

        // It republished the assertion's carried ENRICHED `message.updated`
        // domain event — scoped to the right account + message, with the
        // `projection` the client's row fold consumes (the same shape as the
        // bundled echo), restamped with the local live-stream seq. No counts on
        // the wire: this republished event is the count-invalidation trigger.
        let event = rx.try_recv().expect("an event was republished");
        assert_eq!(event.topic, EVENT_TOPIC_MESSAGE_UPDATED);
        assert_eq!(event.account_id.as_str(), "acct");
        assert_eq!(event.message_id.as_ref().map(MessageId::as_str), Some("m1"));
        assert_eq!(
            event.seq, 1,
            "local live-stream seq, not the authority's 41"
        );
        assert_eq!(event.payload["changes"]["keywords"], true);
        assert_eq!(event.payload["projection"]["isRead"], true);
        assert!(event.payload.get("countDeltas").is_none());

        // And it evicted the cache: the next read re-fetches.
        cache.current_summary(&account, &message).await.unwrap();
        assert_eq!(authority_server.summary_calls.load(Ordering::SeqCst), 2);
    }

    /// An authority server whose only surface is the authoritative event log —
    /// the backing the runtime `FactLog` replays through the `ReadCache`.
    struct EventLogStub {
        events: Vec<DomainEvent>,
    }

    #[async_trait]
    impl AuthorityServerApi for EventLogStub {
        async fn replay_events(
            &self,
            filter: EventFilter,
        ) -> Result<Vec<DomainEvent>, RuntimeError> {
            let after = filter.after_seq.unwrap_or(0);
            Ok(self
                .events
                .iter()
                .filter(|event| event.seq > after)
                .cloned()
                .collect())
        }
    }

    fn event(seq: i64) -> DomainEvent {
        DomainEvent {
            seq,
            account_id: AccountId("acct".into()),
            topic: EVENT_TOPIC_MESSAGE_UPDATED.to_string(),
            occurred_at: "2026-07-03T00:00:00Z".into(),
            mailbox_id: None,
            message_id: None,
            payload: json!({}),
        }
    }

    // D52: the runtime FactLog replays durable facts after the cursor, and
    // reports the head + truncation point the tap resolves the gap frame against.
    #[tokio::test]
    async fn fact_log_replays_events_after_the_cursor() {
        let reads = Arc::new(ReadCache::passthrough(Arc::new(EventLogStub {
            events: vec![event(1), event(2), event(3)],
        })));
        let log = EventLogFactLog::new(reads);
        assert_eq!(
            log.highest_seq().await.unwrap(),
            3,
            "head is the newest seq"
        );
        assert_eq!(
            log.truncation_point().await.unwrap(),
            1,
            "oldest retained seq"
        );
        let frames = log.replay(1, None).await.unwrap();
        assert_eq!(
            frames.iter().map(|f| f.seq()).collect::<Vec<_>>(),
            vec![2, 3],
            "replays facts after the cursor, seq-stamped"
        );
    }

    // D52: the runtime binding is read-only — the tap tails the authority-authored
    // log, appends are the authority server's write path (S3).
    #[tokio::test]
    async fn fact_log_is_read_only_on_the_runtime_side() {
        let reads = Arc::new(ReadCache::passthrough(Arc::new(EventLogStub {
            events: vec![],
        })));
        let log = EventLogFactLog::new(reads);
        assert!(matches!(
            log.append(event(1)).await,
            Err(FactLogError::ReadOnly)
        ));
        assert_eq!(log.highest_seq().await.unwrap(), 0, "empty log head is 0");
    }

    /// An authority server that answers the cheap `event_log` bounds query and
    /// panics on `replay_events` — so a head/truncation read that touches it
    /// proves it took the cheap path, not the replay scan (S2).
    struct BoundsOnlyStub {
        bounds: Option<EventLogBounds>,
    }

    #[async_trait]
    impl AuthorityServerApi for BoundsOnlyStub {
        async fn replay_events(
            &self,
            _filter: EventFilter,
        ) -> Result<Vec<DomainEvent>, RuntimeError> {
            panic!("head/truncation must use the cheap bounds query, not a replay scan");
        }

        async fn event_log_bounds(&self) -> Result<Option<EventLogBounds>, RuntimeError> {
            Ok(self.bounds)
        }
    }

    // S2: the cheap head query — `highest_seq`/`truncation_point` read the
    // store-level `(MIN, MAX)` bounds, never scanning the whole log.
    #[tokio::test]
    async fn head_and_truncation_use_the_cheap_bounds_query() {
        let reads = Arc::new(ReadCache::passthrough(Arc::new(BoundsOnlyStub {
            bounds: Some(EventLogBounds {
                oldest: 5,
                newest: 9,
            }),
        })));
        let log = EventLogFactLog::new(reads);
        assert_eq!(log.highest_seq().await.unwrap(), 9, "MAX(seq) is the head");
        assert_eq!(
            log.truncation_point().await.unwrap(),
            5,
            "MIN(seq) is the oldest"
        );
    }

    #[tokio::test]
    async fn empty_bounds_report_zero() {
        let reads = Arc::new(ReadCache::passthrough(Arc::new(BoundsOnlyStub {
            bounds: None,
        })));
        let log = EventLogFactLog::new(reads);
        assert_eq!(log.highest_seq().await.unwrap(), 0);
        assert_eq!(log.truncation_point().await.unwrap(), 0);
    }

    // S2/D52: a Tap over the runtime FactLog opens the gap frame when the resume
    // cursor fell before the log's oldest retained seq (a truncated backlog),
    // carrying the live head as the re-attach cursor — never a silent drop (N8).
    #[tokio::test]
    async fn tap_over_the_runtime_fact_log_opens_the_gap_frame_past_truncation() {
        use posthaste_link_far_end::down::Tap;
        // Bounds say the oldest retained seq is 5 (seqs 1..=4 truncated).
        let reads = Arc::new(ReadCache::passthrough(Arc::new(BoundsOnlyStub {
            bounds: Some(EventLogBounds {
                oldest: 5,
                newest: 9,
            }),
        })));
        let tap: Tap<EventLogFactLog, &'static str> =
            Tap::new(Arc::new(EventLogFactLog::new(reads)));
        // A cursor at seq 1 wants seq 2 next, which is truncated → gap at head 9.
        let resume = tap.subscribe(&"s", Some(1), None, 0).await.unwrap();
        assert!(resume.is_gap(), "a cursor before truncation opens a gap");
        match resume {
            posthaste_link_far_end::down::TapResume::Gap { highest_seq } => {
                assert_eq!(
                    highest_seq, 9,
                    "the gap carries the live head as the re-attach cursor"
                );
            }
            other => panic!("expected a gap, got {other:?}"),
        }
    }
}
