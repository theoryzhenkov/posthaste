//! The runtime's read path as a read-through cache over a far node.
//!
//! Reads ([replication L4 W4](../replication/L4.md), `DESIGN-L4-read-replication`)
//! go through a [`ReadCache`] over a [`ReadSource`]: the query engine lives at
//! the authority (the far node), and a near node retains the data that flowed
//! back under a **policy** chosen from link cost. The primitive is read-through;
//! caching is the optimization.
//!
//! W4a is the seam only: `LocalReadSource` calls the in-process backend directly
//! and the policy is **passthrough** (retain nothing, always read through), so
//! the co-located deployment behaves exactly as before (`colocated-unchanged`).
//! The retaining policy and the remote source (over the link) are W4c.
//!
//! @spec docs/eph/DESIGN-L4-read-replication#6-co-located-is-the-same-code-collapsed

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures_util::StreamExt;
use posthaste_domain::{
    AccountId, ConversationId, ConversationView, MessageDetail, MessageId, MessageSummary,
};
use posthaste_link_contract::{BackendLink, DownFrame, LinkCoverage};
use posthaste_runtime_contract::{MailQueryPage, MailQueryRequest, RuntimeError, RuntimeErrorCode};

use crate::backend::Backend;

/// The far node's read surface — what a near node reads through to. Co-located
/// it is the in-process backend; split it is carried over the link (W4c).
#[async_trait]
pub(crate) trait ReadSource: Send + Sync {
    /// Compute a page of a mail-list query (the query engine lives here).
    async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError>;

    /// One message's current canonical summary (the point read behind
    /// undo-history). `None` when the message is not held.
    async fn current_summary(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageSummary>, RuntimeError>;

    /// A message's detail (header + attachments) for the `messageDetail` view.
    /// Defaults to unsupported so a source carries it only when wired.
    async fn message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        let _ = (account_id, message_id);
        Err(read_unsupported())
    }

    /// An overlay-folded conversation for the `conversation` view.
    async fn conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<ConversationView, RuntimeError> {
        let _ = conversation_id;
        Err(read_unsupported())
    }
}

fn read_unsupported() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorCode::Internal,
        "read source does not carry this read",
    )
}

/// The co-located read source: calls the in-process backend far node directly
/// (today's reads), zero serialization. The far node owns the query engine; this
/// is the read twin of `InProcessTransport` on the write path.
pub(crate) struct LocalReadSource {
    backend: Arc<Backend>,
}

impl LocalReadSource {
    pub(crate) fn new(backend: Arc<Backend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl ReadSource for LocalReadSource {
    async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        self.backend.query_mail_page(request).await
    }

    async fn current_summary(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageSummary>, RuntimeError> {
        self.backend.current_summary(account_id, message_id).await
    }

    async fn message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        self.backend.message_detail(account_id, message_id).await
    }

    async fn conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<ConversationView, RuntimeError> {
        self.backend.conversation(conversation_id)
    }
}

/// The split read source: reads through to the backend over the link. W4c uses
/// it with a passthrough policy (read-through on every read — always fresh, no
/// retention); a retaining policy waits on the down-channel coherence of W4d so
/// a cached entry is never stale.
pub(crate) struct RemoteReadSource {
    link: BackendLink,
}

impl RemoteReadSource {
    pub(crate) fn new(link: BackendLink) -> Self {
        Self { link }
    }
}

#[async_trait]
impl ReadSource for RemoteReadSource {
    async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        self.link.query_mail_page(request).await
    }

    async fn current_summary(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageSummary>, RuntimeError> {
        self.link
            .current_summary(account_id.clone(), message_id.clone())
            .await
    }
}

/// A read-through cache over a [`ReadSource`], parameterized by policy.
///
/// - **Passthrough** (co-located): every read delegates straight to the source,
///   retaining nothing — no redundant storage, behavior-preserving.
/// - **Retaining** (remote): point reads are served from a coherent summary
///   cache of the messages that flowed back (from point reads *and* query
///   pages), and read through on a miss. The cache is kept correct by the
///   down-channel ([`run_cache_coherence`]): a message's entry is evicted when
///   the backend says it changed, so it is never stale — the next read
///   re-fetches. (Mail-list pages are not cached; the authority computes
///   membership/order, so a list is always a fresh read-through that warms the
///   message cache.)
///
/// The summary cache is keyed by message id alone (as the outbox is): a
/// down-channel assertion carries the message id, so eviction is by id and may
/// over-evict across accounts — which is safe (a cache miss), only less
/// efficient. Carrying the account id on the assertion is a later refinement.
pub(crate) struct ReadCache {
    source: Arc<dyn ReadSource>,
    summaries: Option<Mutex<HashMap<String, MessageSummary>>>,
}

impl ReadCache {
    /// The passthrough cache: read straight through, retain nothing.
    pub(crate) fn passthrough(source: Arc<dyn ReadSource>) -> Self {
        Self {
            source,
            summaries: None,
        }
    }

