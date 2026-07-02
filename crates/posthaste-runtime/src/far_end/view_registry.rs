//! The serving half of the runtime's views (link far-end, RFC D37/D39): the
//! view registry — per-view frame broadcast, the event pump that drives
//! recompute-and-replace, subscription catch-up, and lifecycle (open / extend
//! / close). The projection half — what a view *is* and how its snapshot is
//! recomputed — lives in [`crate::views`] and is consumed from here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use futures_util::StreamExt;
use posthaste_client_link::RuntimeViewSubscription;
use posthaste_contract_core::{
    RuntimeError, ViewDescriptor, ViewFrame, ViewId, ViewRevision, ViewSnapshot,
};
use posthaste_domain_model::DomainEvent;
use tokio::sync::broadcast;
use tokio::task::AbortHandle;
use tracing::warn;

use crate::views::{
    build_snapshot, event_affects_view, grow_message_window, parse_view_kind,
    validate_kind_account_scope, ViewKind,
};

pub(crate) struct ViewRegistry {
    event_sender: broadcast::Sender<DomainEvent>,
    /// The runtime's outbox toward the authority server, folded over mail-list recomputes
    /// so served views are optimistic over forwarded-but-unconfirmed mutations
    /// ([replication authority-server-link L2 §5](../replication/authority-server-link/L2.md)).
    outbox: Arc<crate::near_node::RuntimeAuthorityServerOutbox>,
    /// The mail-list base read through the far node (W4a: passthrough cache over
    /// the in-process authority server, behavior-preserving).
    reads: Arc<crate::read::ReadCache>,
    views: Mutex<HashMap<ViewId, StoredView>>,
    next_view_id: AtomicU64,
}

#[derive(Clone)]
struct StoredView {
    descriptor: ViewDescriptor,
    kind: ViewKind,
    snapshot: ViewSnapshot,
    frames: broadcast::Sender<ViewFrame>,
    event_task: Option<AbortHandle>,
}

