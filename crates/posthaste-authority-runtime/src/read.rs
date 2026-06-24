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

use std::sync::Arc;

use async_trait::async_trait;
use posthaste_domain::{AccountId, MessageId, MessageSummary};
use posthaste_runtime_contract::{MailQueryPage, MailQueryRequest, RuntimeError};

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
}

/// A read-through cache over a [`ReadSource`], parameterized by policy. W4a
/// implements only the **passthrough** policy: every read delegates straight to
/// the source, retaining nothing — the co-located default. A retaining policy
/// (W4c) serves hits from a coherent cache and reads through on a miss.
pub(crate) struct ReadCache {
    source: Arc<dyn ReadSource>,
}

impl ReadCache {
    /// The passthrough cache: read straight through, retain nothing.
    pub(crate) fn passthrough(source: Arc<dyn ReadSource>) -> Self {
        Self { source }
    }

    pub(crate) async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        self.source.query_mail_page(request).await
    }

    pub(crate) async fn current_summary(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageSummary>, RuntimeError> {
        self.source.current_summary(account_id, message_id).await
    }
}
