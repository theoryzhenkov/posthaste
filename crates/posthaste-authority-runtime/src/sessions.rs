use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use posthaste_domain::DomainEvent;
use posthaste_runtime_contract::{
    RuntimeCaller, RuntimeError, RuntimeFrame, RuntimeFrameSubscription, RuntimeSession,
    RuntimeSessionId, RuntimeSessionSeq, RuntimeViewSubscription, ViewDescriptor, ViewFrame,
    ViewId, ViewSnapshot,
};
use tokio::sync::broadcast;
use tokio::task::AbortHandle;

use crate::views::ViewRegistry;

pub(crate) struct SessionRegistry {
    views: Arc<ViewRegistry>,
    event_sender: broadcast::Sender<DomainEvent>,
    sessions: Mutex<HashMap<RuntimeSessionId, StoredSession>>,
    next_session_id: AtomicU64,
}

struct StoredSession {
    account_scope: Option<Vec<String>>,
    last_seq: u64,
    frames: broadcast::Sender<RuntimeFrame>,
    open_views: HashSet<ViewId>,
    latest_snapshots: HashMap<ViewId, ViewSnapshot>,
    event_task: Option<AbortHandle>,
}

impl SessionRegistry {
    pub(crate) fn new(
        views: Arc<ViewRegistry>,
        event_sender: broadcast::Sender<DomainEvent>,
    ) -> Self {
        Self {
            views,
            event_sender,
            sessions: Mutex::new(HashMap::new()),
            next_session_id: AtomicU64::new(1),
        }
    }

    pub(crate) fn open_session(
        self: &Arc<Self>,
        caller: RuntimeCaller,
    ) -> Result<RuntimeSession, RuntimeError> {
        let session_id = RuntimeSessionId::new(format!(
            "session-{}",
            self.next_session_id.fetch_add(1, Ordering::Relaxed)
        ));
        let (frames, _) = broadcast::channel(64);
        self.sessions.lock().map_err(lock_error)?.insert(
            session_id.clone(),
            StoredSession {
                account_scope: caller.account_scope,
                last_seq: 0,
                frames,
                open_views: HashSet::new(),
                latest_snapshots: HashMap::new(),
                event_task: None,
            },
        );
        let event_task = self.spawn_notification_forwarder(session_id.clone());
        if let Some(session) = self
            .sessions
            .lock()
            .map_err(lock_error)?
            .get_mut(&session_id)
        {
            session.event_task = Some(event_task);
        }
        Ok(RuntimeSession { session_id })
    }

