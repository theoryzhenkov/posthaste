use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use posthaste_domain::{DomainEvent, EVENT_TOPIC_MESSAGE_KEYWORDS_CHANGED};
use posthaste_runtime_contract::{
    MailListAnchorState, MailListContinuation, MailListProjectionKind, MailListRowState,
    MailListViewState, MailPresentationRequest, MailQueryPage, MailQueryRequest, ReadWatermark,
    RuntimeCoverage, RuntimeCoverageKind, RuntimeError, RuntimeErrorCode, RuntimeViewSubscription,
    ViewDescriptor, ViewFrame, ViewId, ViewLifecycle, ViewRevision, ViewSnapshot,
};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::mail_queries::MailQueryService;

pub(crate) struct ViewRegistry {
    mail_queries: Arc<MailQueryService>,
    event_sender: broadcast::Sender<DomainEvent>,
    views: Mutex<HashMap<ViewId, StoredView>>,
    next_view_id: AtomicU64,
}

#[derive(Clone)]
struct StoredView {
    descriptor: ViewDescriptor,
    request: MailQueryRequest,
    snapshot: ViewSnapshot,
    frames: broadcast::Sender<ViewFrame>,
}

impl ViewRegistry {
    pub(crate) fn new(
        mail_queries: Arc<MailQueryService>,
        event_sender: broadcast::Sender<DomainEvent>,
    ) -> Self {
        Self {
            mail_queries,
            event_sender,
            views: Mutex::new(HashMap::new()),
            next_view_id: AtomicU64::new(1),
        }
    }

    pub(crate) async fn open_view(
        self: &Arc<Self>,
        descriptor: ViewDescriptor,
        account_scope: Option<&[String]>,
    ) -> Result<ViewSnapshot, RuntimeError> {
        let request = mail_list_request(&descriptor)?;
        validate_request_account_scope(&request, account_scope)?;
        let view_id = ViewId::new(format!(
            "view-{}",
            self.next_view_id.fetch_add(1, Ordering::Relaxed)
        ));
        let snapshot = self
            .build_snapshot(
                view_id.clone(),
                descriptor.clone(),
                &request,
                ViewRevision::new(1),
            )
            .await?;
        let (frames, _) = broadcast::channel(16);
        self.views.lock().map_err(lock_error)?.insert(
            view_id.clone(),
            StoredView {
                descriptor,
                request,
                snapshot: snapshot.clone(),
                frames,
            },
        );
        self.spawn_event_pump(view_id);
        Ok(snapshot)
    }

    pub(crate) fn subscribe_view(
        self: &Arc<Self>,
        view_id: ViewId,
        after_revision: Option<ViewRevision>,
        account_scope: Option<&[String]>,
    ) -> Result<RuntimeViewSubscription, RuntimeError> {
        let current = self.current_view(&view_id)?;
        validate_request_account_scope(&current.request, account_scope)?;
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

    fn spawn_event_pump(self: &Arc<Self>, view_id: ViewId) {
        let registry = Arc::downgrade(self);
        let mut receiver = self.event_sender.subscribe();
        tokio::spawn(async move {
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        let Some(registry) = registry.upgrade() else {
                            break;
                        };
                        if event.topic == EVENT_TOPIC_MESSAGE_KEYWORDS_CHANGED {
                            registry.send_recomputed_replace(&view_id).await;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let Some(registry) = registry.upgrade() else {
                            break;
                        };
                        registry.send_recomputed_snapshot(&view_id).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
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
                &current.request,
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
                &current.request,
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
        request: &MailQueryRequest,
        revision: ViewRevision,
    ) -> Result<ViewSnapshot, RuntimeError> {
        let page = self.mail_queries.query_mail_page(request.clone()).await?;
        let state = mail_list_state(request, page)?;
        Ok(ViewSnapshot {
            view_id,
            descriptor,
            revision,
            lifecycle: ViewLifecycle::Ready,
            read_watermark: state.read_watermark.clone(),
            coverage: state.coverage.clone(),
            data: serde_json::to_value(state).map_err(|error| {
                RuntimeError::new(RuntimeErrorCode::Internal, error.to_string())
            })?,
            pending_mutations: Vec::new(),
            error: None,
        })
    }
}

fn lock_error<T>(_error: T) -> RuntimeError {
    RuntimeError::new(RuntimeErrorCode::Internal, "view registry lock poisoned")
}

fn mail_list_request(descriptor: &ViewDescriptor) -> Result<MailQueryRequest, RuntimeError> {
    if descriptor.family != "mailList" {
        return Err(RuntimeError::invalid_descriptor(format!(
            "unsupported view family '{}'",
            descriptor.family
        )));
    }
    serde_json::from_value(descriptor.payload.clone()).map_err(|error| {
        RuntimeError::invalid_descriptor(format!("invalid mailList descriptor: {error}"))
    })
}

fn validate_request_account_scope(
    request: &MailQueryRequest,
    account_scope: Option<&[String]>,
) -> Result<(), RuntimeError> {
    let Some(account_scope) = account_scope else {
        return Ok(());
    };
    if account_scope.is_empty()
        || account_scope
            .iter()
            .any(|source_id| mail_query_contains_source_scope(&request.query, source_id))
    {
        return Ok(());
    }
    Err(RuntimeError::invalid_descriptor(
        "mailList descriptor is outside the caller account scope",
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
