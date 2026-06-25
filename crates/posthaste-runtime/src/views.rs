use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use posthaste_domain::{
    AccountId, ConversationId, DomainEvent, MessageId, EVENT_TOPIC_ACCOUNT_CREATED,
    EVENT_TOPIC_ACCOUNT_DELETED, EVENT_TOPIC_ACCOUNT_STATUS_CHANGED, EVENT_TOPIC_ACCOUNT_UPDATED,
    EVENT_TOPIC_MESSAGE_UPDATED,
};
use posthaste_runtime_contract::{
    MailListAnchorState, MailListContinuation, MailListProjectionKind, MailListRowState,
    MailListViewState, MailPresentationRequest, MailQueryPage, MailQueryRequest, ReadWatermark,
    RuntimeCoverage, RuntimeCoverageKind, RuntimeError, RuntimeErrorCode, RuntimeViewSubscription,
    ViewDescriptor, ViewFrame, ViewId, ViewLifecycle, ViewRevision, ViewSnapshot,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::broadcast;
use tokio::task::AbortHandle;

/// The parsed, family-specific identity of a runtime view. The registry is
/// generic over families: each carries what `build_snapshot` and the event
/// pump need, so adding a family is a new variant rather than new registry
/// machinery.
///
/// @spec docs/runtime/adapter/L1#view-descriptors
#[derive(Clone)]
enum ViewKind {
    MailList(MailQueryRequest),
    MessageDetail {
        source_id: String,
        message_id: String,
    },
    Conversation {
        conversation_id: String,
    },
    /// Folded account overview(s): `None` serves the full account list, `Some`
    /// serves one account. @spec docs/runtime/adapter/L2#account-status-views
    AccountStatus {
        account_id: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageDetailDescriptor {
    source_id: String,
    message_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationDescriptor {
    conversation_id: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AccountStatusDescriptor {
    #[serde(default)]
    account_id: Option<String>,
}

pub(crate) struct ViewRegistry {
    event_sender: broadcast::Sender<DomainEvent>,
    /// The runtime's outbox toward the backend, folded over mail-list recomputes
    /// so served views are optimistic over forwarded-but-unconfirmed mutations
    /// ([replication backend-link L2 §5](../replication/backend-link/L2.md)).
    outbox: Arc<crate::near_node::RuntimeBackendOutbox>,
    /// The mail-list base read through the far node (W4a: passthrough cache over
    /// the in-process backend, behavior-preserving).
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
    pub(crate) fn new(
        event_sender: broadcast::Sender<DomainEvent>,
        outbox: Arc<crate::near_node::RuntimeBackendOutbox>,
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
        let snapshot = self
            .build_snapshot(
                view_id.clone(),
                descriptor.clone(),
                &kind,
                ViewRevision::new(1),
            )
            .await?;
        let (frames, _) = broadcast::channel(16);
        self.views.lock().map_err(lock_error)?.insert(
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
        if let Some(view) = self.views.lock().map_err(lock_error)?.get_mut(&view_id) {
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
        let snapshot = self
            .build_snapshot(
                view_id.clone(),
                current.descriptor.clone(),
                &kind,
                next_revision,
            )
            .await?;
        if let Some(view) = self.views.lock().map_err(lock_error)?.get_mut(view_id) {
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
                        if event_affects_view(&view.kind, &event) {
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
            .views
            .lock()
            .map_err(lock_error)?
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
        self.views
            .lock()
            .map(|views| views.contains_key(view_id))
            .unwrap_or(false)
    }

    fn current_view(&self, view_id: &ViewId) -> Result<StoredView, RuntimeError> {
        self.views
            .lock()
            .map_err(lock_error)?
            .get(view_id)
            .cloned()
            .ok_or_else(|| RuntimeError::not_found("view not found"))
    }

    async fn recompute_view(&self, view_id: &ViewId) -> Result<ViewSnapshot, RuntimeError> {
        let current = self.current_view(view_id)?;
        let next_revision = ViewRevision::new(current.snapshot.revision.get() + 1);
        let snapshot = self
            .build_snapshot(
                view_id.clone(),
                current.descriptor.clone(),
                &current.kind,
                next_revision,
            )
            .await?;
        if let Some(view) = self.views.lock().map_err(lock_error)?.get_mut(view_id) {
            view.snapshot = snapshot.clone();
        }
        Ok(snapshot)
    }

    async fn recompute_view_if_changed(
        &self,
        view_id: &ViewId,
    ) -> Result<Option<ViewSnapshot>, RuntimeError> {
        let current = self.current_view(view_id)?;
        let next_revision = ViewRevision::new(current.snapshot.revision.get() + 1);
        let snapshot = self
            .build_snapshot(
                view_id.clone(),
                current.descriptor.clone(),
                &current.kind,
                next_revision,
            )
            .await?;
        if snapshot.data == current.snapshot.data {
            return Ok(None);
        }
        if let Some(view) = self.views.lock().map_err(lock_error)?.get_mut(view_id) {
            view.snapshot = snapshot.clone();
        }
        Ok(Some(snapshot))
    }

    async fn build_snapshot(
        &self,
        view_id: ViewId,
        descriptor: ViewDescriptor,
        kind: &ViewKind,
        revision: ViewRevision,
    ) -> Result<ViewSnapshot, RuntimeError> {
        let (data, read_watermark, coverage) = match kind {
            ViewKind::MailList(request) => {
                let page = self.reads.query_mail_page(request.clone()).await?;
                let mut state = mail_list_state(request, page)?;
                // Fold the runtime→backend outbox: served rows are optimistic
                // over forwarded-but-unconfirmed mutations. A no-op when the
                // outbox is empty (the in-process default), so co-located
                // behavior is unchanged (`colocated-unchanged`).
                crate::near_node::apply_outbox_overlay(&mut state, &self.outbox.snapshot());
                let read_watermark = state.read_watermark.clone();
                let coverage = state.coverage.clone();
                let data = serde_json::to_value(state).map_err(|error| {
                    RuntimeError::new(RuntimeErrorCode::Internal, error.to_string())
                })?;
                (data, read_watermark, coverage)
            }
            ViewKind::MessageDetail {
                source_id,
                message_id,
            } => {
                // Body-free by construction (message_detail uses the header read);
                // the body is the separate sanitized `/body` lazy resource, so the
                // view never serves the (unsanitized) cached body.
                let detail = self
                    .reads
                    .message_detail(
                        &AccountId::from(source_id.clone()),
                        &MessageId::from(message_id.clone()),
                    )
                    .await?
                    .ok_or_else(|| RuntimeError::not_found("message not found"))?;
                let data = serde_json::to_value(detail).map_err(|error| {
                    RuntimeError::new(RuntimeErrorCode::Internal, error.to_string())
                })?;
                (data, local_watermark(), complete_coverage())
            }
            ViewKind::Conversation { conversation_id } => {
                let conversation = self
                    .reads
                    .conversation(&ConversationId::from(conversation_id.clone()))
                    .await?;
                let data = serde_json::to_value(conversation).map_err(|error| {
                    RuntimeError::new(RuntimeErrorCode::Internal, error.to_string())
                })?;
                (data, local_watermark(), complete_coverage())
            }
            ViewKind::AccountStatus { account_id } => {
                // Account status reads through the link (the same `reads` path
                // as every other view), so a backend-less runtime serves it too.
                let data = match account_id {
                    Some(account_id) => {
                        let overview = self
                            .reads
                            .get_account(AccountId::from(account_id.clone()))
                            .await?;
                        serde_json::to_value(overview).map_err(|error| {
                            RuntimeError::new(RuntimeErrorCode::Internal, error.to_string())
                        })?
                    }
                    None => {
                        let list = self.reads.list_accounts().await?;
                        serde_json::to_value(list.items).map_err(|error| {
                            RuntimeError::new(RuntimeErrorCode::Internal, error.to_string())
                        })?
                    }
                };
                (data, local_watermark(), complete_coverage())
            }
        };
        Ok(ViewSnapshot {
            view_id,
            descriptor,
            revision,
            lifecycle: ViewLifecycle::Ready,
            read_watermark,
            coverage,
            data,
            pending_mutations: Vec::new(),
            error: None,
        })
    }
}

fn lock_error<T>(_error: T) -> RuntimeError {
    RuntimeError::new(RuntimeErrorCode::Internal, "view registry lock poisoned")
}

/// Parse a view descriptor into its family-specific [`ViewKind`].
fn parse_view_kind(descriptor: &ViewDescriptor) -> Result<ViewKind, RuntimeError> {
    match descriptor.family.as_str() {
        "mailList" => {
            let request = serde_json::from_value(descriptor.payload.clone()).map_err(|error| {
                RuntimeError::invalid_descriptor(format!("invalid mailList descriptor: {error}"))
            })?;
            Ok(ViewKind::MailList(request))
        }
        "messageDetail" => {
            let descriptor: MessageDetailDescriptor =
                serde_json::from_value(descriptor.payload.clone()).map_err(|error| {
                    RuntimeError::invalid_descriptor(format!(
                        "invalid messageDetail descriptor: {error}"
                    ))
                })?;
            Ok(ViewKind::MessageDetail {
                source_id: descriptor.source_id,
                message_id: descriptor.message_id,
            })
        }
        "conversation" => {
            let descriptor: ConversationDescriptor =
                serde_json::from_value(descriptor.payload.clone()).map_err(|error| {
                    RuntimeError::invalid_descriptor(format!(
                        "invalid conversation descriptor: {error}"
                    ))
                })?;
            Ok(ViewKind::Conversation {
                conversation_id: descriptor.conversation_id,
            })
        }
        "accountStatus" => {
            // An empty/absent payload is the all-accounts variant.
            let descriptor: AccountStatusDescriptor = if descriptor.payload.is_null() {
                AccountStatusDescriptor::default()
            } else {
                serde_json::from_value(descriptor.payload.clone()).map_err(|error| {
                    RuntimeError::invalid_descriptor(format!(
                        "invalid accountStatus descriptor: {error}"
                    ))
                })?
            };
            Ok(ViewKind::AccountStatus {
                account_id: descriptor.account_id,
            })
        }
        other => Err(RuntimeError::invalid_descriptor(format!(
            "unsupported view family '{other}'"
        ))),
    }
}

/// Grow a mail-query window by `count` rows so an extend re-queries the larger
/// first-N window (consistent with how event recompute already re-reads it).
fn grow_message_window(request: &mut MailQueryRequest, count: usize) {
    match &mut request.presentation {
        MailPresentationRequest::Messages { limit, .. } => {
            *limit = Some(limit.unwrap_or(0).saturating_add(count));
        }
        MailPresentationRequest::CollapsedByConversation { limit, .. } => {
            *limit = limit.saturating_add(count);
        }
    }
}

fn local_watermark() -> Option<ReadWatermark> {
    Some(ReadWatermark {
        value: "local".to_string(),
    })
}

fn complete_coverage() -> RuntimeCoverage {
    RuntimeCoverage {
        kind: RuntimeCoverageKind::Complete,
        details: Value::Null,
    }
}

/// Whether a `message.updated` event can change which rows a mail list shows or
/// their order: a keyword or mailbox-membership change, a newly arrived message,
/// or a deletion. The diff payload always carries the `changes` flags and
/// `created`; deletion events carry `deleted`.
fn message_event_affects_list(event: &DomainEvent) -> bool {
    let changes = &event.payload["changes"];
    changes["keywords"] == true
        || changes["mailboxes"] == true
        || event.payload["created"] == true
        || event.payload["deleted"] == true
}

/// Whether a domain event should trigger a recompute for a view of this kind.
/// mailList recomputes when message membership/ordering may change (keyword
/// assertions); messageDetail recomputes on any update to its own message.
fn event_affects_view(kind: &ViewKind, event: &DomainEvent) -> bool {
    match kind {
        // A mail list is derived from message membership, ordering, and keyword
        // state. It must recompute on every change that can add, remove, or
        // reorder a row: keyword assertions (read/flag), mailbox membership
        // (archive/move/trash), arrival of a new message, and deletion. The
        // earlier keywords-only gate dropped archives and new arrivals, so the
        // list went stale until the view was reopened. The snapshot equality
        // check in `recompute_view_if_changed` suppresses no-op recomputes, so a
        // broad trigger costs at most a wasted query.
        ViewKind::MailList(_) => {
            event.topic == EVENT_TOPIC_MESSAGE_UPDATED && message_event_affects_list(event)
        }
        ViewKind::MessageDetail {
            source_id,
            message_id,
        } => {
            event.topic == EVENT_TOPIC_MESSAGE_UPDATED
                && event.account_id.as_str() == source_id
                && event.message_id.as_ref().map(MessageId::as_str) == Some(message_id.as_str())
        }
        // Conversations are derived from messages; recompute on any message
        // update and let the data-equality check suppress no-op replacements.
        ViewKind::Conversation { .. } => event.topic == EVENT_TOPIC_MESSAGE_UPDATED,
        // Account config + runtime status events; the all-accounts variant
        // recomputes on any account event, the per-account variant on its own.
        // Data-equality suppression elides no-op recomputes.
        ViewKind::AccountStatus { account_id } => {
            let is_account_event = matches!(
                event.topic.as_str(),
                EVENT_TOPIC_ACCOUNT_STATUS_CHANGED
                    | EVENT_TOPIC_ACCOUNT_CREATED
                    | EVENT_TOPIC_ACCOUNT_UPDATED
                    | EVENT_TOPIC_ACCOUNT_DELETED
            );
            is_account_event
                && account_id
                    .as_ref()
                    .is_none_or(|id| event.account_id.as_str() == id)
        }
    }
}

fn validate_kind_account_scope(
    kind: &ViewKind,
    account_scope: Option<&[String]>,
) -> Result<(), RuntimeError> {
    let Some(account_scope) = account_scope else {
        return Ok(());
    };
    let in_scope = match kind {
        ViewKind::MailList(request) => {
            account_scope.is_empty()
                || account_scope
                    .iter()
                    .any(|source_id| mail_query_contains_source_scope(&request.query, source_id))
        }
        ViewKind::MessageDetail { source_id, .. } => {
            account_scope.is_empty() || account_scope.iter().any(|id| id == source_id)
        }
        // The conversation id is opaque (it does not name an account); access is
        // gated at the API capability layer. A finer runtime-side scope check
        // would require reading the conversation first.
        ViewKind::Conversation { .. } => true,
        // The all-accounts view is global (settings shows every account); a
        // per-account view must be in scope.
        ViewKind::AccountStatus { account_id } => match account_id {
            None => true,
            Some(account_id) => {
                account_scope.is_empty() || account_scope.iter().any(|id| id == account_id)
            }
        },
    };
    if in_scope {
        return Ok(());
    }
    Err(RuntimeError::invalid_descriptor(
        "view descriptor is outside the caller account scope",
    ))
}

fn mail_query_contains_source_scope(query: &str, source_id: &str) -> bool {
    query.split_whitespace().any(|token| {
        let token = token
            .trim_start_matches('!')
            .trim_start_matches('-')
            .trim_start_matches('+');
        let Some(selector) = token
            .strip_prefix("in:")
            .or_else(|| token.strip_prefix("IN:"))
        else {
            return false;
        };
        selector
            .split_once('/')
            .is_some_and(|(account, _mailbox)| account == source_id)
    })
}

fn mail_list_state(
    request: &MailQueryRequest,
    page: MailQueryPage,
) -> Result<MailListViewState, RuntimeError> {
    let MailQueryPage::Messages(page) = page else {
        return Err(RuntimeError::invalid_descriptor(
            "mailList views require a message presentation",
        ));
    };
    let rows = page
        .items
        .iter()
        .enumerate()
        .map(|(index, message)| {
            let projection = serde_json::to_value(message).unwrap_or(Value::Null);
            MailListRowState {
                row_key: format!("{}:{}", message.source_id.as_str(), message.id.as_str()),
                resource_ref: Some(format!(
                    "message:{}:{}",
                    message.source_id.as_str(),
                    message.id.as_str()
                )),
                sort_key: json!([message.received_at, message.id.as_str()]),
                projection,
                order_key: format!("{index:08}"),
                pending_markers: Vec::new(),
            }
        })
        .collect();
    Ok(MailListViewState {
        scope: json!({ "query": request.query }),
        projection_kind: MailListProjectionKind::Message,
        sort: presentation_sort(&request.presentation),
        window_request: presentation_window(&request.presentation),
        rows,
        continuation: MailListContinuation {
            before_cursor: None,
            after_cursor: page
                .next_cursor
                .as_ref()
                .and_then(|cursor| serde_json::to_string(cursor).ok()),
            has_before: false,
            has_after: page.next_cursor.is_some(),
        },
        read_watermark: Some(ReadWatermark {
            value: "local".to_string(),
        }),
        coverage: RuntimeCoverage {
            kind: RuntimeCoverageKind::Complete,
            details: Value::Null,
        },
        known_total_count: None,
        pending_mutations: Vec::new(),
        anchor: MailListAnchorState::NotRequested,
    })
}

fn presentation_sort(presentation: &MailPresentationRequest) -> Value {
    match presentation {
        MailPresentationRequest::Messages {
            sort_field,
            sort_direction,
            ..
        } => json!({ "field": sort_field, "direction": sort_direction }),
        MailPresentationRequest::CollapsedByConversation {
            sort_field,
            sort_direction,
            ..
        } => json!({ "field": sort_field, "direction": sort_direction }),
    }
}

fn presentation_window(presentation: &MailPresentationRequest) -> Value {
    match presentation {
        MailPresentationRequest::Messages { limit, cursor, .. } => {
            json!({ "limit": limit, "cursor": cursor })
        }
        MailPresentationRequest::CollapsedByConversation { limit, cursor, .. } => {
            json!({ "limit": limit, "cursor": cursor })
        }
    }
}

#[cfg(test)]
mod recompute_trigger_tests {
    use super::*;
    use posthaste_domain::EVENT_TOPIC_MESSAGE_UPDATED;
    use serde_json::json;

    fn message_event(payload: serde_json::Value) -> DomainEvent {
        DomainEvent {
            seq: 1,
            account_id: AccountId::from("acct"),
            topic: EVENT_TOPIC_MESSAGE_UPDATED.to_string(),
            occurred_at: "2026-06-23T00:00:00Z".to_string(),
            mailbox_id: None,
            message_id: Some(MessageId::from("m1")),
            payload,
        }
    }

    #[test]
    fn keyword_change_affects_list() {
        assert!(message_event_affects_list(&message_event(
            json!({ "changes": { "keywords": true, "mailboxes": false } })
        )));
    }

    #[test]
    fn mailbox_change_affects_list() {
        // Archive / move: membership changed, keywords did not. Regression guard
        // for the keywords-only gate that left archived rows in the list.
        assert!(message_event_affects_list(&message_event(
            json!({ "changes": { "keywords": false, "mailboxes": true } })
        )));
    }

    #[test]
    fn new_message_affects_list() {
        assert!(message_event_affects_list(&message_event(
            json!({ "created": true, "changes": { "keywords": true, "mailboxes": true } })
        )));
    }

    #[test]
    fn deletion_affects_list() {
        assert!(message_event_affects_list(&message_event(
            json!({ "deleted": true })
        )));
    }

    #[test]
    fn unrelated_payload_does_not_affect_list() {
        assert!(!message_event_affects_list(&message_event(
            json!({ "changes": { "keywords": false, "mailboxes": false } })
        )));
    }
}