    /// The retaining cache: hold the summaries that flow back, kept coherent by
    /// the down-channel.
    pub(crate) fn retaining(source: Arc<dyn ReadSource>) -> Self {
        Self {
            source,
            summaries: Some(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        let page = self.source.query_mail_page(request).await?;
        // A list read-through warms the message cache with what it returned.
        if let (Some(summaries), MailQueryPage::Messages(messages)) = (&self.summaries, &page) {
            let mut cache = summaries.lock().expect("summary cache poisoned");
            for message in &messages.items {
                cache.insert(message.id.as_str().to_string(), message.clone());
            }
        }
        Ok(page)
    }

    /// Read a message's detail through the source (passthrough; the detail view
    /// is recomputed per open, so it is not cached here).
    pub(crate) async fn message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        self.source.message_detail(account_id, message_id).await
    }

    /// Read a conversation through the source (passthrough).
    pub(crate) async fn conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<ConversationView, RuntimeError> {
        self.source.conversation(conversation_id).await
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
        let result = self.source.current_summary(account_id, message_id).await?;
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

/// Keep a retaining [`ReadCache`] coherent: consume the link's down-channel and
/// evict cached summaries as the backend asserts changes. Spawned for a split
/// runtime; returns when the stream closes.
pub(crate) async fn run_cache_coherence(link: BackendLink, reads: Arc<ReadCache>) {
    let Ok(mut stream) = link.subscribe(LinkCoverage::Complete).await else {
        return;
    };
    while let Some(frame) = stream.next().await {
        reads.apply_coherence_frame(&frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    use posthaste_domain::MessagePage;
    use posthaste_link_contract::{BaseAssertion, BaseUpdate};
    use posthaste_link_core::MessageFoldState;
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

    /// A read source that counts how often each read reaches it.
    struct CountingSource {
        summary_calls: AtomicU64,
        query_calls: AtomicU64,
        page: MailQueryPage,
    }

    impl CountingSource {
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
    impl ReadSource for CountingSource {
        async fn query_mail_page(
            &self,
            _request: MailQueryRequest,
        ) -> Result<MailQueryPage, RuntimeError> {
            self.query_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.page.clone())
        }

        async fn current_summary(
            &self,
            _account_id: &AccountId,
            message_id: &MessageId,
        ) -> Result<Option<MessageSummary>, RuntimeError> {
            self.summary_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Some(summary(message_id.as_str())))
        }
    }

    fn base_frame(message_id: &str) -> DownFrame {
        DownFrame::Base {
            assertions: vec![BaseAssertion {
                message_id: message_id.to_string(),
                update: BaseUpdate::Present(MessageFoldState::default()),
            }],
        }
    }

    #[tokio::test]
    async fn passthrough_never_retains() {
        let source = Arc::new(CountingSource::new(vec![]));
        let cache = ReadCache::passthrough(source.clone());
        let account = AccountId("acct".into());
        let message = MessageId("m1".into());
        cache.current_summary(&account, &message).await.unwrap();
        cache.current_summary(&account, &message).await.unwrap();
        assert_eq!(source.summary_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn retaining_serves_a_point_read_from_cache_after_the_first() {
        let source = Arc::new(CountingSource::new(vec![]));
        let cache = ReadCache::retaining(source.clone());
        let account = AccountId("acct".into());
        let message = MessageId("m1".into());
        cache.current_summary(&account, &message).await.unwrap();
        cache.current_summary(&account, &message).await.unwrap();
        // Second read is a cache hit.
        assert_eq!(source.summary_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_query_warms_the_message_cache() {
        let source = Arc::new(CountingSource::new(vec![summary("m1")]));
        let cache = ReadCache::retaining(source.clone());
        let request: MailQueryRequest = serde_json::from_value(json!({
            "query": "in:acct/inbox",
            "presentation": {
                "kind": "messages", "limit": 10, "cursor": null,
                "sortField": "date", "sortDirection": "desc"
            }
        }))
        .unwrap();
        cache.query_mail_page(request).await.unwrap();
        // The point read is now served from the warmed cache, never reaching the source.
        cache
            .current_summary(&AccountId("acct".into()), &MessageId("m1".into()))
            .await
            .unwrap();
        assert_eq!(source.summary_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_coherence_frame_evicts_so_the_next_read_refetches() {
        let source = Arc::new(CountingSource::new(vec![]));
        let cache = ReadCache::retaining(source.clone());
        let account = AccountId("acct".into());
        let message = MessageId("m1".into());
        cache.current_summary(&account, &message).await.unwrap();
        // The backend asserts m1 changed; the cached summary is evicted.
        cache.apply_coherence_frame(&base_frame("m1"));
        cache.current_summary(&account, &message).await.unwrap();
        assert_eq!(source.summary_calls.load(Ordering::SeqCst), 2);
    }
}
