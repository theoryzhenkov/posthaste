use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use futures_util::StreamExt;
use posthaste_domain::{DomainEvent, Id};
use posthaste_link_core::MessageChangeDiff;
use posthaste_runtime_contract::{
    ClientMutationId, DiffStep, MailListDelta, MailListRowState, MailListViewState,
    MutationReceipt, MutationRequest, MutationSettlementState, RuntimeAdapterError, RuntimeCaller,
    RuntimeError, RuntimeFrame, RuntimeFrameSubscription, RuntimeMutationId,
    RuntimeMutationSettlement, RuntimeSession, RuntimeSessionId, RuntimeSessionSeq,
    RuntimeViewSubscription, ViewDescriptor, ViewFrame, ViewId, ViewSnapshot,
};
use serde_json::Value;
use tokio::sync::broadcast;
use tokio::task::AbortHandle;
use tracing::{debug, warn};

use crate::views::ViewRegistry;

/// Capacity of the per-session frame broadcast channel. A burst (e.g. a sync
/// delivering many messages at once) emits one view frame per recompute; if the
/// SSE consumer can't drain them before the channel fills, `recv` returns
/// `Lagged` and we recover by collapsing to current state. Sized generously so
/// ordinary bursts never lag; the collapse path remains the safety net.
const SESSION_FRAME_CHANNEL_CAPACITY: usize = 512;

pub(crate) struct SessionRegistry {
    views: Arc<ViewRegistry>,
    event_sender: broadcast::Sender<DomainEvent>,
    sessions: Mutex<HashMap<RuntimeSessionId, StoredSession>>,
    next_mutation_id: AtomicU64,
}

struct StoredSession {
    account_scope: Option<Vec<String>>,
    /// The session opted into incremental mail-list deltas ([`RuntimeFrame::ViewDelta`])
    /// instead of whole-view replaces (L6).
    delta_capable: bool,
    last_seq: u64,
    frames: broadcast::Sender<RuntimeFrame>,
    open_views: HashSet<ViewId>,
    latest_snapshots: HashMap<ViewId, ViewSnapshot>,
    latest_mutations: HashMap<RuntimeMutationId, StoredMutation>,
    mutations_by_client_id: HashMap<ClientMutationId, RuntimeMutationId>,
    /// Terminal mutation IDs in settlement order, used to evict the oldest
    /// settled mutations once the live catch-up window reaches its cap.
    settled_mutation_ids: VecDeque<RuntimeMutationId>,
    /// Reversible mutations the session has applied, oldest first. Each step is
    /// an invertible change-diff; undo applies `inverse(diff)`, redo applies
    /// `diff`, both as ordinary `message.applyDiff` mutations.
    undo_history: Vec<DiffStep>,
    /// Steps moved off `undo_history` by an undo, oldest first. Redo replays the
    /// most recent. Cleared whenever a new user mutation's diff is recorded.
    redo_history: Vec<DiffStep>,
    event_task: Option<AbortHandle>,
}

/// Upper bound on each session's undo and redo history (matches the renderer's
/// former client-side bound).
const MAX_HISTORY: usize = 50;

/// Upper bound on the number of terminal mutations retained in a session for
/// catch-up retransmission. Older mutations are evicted so reconnect cost stays
/// bounded rather than growing with session age.
const MAX_LATEST_MUTATIONS: usize = 100;

#[derive(Clone)]
struct StoredMutation {
    mutation_id: RuntimeMutationId,
    client_mutation_id: ClientMutationId,
    name: String,
    args: Value,
    state: MutationSettlementState,
    error: Option<RuntimeAdapterError>,
    output: Value,
}

pub(crate) enum MutationAcceptance {
    New { mutation_id: RuntimeMutationId },
    Existing(MutationReceipt),
}

impl StoredMutation {
    fn receipt(&self) -> MutationReceipt {
        MutationReceipt {
            runtime_mutation_id: Some(self.mutation_id.clone()),
            client_mutation_id: self.client_mutation_id.clone(),
            name: self.name.clone(),
            state: self.state.clone(),
            error: self.error.clone(),
            output: self.output.clone(),
        }
    }

    fn settlement(&self) -> RuntimeMutationSettlement {
        RuntimeMutationSettlement {
            client_mutation_id: self.client_mutation_id.clone(),
            name: self.name.clone(),
            status: self.state.clone(),
            error: self.error.clone(),
        }
    }