    pub(crate) fn subscribe_frames(
        self: &Arc<Self>,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        after_seq: Option<RuntimeSessionSeq>,
    ) -> Result<RuntimeFrameSubscription, RuntimeError> {
        let (catch_up, mut receiver) = {
            let mut sessions = self.sessions.lock().map_err(lock_error)?;
            let session = sessions
                .get_mut(&session_id)
                .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
            ensure_caller_matches_session(session, caller.account_scope.as_deref())?;
            let current_seq = RuntimeSessionSeq::new(session.last_seq);
            let needs_initial_snapshots =
                session.last_seq == 0 && !session.latest_snapshots.is_empty();
            let catch_up = if after_seq == Some(current_seq) && !needs_initial_snapshots {
                Vec::new()
            } else {
                collapse_snapshots(session)
            };
            (catch_up, session.frames.subscribe())
        };

        let registry = self.clone();
        let caller_scope = caller.account_scope.clone();
        let stream = async_stream::stream! {
            loop {
                match receiver.recv().await {
                    Ok(frame) => yield frame,
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        match registry.collapse_session(&session_id, caller_scope.as_deref()) {
                            Ok(frames) => {
                                for frame in frames {
                                    yield frame;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        };

        Ok(RuntimeFrameSubscription {
            catch_up,
            live: stream.boxed(),
        })
    }

    pub(crate) fn close_session(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
    ) -> Result<(), RuntimeError> {
        let (open_views, event_task) = {
            let mut sessions = self.sessions.lock().map_err(lock_error)?;
            let session = sessions
                .get(&session_id)
                .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
            ensure_caller_matches_session(session, caller.account_scope.as_deref())?;
            let session = sessions
                .remove(&session_id)
                .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
            (session.open_views, session.event_task)
        };
        if let Some(event_task) = event_task {
            event_task.abort();
        }
        for view_id in open_views {
            let _ = self.views.close_view(&view_id);
        }
        Ok(())
    }

    pub(crate) async fn open_view(
        self: &Arc<Self>,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        descriptor: ViewDescriptor,
    ) -> Result<ViewSnapshot, RuntimeError> {
        let session_scope = self.session_scope(&session_id, caller.account_scope.as_deref())?;
        let snapshot = self
            .views
            .open_view(descriptor, session_scope.as_deref())
            .await?;
        let subscription = self.views.subscribe_view(
            snapshot.view_id.clone(),
            Some(snapshot.revision),
            session_scope.as_deref(),
        )?;
        self.record_open_view(&session_id, snapshot.clone())?;
        self.spawn_view_forwarder(session_id, subscription);
        Ok(snapshot)
    }

    pub(crate) fn close_view(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        view_id: ViewId,
    ) -> Result<(), RuntimeError> {
        let mut sessions = self.sessions.lock().map_err(lock_error)?;
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
        ensure_caller_matches_session(session, caller.account_scope.as_deref())?;
        session.open_views.remove(&view_id);
        session.latest_snapshots.remove(&view_id);
        let seq = next_seq(session);
        let sender = session.frames.clone();
        drop(sessions);
        let _ = self.views.close_view(&view_id);
        let _ = sender.send(RuntimeFrame::ViewClosed {
            session_seq: seq,
            view_id,
        });
        Ok(())
    }

    fn session_scope(
        &self,
        session_id: &RuntimeSessionId,
        caller_scope: Option<&[String]>,
    ) -> Result<Option<Vec<String>>, RuntimeError> {
        let sessions = self.sessions.lock().map_err(lock_error)?;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
        ensure_caller_matches_session(session, caller_scope)?;
        Ok(session.account_scope.clone())
    }

    fn record_open_view(
        &self,
        session_id: &RuntimeSessionId,
        snapshot: ViewSnapshot,
    ) -> Result<(), RuntimeError> {
        let mut sessions = self.sessions.lock().map_err(lock_error)?;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
        session.open_views.insert(snapshot.view_id.clone());
        session
            .latest_snapshots
            .insert(snapshot.view_id.clone(), snapshot);
        Ok(())
    }

    fn spawn_view_forwarder(
        self: &Arc<Self>,
        session_id: RuntimeSessionId,
        mut subscription: RuntimeViewSubscription,
    ) {
        let registry = Arc::downgrade(self);
        tokio::spawn(async move {
            if let Some(frame) = subscription.catch_up.take() {
                let Some(registry) = registry.upgrade() else {
                    return;
                };
                if !registry.forward_view_frame(&session_id, frame) {
                    return;
                }
            }
            while let Some(frame) = subscription.live.next().await {
                let Some(registry) = registry.upgrade() else {
                    return;
                };
                if !registry.forward_view_frame(&session_id, frame) {
                    return;
                }
            }
        });
    }

    fn spawn_notification_forwarder(self: &Arc<Self>, session_id: RuntimeSessionId) -> AbortHandle {
        let registry = Arc::downgrade(self);
        let mut receiver = self.event_sender.subscribe();
        let task = tokio::spawn(async move {
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        let Some(registry) = registry.upgrade() else {
                            return;
                        };
                        if !registry.forward_notification(&session_id, event) {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        task.abort_handle()
    }

    fn forward_view_frame(&self, session_id: &RuntimeSessionId, frame: ViewFrame) -> bool {
        let mut sessions = match self.sessions.lock().map_err(lock_error) {
            Ok(sessions) => sessions,
            Err(_) => return false,
        };
        let Some(session) = sessions.get_mut(session_id) else {
            return false;
        };
        let Some(runtime_frame) = view_frame_to_runtime(session, frame) else {
            return false;
        };
        let sender = session.frames.clone();
        drop(sessions);
        let _ = sender.send(runtime_frame);
        true
    }

    fn forward_notification(&self, session_id: &RuntimeSessionId, event: DomainEvent) -> bool {
        let mut sessions = match self.sessions.lock().map_err(lock_error) {
            Ok(sessions) => sessions,
            Err(_) => return false,
        };
        let Some(session) = sessions.get_mut(session_id) else {
            return false;
        };
        if !event_matches_session_scope(&event, session.account_scope.as_deref()) {
            return true;
        }
        let payload = match serde_json::to_value(&event) {
            Ok(payload) => payload,
            Err(_) => return false,
        };
        let frame = RuntimeFrame::Notification {
            session_seq: next_seq(session),
            kind: event.topic.clone(),
            payload,
        };
        let sender = session.frames.clone();
        drop(sessions);
        let _ = sender.send(frame);
        true
    }

    fn collapse_session(
        &self,
        session_id: &RuntimeSessionId,
        caller_scope: Option<&[String]>,
    ) -> Result<Vec<RuntimeFrame>, RuntimeError> {
        let mut sessions = self.sessions.lock().map_err(lock_error)?;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
        ensure_caller_matches_session(session, caller_scope)?;
        Ok(collapse_snapshots(session))
    }
}

fn collapse_snapshots(session: &mut StoredSession) -> Vec<RuntimeFrame> {
    let mut snapshots: Vec<_> = session.latest_snapshots.values().cloned().collect();
    snapshots.sort_by(|left, right| left.view_id.as_str().cmp(right.view_id.as_str()));
    snapshots
        .into_iter()
        .map(|snapshot| {
            let session_seq = next_seq(session);
            RuntimeFrame::ViewSnapshot {
                session_seq,
                view_id: snapshot.view_id.clone(),
                revision: snapshot.revision,
                snapshot,
            }
        })
        .collect()
}

fn view_frame_to_runtime(session: &mut StoredSession, frame: ViewFrame) -> Option<RuntimeFrame> {
    match frame {
        ViewFrame::Snapshot { snapshot } => {
            if !session.open_views.contains(&snapshot.view_id) {
                return None;
            }
            let session_seq = next_seq(session);
            let view_id = snapshot.view_id.clone();
            let revision = snapshot.revision;
            session
                .latest_snapshots
                .insert(view_id.clone(), snapshot.clone());
            Some(RuntimeFrame::ViewSnapshot {
                session_seq,
                view_id,
                revision,
                snapshot,
            })
        }
        ViewFrame::Replace { snapshot } => {
            if !session.open_views.contains(&snapshot.view_id) {
                return None;
            }
            let session_seq = next_seq(session);
            let view_id = snapshot.view_id.clone();
            let revision = snapshot.revision;
            session
                .latest_snapshots
                .insert(view_id.clone(), snapshot.clone());
            Some(RuntimeFrame::ViewReplace {
                session_seq,
                view_id,
                revision,
                snapshot,
            })
        }
        ViewFrame::Error { view_id, error, .. } => {
            if !session.open_views.contains(&view_id) {
                return None;
            }
            let session_seq = next_seq(session);
            Some(RuntimeFrame::ViewError {
                session_seq,
                view_id,
                error,
            })
        }
        ViewFrame::Closed { view_id } => {
            if !session.open_views.remove(&view_id) {
                return None;
            }
            session.latest_snapshots.remove(&view_id);
            let session_seq = next_seq(session);
            Some(RuntimeFrame::ViewClosed {
                session_seq,
                view_id,
            })
        }
    }
}

fn next_seq(session: &mut StoredSession) -> RuntimeSessionSeq {
    session.last_seq += 1;
    RuntimeSessionSeq::new(session.last_seq)
}

fn event_matches_session_scope(event: &DomainEvent, account_scope: Option<&[String]>) -> bool {
    account_scope
        .map(|scope| {
            scope
                .iter()
                .any(|account_id| account_id == event.account_id.as_str())
        })
        .unwrap_or(true)
}

fn ensure_caller_matches_session(
    session: &StoredSession,
    caller_scope: Option<&[String]>,
) -> Result<(), RuntimeError> {
    match (session.account_scope.as_deref(), caller_scope) {
        (None, None) => Ok(()),
        (Some(_), None) => Ok(()),
        (None, Some(_)) => Err(RuntimeError::unauthorized(
            "account-scoped caller cannot access a full-scope runtime session",
        )),
        (Some(session_scope), Some(caller_scope)) if session_scope == caller_scope => Ok(()),
        (Some(_), Some(_)) => Err(RuntimeError::unauthorized(
            "caller account scope does not match runtime session",
        )),
    }
}

fn lock_error<T>(_error: T) -> RuntimeError {
    RuntimeError::new(
        posthaste_runtime_contract::RuntimeErrorCode::Internal,
        "runtime session registry lock poisoned",
    )
}