impl ViewRegistry {
    fn lock_views(&self) -> MutexGuard<'_, HashMap<ViewId, StoredView>> {
        match self.views.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("view registry mutex was poisoned; recovering");
                poisoned.into_inner()
            }
        }
    }

    pub(crate) fn new(
        event_sender: broadcast::Sender<DomainEvent>,
        outbox: Arc<crate::near_node::RuntimeAuthorityServerOutbox>,
        reads: Arc<crate::read::ReadCache>,
    ) -> Self {
        Self {
            event_sender,
            outbox,
            reads,
            views: Mutex::new(HashMap::new()),
            next_view_id: AtomicU64::new(1),
        }
    }

    pub(crate) async fn open_view(
        self: &Arc<Self>,
        descriptor: ViewDescriptor,
        account_scope: Option<&[String]>,
    ) -> Result<ViewSnapshot, RuntimeError> {
        let kind = parse_view_kind(&descriptor)?;
        validate_kind_account_scope(&kind, account_scope)?;
        let view_id = ViewId::new(format!(
            "view-{}",
            self.next_view_id.fetch_add(1, Ordering::Relaxed)
        ));
        let snapshot = build_snapshot(
            &self.reads,
            &self.outbox,
            view_id.clone(),
            descriptor.clone(),
            &kind,
            ViewRevision::new(1),
        )
        .await?;
        let (frames, _) = broadcast::channel(16);
        self.lock_views().insert(
            view_id.clone(),
            StoredView {
                descriptor,
                kind,
                snapshot: snapshot.clone(),
                frames,
                event_task: None,
            },
        );
        let event_task = self.spawn_event_pump(view_id.clone());
        if let Some(view) = self.lock_views().get_mut(&view_id) {
            view.event_task = Some(event_task);
        }
        Ok(snapshot)
    }

    /// Grow a windowed view's window by `count` rows in place, recompute its
    /// snapshot, store the larger window, and broadcast a `Replace` so every
    /// subscriber (and the caller) sees the extended page. Only the windowed
    /// `mailList` family supports this; single-object views reject it.
    ///
    /// @spec docs/runtime/adapter/L2#view-operation-flow
    pub(crate) async fn extend_view(
        self: &Arc<Self>,
        view_id: &ViewId,
        count: usize,
        account_scope: Option<&[String]>,
    ) -> Result<ViewSnapshot, RuntimeError> {
        let current = self.current_view(view_id)?;
        validate_kind_account_scope(&current.kind, account_scope)?;
        let ViewKind::MailList(request) = &current.kind else {
            return Err(RuntimeError::invalid_descriptor(
                "view family does not support window extension",
            ));
        };
        let mut request = request.clone();
        grow_message_window(&mut request, count);
        let kind = ViewKind::MailList(request);
        let next_revision = ViewRevision::new(current.snapshot.revision.get() + 1);
        let snapshot = build_snapshot(
            &self.reads,
            &self.outbox,
            view_id.clone(),
            current.descriptor.clone(),
            &kind,
            next_revision,
        )
        .await?;
        if let Some(view) = self.lock_views().get_mut(view_id) {
            view.kind = kind;
            view.snapshot = snapshot.clone();
        }
        self.send_view_frame(
            view_id,
            ViewFrame::Replace {
                snapshot: snapshot.clone(),
            },
        );
        Ok(snapshot)
    }

    pub(crate) fn subscribe_view(
        self: &Arc<Self>,
        view_id: ViewId,
        after_revision: Option<ViewRevision>,
        account_scope: Option<&[String]>,
    ) -> Result<RuntimeViewSubscription, RuntimeError> {
        let current = self.current_view(&view_id)?;
        validate_kind_account_scope(&current.kind, account_scope)?;
        let catch_up = if after_revision == Some(current.snapshot.revision) {
            None
        } else {
            Some(ViewFrame::Snapshot {
                snapshot: current.snapshot.clone(),
            })
        };
        let registry = self.clone();
        let mut receiver = current.frames.subscribe();
        let stream = async_stream::stream! {
            loop {
                match receiver.recv().await {
                    Ok(frame) => yield frame,
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        match registry.current_view(&view_id) {
                            Ok(view) => yield ViewFrame::Snapshot { snapshot: view.snapshot },
                            Err(error) => {
                                yield ViewFrame::Error {
                                    view_id: view_id.clone(),
                                    revision: ViewRevision::new(0),
                                    error: error.envelope().clone(),
                                };
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        };
        Ok(RuntimeViewSubscription {
            catch_up,
            live: stream.boxed(),
        })
    }

    fn spawn_event_pump(self: &Arc<Self>, view_id: ViewId) -> AbortHandle {
        let registry = Arc::downgrade(self);
        let mut receiver = self.event_sender.subscribe();
        let task = tokio::spawn(async move {
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        let Some(registry) = registry.upgrade() else {
                            break;
                        };
                        let Ok(view) = registry.current_view(&view_id) else {
                            break;
                        };
                        // The client entity store self-maintains evaluable
                        // mail-list membership from the `message.updated` firehose
                        // (option iii, single-source-view-membership), so the
                        // runtime no longer recomputes + re-serves the whole
                        // mail-list view per affecting event — the O(view)
                        // `build_snapshot` that dominated sync cost. Other view
                        // kinds (detail, conversation, account) are not
                        // client-self-maintained and still recompute. Resync
                        // re-derives mail-lists fresh (`refresh_open_views`).
                        // Self-maintained iff this is a mail-list the client
                        // store owns the membership of (evaluable predicate). The
                        // client stamps `client_self_maintained` on the view
                        // descriptor from its predicate; `Deferred` mail-lists
                        // (smart-mailbox / global / non-`date`) stay false and
                        // are still re-served per event — they have no client
                        // self-maintenance, so skipping would stale them until
                        // reload (the option-iii regression).
                        let self_maintained = matches!(view.kind, ViewKind::MailList(_))
                            && view.descriptor.client_self_maintained;
                        if !self_maintained && event_affects_view(&view.kind, &event) {
                            registry.send_recomputed_replace(&view_id).await;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let Some(registry) = registry.upgrade() else {
                            break;
                        };
                        if !registry.view_exists(&view_id) {
                            break;
                        }
                        registry.send_recomputed_snapshot(&view_id).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        task.abort_handle()
    }

    async fn send_recomputed_replace(&self, view_id: &ViewId) {
        match self.recompute_view_if_changed(view_id).await {
            Ok(Some(snapshot)) => self.send_view_frame(view_id, ViewFrame::Replace { snapshot }),
            Ok(None) => {}
            Err(error) => self.send_current_error(view_id, error),
        }
    }

    async fn send_recomputed_snapshot(&self, view_id: &ViewId) {
        match self.recompute_view(view_id).await {
            Ok(snapshot) => self.send_view_frame(view_id, ViewFrame::Snapshot { snapshot }),
            Err(error) => self.send_current_error(view_id, error),
        }
    }

    fn send_current_error(&self, view_id: &ViewId, error: RuntimeError) {
        let revision = self
            .current_view(view_id)
            .map(|view| view.snapshot.revision)
            .unwrap_or_else(|_| ViewRevision::new(0));
        self.send_view_frame(
            view_id,
            ViewFrame::Error {
                view_id: view_id.clone(),
                revision,
                error: error.envelope().clone(),
            },
        );
    }

    fn send_view_frame(&self, view_id: &ViewId, frame: ViewFrame) {
        if let Ok(view) = self.current_view(view_id) {
            let _ = view.frames.send(frame);
        }
    }

    pub(crate) fn close_view(&self, view_id: &ViewId) -> Result<(), RuntimeError> {
        let removed = self
            .lock_views()
            .remove(view_id)
            .ok_or_else(|| RuntimeError::not_found("view not found"))?;
        if let Some(event_task) = removed.event_task {
            event_task.abort();
        }
        let _ = removed.frames.send(ViewFrame::Closed {
            view_id: view_id.clone(),
        });
        Ok(())
    }

    fn view_exists(&self, view_id: &ViewId) -> bool {
        self.lock_views().contains_key(view_id)
    }

    fn current_view(&self, view_id: &ViewId) -> Result<StoredView, RuntimeError> {
        self.lock_views()
            .get(view_id)
            .cloned()
            .ok_or_else(|| RuntimeError::not_found("view not found"))
    }

    async fn recompute_view(&self, view_id: &ViewId) -> Result<ViewSnapshot, RuntimeError> {
        let current = self.current_view(view_id)?;
        let next_revision = ViewRevision::new(current.snapshot.revision.get() + 1);
        let snapshot = build_snapshot(
            &self.reads,
            &self.outbox,
            view_id.clone(),
            current.descriptor.clone(),
            &current.kind,
            next_revision,
        )
        .await?;
        if let Some(view) = self.lock_views().get_mut(view_id) {
            view.snapshot = snapshot.clone();
        }
        Ok(snapshot)
    }

    pub(crate) async fn recompute_view_if_changed(
        &self,
        view_id: &ViewId,
    ) -> Result<Option<ViewSnapshot>, RuntimeError> {
        let current = self.current_view(view_id)?;
        let next_revision = ViewRevision::new(current.snapshot.revision.get() + 1);
        let snapshot = build_snapshot(
            &self.reads,
            &self.outbox,
            view_id.clone(),
            current.descriptor.clone(),
            &current.kind,
            next_revision,
        )
        .await?;
        if snapshot.data == current.snapshot.data {
            return Ok(None);
        }
        if let Some(view) = self.lock_views().get_mut(view_id) {
            view.snapshot = snapshot.clone();
        }
        Ok(Some(snapshot))
    }
}

#[cfg(test)]
mod rev_log_view_tests {
    use super::*;
    use crate::near_node::RuntimeAuthorityServerOutbox;
    use crate::read::ReadCache;
    use async_trait::async_trait;
    use posthaste_authority_server_link::AuthorityServerApi;
    use posthaste_domain_model::{AccountId, RevCursor, RevLogSnapshot, RevLogStep};
    use serde_json::{json, Value};

    /// A read-only `AuthorityServerLink` stub that serves a canned `RevLogSnapshot` for
    /// every account — enough to drive the `RevLog` view's build/read path
    /// without the store plumbing (Slice 2b wires the real `LocalAuthorityServer`).
    struct RevLogStubAuthorityServerLink {
        snapshot: RevLogSnapshot,
    }

    #[async_trait]
    impl AuthorityServerApi for RevLogStubAuthorityServerLink {
        async fn rev_log_snapshot(
            &self,
            _account_id: AccountId,
        ) -> Result<RevLogSnapshot, RuntimeError> {
            Ok(self.snapshot.clone())
        }
    }

    fn registry(snapshot: RevLogSnapshot) -> Arc<ViewRegistry> {
        let (event_sender, _) = broadcast::channel(16);
        let outbox = Arc::new(RuntimeAuthorityServerOutbox::new(false));
        let reads = Arc::new(ReadCache::passthrough(Arc::new(
            RevLogStubAuthorityServerLink { snapshot },
        )));
        Arc::new(ViewRegistry::new(event_sender, outbox, reads))
    }

    fn rev_log_descriptor(account_id: &str) -> ViewDescriptor {
        ViewDescriptor {
            family: "revLog".to_string(),
            payload: json!({ "accountId": account_id }),
            client_self_maintained: false,
        }
    }

    fn sample_snapshot() -> RevLogSnapshot {
        RevLogSnapshot {
            steps: vec![
                RevLogStep {
                    step_id: "step-1".to_string(),
                    seq: 1,
                    message_id: "msg-1".to_string(),
                    source_id: "acct".to_string(),
                    diff: json!({"keywords": {"added": ["$seen"], "removed": []}}),
                    created_at: "2026-06-28T00:00:00Z".to_string(),
                },
                RevLogStep {
                    step_id: "step-2".to_string(),
                    seq: 2,
                    message_id: "msg-2".to_string(),
                    source_id: "acct".to_string(),
                    diff: json!({"mailboxes": {"added": ["archive"], "removed": ["inbox"]}}),
                    created_at: "2026-06-28T00:01:00Z".to_string(),
                },
            ],
            cursor: RevCursor {
                cursor_step_id: Some("step-1".to_string()),
                redo_tail: vec!["step-2".to_string()],
            },
        }
    }

    #[tokio::test]
    async fn rev_log_view_serves_the_log_and_cursor() {
        let views = registry(sample_snapshot());
        let snapshot = views
            .open_view(rev_log_descriptor("acct"), None)
            .await
            .expect("revLog view opens");
        // The snapshot data mirrors the canned log + cursor (camelCase wire).
        let steps = snapshot.data["steps"]
            .as_array()
            .expect("steps is an array");
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0]["stepId"], "step-1");
        assert_eq!(steps[0]["seq"], 1);
        assert_eq!(steps[1]["stepId"], "step-2");
        assert_eq!(steps[1]["seq"], 2);
        assert_eq!(snapshot.data["cursor"]["cursorStepId"], "step-1");
        assert_eq!(snapshot.data["cursor"]["redoTail"][0], "step-2");
    }

    #[tokio::test]
    async fn rev_log_view_serves_an_empty_account() {
        let views = registry(RevLogSnapshot::default());
        let snapshot = views
            .open_view(rev_log_descriptor("acct"), None)
            .await
            .expect("empty revLog view opens");
        assert_eq!(snapshot.data["steps"].as_array().unwrap().len(), 0);
        assert_eq!(snapshot.data["cursor"]["cursorStepId"], Value::Null);
        assert_eq!(
            snapshot.data["cursor"]["redoTail"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    #[tokio::test]
    async fn rev_log_view_rejects_an_out_of_scope_account() {
        let views = registry(sample_snapshot());
        // The caller's scope is ["other-acct"]; "acct" is outside it.
        let scope = vec!["other-acct".to_string()];
        let result = views
            .open_view(rev_log_descriptor("acct"), Some(scope.as_slice()))
            .await;
        assert!(result.is_err(), "out-of-scope revLog view must be rejected");
    }
}