    fn frame(&self, session_seq: RuntimeSessionSeq) -> RuntimeFrame {
        RuntimeFrame::MutationSettlement {
            session_seq,
            mutation_id: self.mutation_id.clone(),
            state: self.settlement(),
        }
    }
}

impl StoredSession {
    /// Evict the oldest terminal mutations once the live catch-up window is
    /// full, keeping the `latest_mutations` and `mutations_by_client_id` maps
    /// bounded. Pending mutations are never evicted.
    fn prune_settled_mutations(&mut self) {
        while self.settled_mutation_ids.len() > MAX_LATEST_MUTATIONS {
            let Some(oldest_id) = self.settled_mutation_ids.pop_front() else {
                break;
            };
            if let Some(oldest) = self.latest_mutations.remove(&oldest_id) {
                self.mutations_by_client_id
                    .remove(&oldest.client_mutation_id);
            }
        }
    }
}

impl SessionRegistry {
    fn lock_sessions(&self) -> MutexGuard<'_, HashMap<RuntimeSessionId, StoredSession>> {
        match self.sessions.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("session registry mutex was poisoned; recovering");
                poisoned.into_inner()
            }
        }
    }

    pub(crate) fn new(
        views: Arc<ViewRegistry>,
        event_sender: broadcast::Sender<DomainEvent>,
    ) -> Self {
        Self {
            views,
            event_sender,
            sessions: Mutex::new(HashMap::new()),
            next_mutation_id: AtomicU64::new(1),
        }
    }

    pub(crate) fn open_session(
        self: &Arc<Self>,
        caller: RuntimeCaller,
    ) -> Result<RuntimeSession, RuntimeError> {
        let session_id = RuntimeSessionId::new(format!("session-{}", Id::generate()));
        let (frames, _) = broadcast::channel(SESSION_FRAME_CHANNEL_CAPACITY);
        debug!(session_id = %session_id.as_str(), "runtime session opened");
        self.lock_sessions().insert(
            session_id.clone(),
            StoredSession {
                account_scope: caller.account_scope,
                delta_capable: caller.capabilities.view_delta,
                last_seq: 0,
                frames,
                open_views: HashSet::new(),
                latest_snapshots: HashMap::new(),
                latest_mutations: HashMap::new(),
                mutations_by_client_id: HashMap::new(),
                settled_mutation_ids: VecDeque::new(),
                undo_history: Vec::new(),
                redo_history: Vec::new(),
                event_task: None,
            },
        );
        let event_task = self.spawn_notification_forwarder(session_id.clone());
        if let Some(session) = self.lock_sessions().get_mut(&session_id) {
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
            let mut sessions = self.lock_sessions();
            let session = sessions
                .get_mut(&session_id)
                .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
            ensure_caller_matches_session(session, caller.account_scope.as_deref())?;
            let current_seq = RuntimeSessionSeq::new(session.last_seq);
            let needs_initial_frames = session.last_seq == 0
                && (!session.latest_snapshots.is_empty() || !session.latest_mutations.is_empty());
            let catch_up = if after_seq == Some(current_seq) && !needs_initial_frames {
                Vec::new()
            } else {
                collapse_session_frames(session)
            };
            (catch_up, session.frames.subscribe())
        };

        let registry = self.clone();
        let caller_scope = caller.account_scope;
        let stream = async_stream::stream! {
            loop {
                match receiver.recv().await {
                    Ok(frame) => yield frame,
                    Err(broadcast::error::RecvError::Lagged(missed)) => {
                        // A burst outran the consumer: `missed` frames were
                        // dropped from the channel. Recover by collapsing to the
                        // session's current state (idempotent for the client).
                        // A transient collapse failure must NOT kill the stream —
                        // keep looping so the next live frame still flows.
                        warn!(
                            session_id = %session_id.as_str(),
                            missed_frames = missed,
                            "session frame stream lagged; recovering with a collapsed snapshot",
                        );
                        match registry.collapse_session(&session_id, caller_scope.as_deref()) {
                            Ok(frames) => {
                                for frame in frames {
                                    yield frame;
                                }
                            }
                            Err(error) => {
                                warn!(
                                    session_id = %session_id.as_str(),
                                    %error,
                                    "failed to collapse session after lag; continuing the stream",
                                );
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!(session_id = %session_id.as_str(), "session frame stream closed");
                        break;
                    }
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
            let mut sessions = self.lock_sessions();
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

    /// Extend an open windowed view's window. The recompute broadcasts a
    /// `ViewReplace` through the view forwarder (which refreshes
    /// `latest_snapshots`); the grown snapshot is also returned for the request.
    ///
    /// @spec docs/runtime/adapter/L2#view-operation-flow
    pub(crate) async fn extend_view(
        self: &Arc<Self>,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        view_id: ViewId,
        count: usize,
    ) -> Result<ViewSnapshot, RuntimeError> {
        let session_scope = self.session_scope(&session_id, caller.account_scope.as_deref())?;
        {
            let sessions = self.lock_sessions();
            let session = sessions
                .get(&session_id)
                .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
            if !session.open_views.contains(&view_id) {
                return Err(RuntimeError::not_found("view is not open in this session"));
            }
        }
        self.views
            .extend_view(&view_id, count, session_scope.as_deref())
            .await
    }

    pub(crate) fn close_view(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        view_id: ViewId,
    ) -> Result<(), RuntimeError> {
        let mut sessions = self.lock_sessions();
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

    pub(crate) fn accept_mutation(
        &self,
        caller: RuntimeCaller,
        request: &MutationRequest,
    ) -> Result<MutationAcceptance, RuntimeError> {
        let session_id = request.session_id.as_ref().ok_or_else(|| {
            RuntimeError::invalid_mutation("runtime mutation requires a session id")
        })?;
        let mut sessions = self.lock_sessions();
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
        ensure_caller_matches_session(session, caller.account_scope.as_deref())?;
        if let Some(mutation_id) = session
            .mutations_by_client_id
            .get(&request.client_mutation_id)
        {
            let mutation = session.latest_mutations.get(mutation_id).ok_or_else(|| {
                RuntimeError::internal("runtime mutation index is inconsistent", None)
            })?;
            if mutation.name != request.name || mutation.args != request.args {
                return Err(RuntimeError::invalid_mutation(
                    "client mutation id was already used for a different mutation",
                ));
            }
            return Ok(MutationAcceptance::Existing(mutation.receipt()));
        }

        let mutation_id = RuntimeMutationId::new(format!(
            "mutation-{}",
            self.next_mutation_id.fetch_add(1, Ordering::Relaxed)
        ));
        let mutation = StoredMutation {
            mutation_id: mutation_id.clone(),
            client_mutation_id: request.client_mutation_id.clone(),
            name: request.name.clone(),
            args: request.args.clone(),
            state: MutationSettlementState::Accepted,
            error: None,
            output: Value::Null,
        };
        session
            .mutations_by_client_id
            .insert(request.client_mutation_id.clone(), mutation_id.clone());
        session
            .latest_mutations
            .insert(mutation_id.clone(), mutation.clone());
        let frame = mutation.frame(next_seq(session));
        let sender = session.frames.clone();
        drop(sessions);
        let _ = sender.send(frame);
        Ok(MutationAcceptance::New { mutation_id })
    }

    /// Record a reversible mutation's invertible diff onto the session's undo
    /// history (the step's seq is assigned here), clearing the redo history, and
    /// broadcast the new availability. Called after the backend confirms a
    /// diff-eligible message mutation; the before/after read produced `diff`.
    pub(crate) fn record_diff(
        &self,
        session_id: &RuntimeSessionId,
        source_id: String,
        message_id: String,
        diff: MessageChangeDiff,
    ) -> Result<(), RuntimeError> {
        let mut sessions = self.lock_sessions();
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
        let step = DiffStep {
            seq: next_seq(session),
            source_id,
            message_id,
            diff,
        };
        push_capped(&mut session.undo_history, step);
        session.redo_history.clear();
        let frame = history_frame(session);
        let sender = session.frames.clone();
        drop(sessions);
        let _ = sender.send(frame);
        Ok(())
    }

    /// Take the undo step with the given seq off the undo history so the caller
    /// can run its inverse (`message.applyDiff { undoOf }`). On success the
    /// caller pushes it to redo with [`push_redo`]; on failure it restores it with
    /// [`push_undo`]. `None` when no step matches (a stale `undoOf`: under
    /// last-writer-wins the applyDiff still runs, it just does not navigate).
    pub(crate) fn pop_undo_by_seq(
        &self,
        session_id: &RuntimeSessionId,
        seq: RuntimeSessionSeq,
    ) -> Result<Option<DiffStep>, RuntimeError> {
        let mut sessions = self.lock_sessions();
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
        Ok(remove_by_seq(&mut session.undo_history, seq))
    }

    /// Take the redo step with the given seq off the redo history so the caller
    /// can replay its forward diff (`message.applyDiff { redoOf }`).
    pub(crate) fn pop_redo_by_seq(
        &self,
        session_id: &RuntimeSessionId,
        seq: RuntimeSessionSeq,
    ) -> Result<Option<DiffStep>, RuntimeError> {
        let mut sessions = self.lock_sessions();
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
        Ok(remove_by_seq(&mut session.redo_history, seq))
    }

    /// Push a step onto the undo history (capped), without clearing redo. Used to
    /// commit a redo replay (the step returns to undo) or to roll back a failed
    /// undo (the step returns to where it was popped from).
    pub(crate) fn push_undo(
        &self,
        session_id: &RuntimeSessionId,
        step: DiffStep,
    ) -> Result<(), RuntimeError> {
        let mut sessions = self.lock_sessions();
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
        push_capped(&mut session.undo_history, step);
        Ok(())
    }

    /// Push a step onto the redo history (capped), without clearing it. Used to
    /// commit an undo (the undone step becomes redoable).
    pub(crate) fn push_redo(
        &self,
        session_id: &RuntimeSessionId,
        step: DiffStep,
    ) -> Result<(), RuntimeError> {
        let mut sessions = self.lock_sessions();
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
        push_capped(&mut session.redo_history, step);
        Ok(())
    }

    /// Broadcast the session's current undo/redo availability — `canUndo`/
    /// `canRedo` plus the current top of each stack as an invertible diff — so
    /// the renderer can drive button state and construct the undo/redo
    /// `message.applyDiff` mutation. Called whenever the histories change.
    ///
    /// @spec docs/runtime/mutations/L1#mutation-pipeline-and-catalog
    pub(crate) fn emit_history_frame(
        &self,
        session_id: &RuntimeSessionId,
    ) -> Result<(), RuntimeError> {
        let mut sessions = self.lock_sessions();
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
        let frame = history_frame(session);
        let sender = session.frames.clone();
        drop(sessions);
        let _ = sender.send(frame);
        Ok(())
    }

    pub(crate) fn settle_mutation(
        &self,
        session_id: &RuntimeSessionId,
        mutation_id: &RuntimeMutationId,
        state: MutationSettlementState,
        error: Option<RuntimeAdapterError>,
        output: Value,
    ) -> Result<MutationReceipt, RuntimeError> {
        let mut sessions = self.lock_sessions();
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
        let is_terminal = state.is_terminal();
        let (receipt, frame) = {
            let mutation = session
                .latest_mutations
                .get_mut(mutation_id)
                .ok_or_else(|| RuntimeError::not_found("runtime mutation not found"))?;
            mutation.state = state;
            mutation.error = error;
            mutation.output = output;
            let mutation = mutation.clone();
            let frame = mutation.frame(next_seq(session));
            (mutation.receipt(), frame)
        };
        if is_terminal {
            session.settled_mutation_ids.push_back(mutation_id.clone());
            session.prune_settled_mutations();
        }
        let sender = session.frames.clone();
        drop(sessions);
        let _ = sender.send(frame);
        Ok(receipt)
    }

    pub(crate) fn session_scope(
        &self,
        session_id: &RuntimeSessionId,
        caller_scope: Option<&[String]>,
    ) -> Result<Option<Vec<String>>, RuntimeError> {
        let sessions = self.lock_sessions();
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
        let mut sessions = self.lock_sessions();
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
        let mut sessions = self.lock_sessions();
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
        let mut sessions = self.lock_sessions();
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
            kind: event.topic,
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
        let mut sessions = self.lock_sessions();
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| RuntimeError::not_found("runtime session not found"))?;
        ensure_caller_matches_session(session, caller_scope)?;
        Ok(collapse_session_frames(session))
    }
}

fn collapse_session_frames(session: &mut StoredSession) -> Vec<RuntimeFrame> {
    let mut frames = Vec::new();
    let mut snapshots: Vec<_> = session.latest_snapshots.values().cloned().collect();
    snapshots.sort_by(|left, right| left.view_id.as_str().cmp(right.view_id.as_str()));
    for snapshot in snapshots {
        let session_seq = next_seq(session);
        frames.push(RuntimeFrame::ViewSnapshot {
            session_seq,
            view_id: snapshot.view_id.clone(),
            revision: snapshot.revision,
            snapshot,
        });
    }
    let mut mutations: Vec<_> = session.latest_mutations.values().cloned().collect();
    mutations.sort_by(|left, right| left.mutation_id.as_str().cmp(right.mutation_id.as_str()));
    for mutation in mutations {
        let session_seq = next_seq(session);
        frames.push(mutation.frame(session_seq));
    }
    frames
}

/// Compute a row-local mail-list delta between two snapshots, or `None` when the
/// change is not row-local — a structural change (scope/sort/window/continuation)
/// or a non-mail-list view — in which case the caller re-serves a whole
/// `ViewReplace` (L6). Rows are keyed by `row_key`; `order` is sent only when the
/// id sequence changed; `upserts` carry only new or content-changed rows.
fn mail_list_delta(old: &ViewSnapshot, new: &ViewSnapshot) -> Option<MailListDelta> {
    let old_state: MailListViewState = serde_json::from_value(old.data.clone()).ok()?;
    let new_state: MailListViewState = serde_json::from_value(new.data.clone()).ok()?;
    if old_state.scope != new_state.scope
        || old_state.projection_kind != new_state.projection_kind
        || old_state.sort != new_state.sort
        || old_state.window_request != new_state.window_request
        || old_state.continuation != new_state.continuation
    {
        return None;
    }
    let old_by_key: HashMap<&str, &MailListRowState> = old_state
        .rows
        .iter()
        .map(|row| (row.row_key.as_str(), row))
        .collect();
    let old_order: Vec<&str> = old_state
        .rows
        .iter()
        .map(|row| row.row_key.as_str())
        .collect();
    let new_order: Vec<&str> = new_state
        .rows
        .iter()
        .map(|row| row.row_key.as_str())
        .collect();
    let order =
        (old_order != new_order).then(|| new_order.iter().map(|key| key.to_string()).collect());
    let mut upserts = Vec::new();
    for new_row in &new_state.rows {
        let changed = match old_by_key.get(new_row.row_key.as_str()) {
            Some(old_row) => *old_row != new_row,
            None => true,
        };
        if changed {
            upserts.push(new_row.clone());
        }
    }
    Some(MailListDelta { order, upserts })
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
            let previous = session
                .latest_snapshots
                .insert(view_id.clone(), snapshot.clone());
            // A delta-capable session receives only the rows that changed, when
            // the change is row-local; structural changes (and non-mail-list
            // views) fall back to a whole replace (L6).
            if session.delta_capable {
                if let Some(delta) = previous
                    .as_ref()
                    .and_then(|old| mail_list_delta(old, &snapshot))
                {
                    return Some(RuntimeFrame::ViewDelta {
                        session_seq,
                        view_id,
                        revision,
                        delta,
                    });
                }
            }
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

/// Push onto a history stack, evicting the oldest entry once `MAX_HISTORY` is
/// exceeded so a session's undo/redo state stays bounded.
fn push_capped<T>(stack: &mut Vec<T>, entry: T) {
    stack.push(entry);
    if stack.len() > MAX_HISTORY {
        stack.remove(0);
    }
}

/// Build the `MutationHistory` frame for a session's current undo/redo state:
/// availability flags plus the top diff of each stack. The client needs only the
/// current top of each stack to construct the undo/redo `message.applyDiff`
/// mutation (it never needs the full history).
fn history_frame(session: &mut StoredSession) -> RuntimeFrame {
    let undo_top = session.undo_history.last().cloned();
    let redo_top = session.redo_history.last().cloned();
    RuntimeFrame::MutationHistory {
        session_seq: next_seq(session),
        can_undo: undo_top.is_some(),
        can_redo: redo_top.is_some(),
        undo_top,
        redo_top,
    }
}

/// Remove the step with the given seq from a history, searching from the most
/// recent (the top), preserving the order of the remaining steps.
fn remove_by_seq(history: &mut Vec<DiffStep>, seq: RuntimeSessionSeq) -> Option<DiffStep> {
    let position = history.iter().rposition(|step| step.seq == seq)?;
    Some(history.remove(position))
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

#[cfg(test)]
mod delta_tests {
    use super::*;
    use posthaste_runtime_contract::{
        RuntimeCoverage, RuntimeCoverageKind, ViewLifecycle, ViewRevision,
    };
    use serde_json::json;

    fn row(key: &str, flagged: bool) -> Value {
        json!({
            "rowKey": key,
            "resourceRef": null,
            "projection": { "id": key, "isFlagged": flagged },
            "sortKey": {},
            "orderKey": key,
            "pendingMarkers": []
        })
    }

    fn snapshot(rows: Vec<Value>, has_after: bool) -> ViewSnapshot {
        ViewSnapshot {
            view_id: ViewId::new("v1"),
            descriptor: ViewDescriptor {
                family: "mailList".into(),
                payload: Value::Null,
            },
            revision: ViewRevision::new(1),
            lifecycle: ViewLifecycle::Ready,
            read_watermark: None,
            coverage: RuntimeCoverage {
                kind: RuntimeCoverageKind::Complete,
                details: Value::Null,
            },
            data: json!({
                "scope": {},
                "projectionKind": "message",
                "sort": {},
                "windowRequest": {},
                "rows": rows,
                "continuation": {
                    "beforeCursor": null, "afterCursor": null,
                    "hasBefore": false, "hasAfter": has_after
                },
                "readWatermark": null,
                "coverage": { "kind": "complete", "details": {} },
                "knownTotalCount": null,
                "pendingMutations": [],
                "anchor": { "kind": "notRequested" }
            }),
            pending_mutations: vec![],
            error: None,
        }
    }

    #[test]
    fn flag_toggle_is_a_single_upsert_no_reorder() {
        let old = snapshot(vec![row("m1", false), row("m2", false)], false);
        let new = snapshot(vec![row("m1", true), row("m2", false)], false);
        let delta = mail_list_delta(&old, &new).expect("row-local change yields a delta");
        assert!(delta.order.is_none(), "order unchanged");
        assert_eq!(delta.upserts.len(), 1, "only the changed row");
        assert_eq!(delta.upserts[0].row_key, "m1");
    }

    #[test]
    fn removal_sends_the_new_order_and_no_upserts() {
        let old = snapshot(vec![row("m1", false), row("m2", false)], false);
        let new = snapshot(vec![row("m2", false)], false);
        let delta = mail_list_delta(&old, &new).expect("removal yields a delta");
        assert_eq!(delta.order.as_deref(), Some(["m2".to_string()].as_slice()));
        assert!(delta.upserts.is_empty(), "the surviving row is unchanged");
    }

    #[test]
    fn structural_change_falls_back_to_a_whole_replace() {
        let old = snapshot(vec![row("m1", false)], false);
        // Same rows, but the continuation (hasAfter) changed — a structural change.
        let new = snapshot(vec![row("m1", false)], true);
        assert!(
            mail_list_delta(&old, &new).is_none(),
            "a non-row-local change must re-serve the whole view"
        );
    }
}

#[cfg(test)]
mod mutation_eviction_tests {
    use super::*;
    use serde_json::json;

    fn stored_mutation(id: u64, client_id: u64, state: MutationSettlementState) -> StoredMutation {
        StoredMutation {
            mutation_id: RuntimeMutationId::new(format!("mutation-{id}")),
            client_mutation_id: ClientMutationId::new(format!("client-{client_id}")),
            name: "message.setFlaggedState".to_string(),
            args: json!({ "flagged": true }),
            state,
            error: None,
            output: Value::Null,
        }
    }

    fn empty_session() -> StoredSession {
        let (frames, _) = broadcast::channel(1);
        StoredSession {
            account_scope: None,
            delta_capable: false,
            last_seq: 0,
            frames,
            open_views: HashSet::new(),
            latest_snapshots: HashMap::new(),
            latest_mutations: HashMap::new(),
            mutations_by_client_id: HashMap::new(),
            settled_mutation_ids: VecDeque::new(),
            undo_history: Vec::new(),
            redo_history: Vec::new(),
            event_task: None,
        }
    }

    #[test]
    fn prune_evicts_oldest_terminal_mutations_once_window_is_full() {
        let mut session = empty_session();
        let cap = MAX_LATEST_MUTATIONS;

        for id in 1..=cap + 5 {
            let m = stored_mutation(id as u64, id as u64, MutationSettlementState::Confirmed);
            session
                .latest_mutations
                .insert(m.mutation_id.clone(), m.clone());
            session
                .mutations_by_client_id
                .insert(m.client_mutation_id.clone(), m.mutation_id.clone());
            session
                .settled_mutation_ids
                .push_back(m.mutation_id.clone());
            session.prune_settled_mutations();
            assert!(
                session.latest_mutations.len() <= cap,
                "latest_mutations never exceeds the cap after pruning"
            );
        }

        assert_eq!(
            session.latest_mutations.len(),
            cap,
            "the most recent terminal mutations are retained"
        );
        assert_eq!(
            session.settled_mutation_ids.len(),
            cap,
            "settled_mutation_ids tracks only the live window"
        );

        for id in 1..=5 {
            let evicted_id = RuntimeMutationId::new(format!("mutation-{id}"));
            let evicted_client = ClientMutationId::new(format!("client-{id}"));
            assert!(
                !session.latest_mutations.contains_key(&evicted_id),
                "oldest terminal mutation {id} was evicted"
            );
            assert!(
                !session.mutations_by_client_id.contains_key(&evicted_client),
                "client id of evicted mutation {id} was also removed"
            );
        }

        for id in 6..=cap + 5 {
            let retained_id = RuntimeMutationId::new(format!("mutation-{id}"));
            assert!(
                session.latest_mutations.contains_key(&retained_id),
                "recent terminal mutation {id} is still retained"
            );
        }
    }

    #[test]
    fn prune_never_evicts_pending_mutations() {
        let mut session = empty_session();
        let cap = MAX_LATEST_MUTATIONS;

        // Seed many pending mutations beyond the cap.
        for id in 1..=cap + 10 {
            let m = stored_mutation(id as u64, id as u64, MutationSettlementState::Accepted);
            session
                .latest_mutations
                .insert(m.mutation_id.clone(), m.clone());
            session
                .mutations_by_client_id
                .insert(m.client_mutation_id.clone(), m.mutation_id.clone());
        }

        session.prune_settled_mutations();

        assert_eq!(
            session.latest_mutations.len(),
            cap + 10,
            "pending mutations are left intact when there are no terminal mutations"
        );

        // Settle only the oldest pending mutation; it should become eligible for
        // eviction only after enough newer mutations are also settled.
        let first_id = RuntimeMutationId::new("mutation-1");
        let first_client = ClientMutationId::new("client-1");
        session.settled_mutation_ids.push_back(first_id.clone());
        session.prune_settled_mutations();

        assert!(
            session.latest_mutations.contains_key(&first_id),
            "single settled mutation is not evicted while the window is not full"
        );
        assert!(
            session.mutations_by_client_id.contains_key(&first_client),
            "client id is not removed until the mutation leaves the window"
        );
    }

    #[test]
    fn collapse_session_frames_emits_at_most_the_live_mutation_window() {
        let mut session = empty_session();
        let cap = MAX_LATEST_MUTATIONS;

        for id in 1..=cap + 50 {
            let m = stored_mutation(id as u64, id as u64, MutationSettlementState::Confirmed);
            session
                .latest_mutations
                .insert(m.mutation_id.clone(), m.clone());
            session
                .mutations_by_client_id
                .insert(m.client_mutation_id.clone(), m.mutation_id.clone());
            session
                .settled_mutation_ids
                .push_back(m.mutation_id.clone());
            session.prune_settled_mutations();
        }

        let frames = collapse_session_frames(&mut session);
        let mutation_frames = frames
            .iter()
            .filter(|f| matches!(f, RuntimeFrame::MutationSettlement { .. }))
            .count();
        assert_eq!(
            mutation_frames, cap,
            "reconnect re-emits at most MAX_LATEST_MUTATIONS settlement frames"
        );
    }
}
