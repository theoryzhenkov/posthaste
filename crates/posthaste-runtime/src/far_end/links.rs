use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use posthaste_client_link::{RuntimeFrameSubscription, RuntimeViewSubscription};
use posthaste_contract_core::{
    ClientMutationId, MailListDelta, MailListRowState, MailListViewState, MailOperation,
    MutationNotification, MutationReceipt, MutationRequest, MutationSettlementState,
    RuntimeAdapterError, RuntimeCaller, RuntimeError, RuntimeFrame, RuntimeLinkConnection,
    RuntimeLinkId, RuntimeLinkSeq, RuntimeMutationId, ViewDescriptor, ViewFrame, ViewId,
    ViewSnapshot,
};
use posthaste_domain_model::{
    DomainEvent, Id, OperationDispatchUncertain, OperationId, OperationOutcome,
    OperationSettlement, EVENT_TOPIC_OPERATION_DISPATCH_UNCERTAIN, EVENT_TOPIC_OPERATION_SETTLED,
};
use posthaste_link_far_end::down::{ReplayStore, Resume};
use posthaste_link_far_end::up::{Accept, DedupStore, TerminalClass};
use serde_json::Value;
use tokio::sync::broadcast;
use tokio::task::AbortHandle;
use tracing::{debug, warn};

use crate::far_end::view_registry::ViewRegistry;

/// Capacity of the per-link frame broadcast channel. A burst (e.g. a sync
/// delivering many messages at once) emits one view frame per recompute; if the
/// SSE consumer can't drain them before the channel fills, `recv` returns
/// `Lagged` and we recover by collapsing to current state. Sized generously so
/// ordinary bursts never lag; the collapse path remains the safety net.
const LINK_FRAME_CHANNEL_CAPACITY: usize = 512;

/// Idle-session reaper TTL (RFC-L2-lifecycle D68 / M28), in `now_secs` ticks
/// (**seconds**). A link-connection whose SSE down-stream has been gone this
/// long — the leak shape `open → stream → disconnect` with no explicit `DELETE`
/// (`close_link`) — is reaped: its registry entry, open views, and dedup ledger
/// are released. Also age-reaps a link opened but never streamed. Reuses the D48
/// tick discipline (the same `now_secs` tick the dedup reaper runs on), never a
/// new timer (D68). Sized to comfortably outlast an ordinary client reconnect
/// gap while bounding leaked-session growth (cures N9). Flagged for review.
const SESSION_IDLE_TTL: u64 = 300;

pub(crate) struct LinkRegistry {
    views: Arc<ViewRegistry>,
    event_sender: broadcast::Sender<DomainEvent>,
    links: Mutex<HashMap<RuntimeLinkId, StoredLink>>,
    /// The shared idempotency-dedup sub-store (RFC D45/D47), keyed
    /// `(RuntimeLinkId, ClientMutationId)` — the client↔runtime seam's
    /// assembly of the far-end engine. Replaces the former per-link
    /// `latest_mutations` / `mutations_by_client_id` / `settled_mutation_ids`
    /// hand-roll; a client "link" IS this seam's `LinkId` (D42).
    dedup: DedupStore<RuntimeLinkId, StoredMutation>,
    /// The shared seq mechanism (D50): the per-link monotonic frame seq counter
    /// + reconnect (collapse) detection, in **collapse-always** mode — the runtime
    /// far-end's collapse re-serves whole snapshots, so a per-frame replay backlog
    /// buys nothing; the store owns the counter and the resume→collapse decision.
    /// Replaces the former hand-rolled `StoredLink.last_seq` / `next_seq`.
    seq: ReplayStore<RuntimeLinkId, ()>,
    next_mutation_id: AtomicU64,
    /// The send-bridge near-node half: outbox operation id → the client link +
    /// runtime mutation id of a DEFERRED async-settled mutation (a Send) whose
    /// verdict is held past the authority receipt (`Accepted`). Populated when the
    /// up-channel accepts the send; drained + settled (→ the terminal
    /// `mutation.notification` the client's fold consumes) when the co-located
    /// settlement bridge sees the flush's `operation.settled`/`dispatch_uncertain`.
    /// Co-located delivery only — a remote near node settles from the routed
    /// down-channel `Settlement` frame instead.
    deferred_settlements: Mutex<HashMap<OperationId, (RuntimeLinkId, RuntimeMutationId)>>,
    /// Test-only barrier fired inside `subscribe_frames`, exactly between the live
    /// `frames.subscribe()` and the catch-up snapshot — lets a test deterministically
    /// interpose a `settle` in that window to pin the [2] ordering invariant.
    #[cfg(test)]
    subscribe_barrier: Mutex<Option<Box<dyn FnMut() + Send>>>,
    /// Test-only barrier fired inside `accept_mutation`, exactly between the dedup
    /// insert and the link revalidation — lets a test deterministically
    /// interpose a `close_link` there to pin the [4] self-sweep invariant.
    #[cfg(test)]
    accept_barrier: Mutex<Option<Box<dyn FnMut() + Send>>>,
}

struct StoredLink {
    account_scope: Option<Vec<String>>,
    /// The link opted into incremental mail-list deltas ([`RuntimeFrame::ViewDelta`])
    /// instead of whole-view replaces (L6).
    delta_capable: bool,
    frames: broadcast::Sender<RuntimeFrame>,
    open_views: HashSet<ViewId>,
    latest_snapshots: HashMap<ViewId, ViewSnapshot>,
    event_task: Option<AbortHandle>,
    /// The last `now_secs` tick this link was opened or (re)subscribed, refreshed
    /// while a live SSE down-stream holds it (M28/D68). A link with no live
    /// down-stream (`frames.receiver_count() == 0`) idle past [`SESSION_IDLE_TTL`]
    /// since this tick is reaped by [`LinkRegistry::reap_idle_sessions`].
    last_active: u64,
}

#[derive(Clone)]
struct StoredMutation {
    mutation_id: RuntimeMutationId,
    client_mutation_id: ClientMutationId,
    /// The typed operation the client forwarded. Re-accept idempotency compares
    /// this by value (D8 — `PartialEq` on the typed op, not a `name`+`args`
    /// string/JSON compare); the receipt's echoed name derives from it.
    operation: MailOperation,
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
            name: self.operation.name().to_string(),
            state: self.state.clone(),
            error: self.error.clone(),
            output: self.output.clone(),
        }
    }

    /// The terminal verdict to publish for this mutation's current state, or
    /// `None` while it is still non-terminal (Accepted) — that ack is not
    /// emitted (`mutation.notification` carries only terminal outcomes; the
    /// client tracks the in-flight op locally). `Confirmed` maps through;
    /// `Failed` collapses into `Rejected`, the distinction carried by the error
    /// code.
    fn notification(&self) -> Option<MutationNotification> {
        match self.state {
            MutationSettlementState::Confirmed => Some(MutationNotification::Confirmed),
            MutationSettlementState::Failed => Some(MutationNotification::Rejected {
                error: self.error.clone().unwrap_or_else(|| {
                    RuntimeError::internal("mutation rejected without an error", None)
                        .envelope()
                        .clone()
                }),
            }),
            MutationSettlementState::Accepted => None,
        }
    }

    fn notification_frame(&self, link_seq: RuntimeLinkSeq) -> Option<RuntimeFrame> {
        self.notification()
            .map(|notification| RuntimeFrame::MutationNotification {
                link_seq,
                client_mutation_id: self.client_mutation_id.clone(),
                notification,
            })
    }
}

impl LinkRegistry {
    fn lock_links(&self) -> MutexGuard<'_, HashMap<RuntimeLinkId, StoredLink>> {
        match self.links.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!("link registry mutex was poisoned; recovering");
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
            links: Mutex::new(HashMap::new()),
            dedup: DedupStore::new(),
            seq: ReplayStore::collapse_always(),
            next_mutation_id: AtomicU64::new(1),
            deferred_settlements: Mutex::new(HashMap::new()),
            #[cfg(test)]
            subscribe_barrier: Mutex::new(None),
            #[cfg(test)]
            accept_barrier: Mutex::new(None),
        }
    }

    /// Fire a test barrier (no-op in production builds).
    #[cfg(test)]
    fn fire_barrier(&self, barrier: &Mutex<Option<Box<dyn FnMut() + Send>>>) {
        let hook = barrier.lock().unwrap().take();
        if let Some(mut hook) = hook {
            hook();
        }
    }

    /// Stamp the next monotonic per-link frame seq from the shared store (D50)
    /// and wrap it as the wire's [`RuntimeLinkSeq`] (this seam keeps the seq
    /// inside `RuntimeFrame` for client-visible wire compatibility — the store owns
    /// the counter, the frame carries it).
    fn stamp(&self, link_id: &RuntimeLinkId) -> RuntimeLinkSeq {
        RuntimeLinkSeq::new(self.seq.stamp(link_id))
    }

    pub(crate) fn open_link(
        self: &Arc<Self>,
        caller: RuntimeCaller,
    ) -> Result<RuntimeLinkConnection, RuntimeError> {
        let link_id = RuntimeLinkId::new(format!("link-{}", Id::generate()));
        let (frames, _) = broadcast::channel(LINK_FRAME_CHANNEL_CAPACITY);
        debug!(link_id = %link_id.as_str(), "runtime link opened");
        self.lock_links().insert(
            link_id.clone(),
            StoredLink {
                account_scope: caller.account_scope,
                delta_capable: caller.capabilities.view_delta,
                frames,
                open_views: HashSet::new(),
                latest_snapshots: HashMap::new(),
                event_task: None,
                last_active: now_secs(),
            },
        );
        let event_task = self.spawn_notification_forwarder(link_id.clone());
        if let Some(link) = self.lock_links().get_mut(&link_id) {
            link.event_task = Some(event_task);
        }
        Ok(RuntimeLinkConnection { link_id })
    }

    pub(crate) async fn subscribe_frames(
        self: &Arc<Self>,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
        after_seq: Option<RuntimeLinkSeq>,
    ) -> Result<RuntimeFrameSubscription, RuntimeError> {
        {
            let links = self.lock_links();
            let link = links
                .get(&link_id)
                .ok_or_else(|| RuntimeError::not_found("runtime link not found"))?;
            ensure_caller_matches_link(link, caller.account_scope.as_deref())?;
        }

        // The resume cursor IS the ack signal (D48): a resume from `after_seq`
        // means the client has seen every frame up to it, so terminal dedup
        // records whose settlement frame it has passed are reclaimed. Also drive
        // the TTL fallback opportunistically (D48 (b)).
        let now = now_secs();
        if let Some(seq) = after_seq {
            self.dedup.ack(&link_id, seq.get());
        }
        self.dedup.reap(now);
        // M28/D68 idle-session reaper, on the same D48 tick (reuse the plumbing,
        // no new timer). Mark this link active FIRST so a stale reap cannot evict
        // the very link we are (re)subscribing to before its new down-stream
        // attaches below.
        self.mark_link_active(&link_id, now);
        self.reap_idle_sessions(now);

        // Reconnect detection (D50): the shared store's resume decides. `Fresh`
        // (no cursor) or `Replay` at head means the client is current — no
        // catch-up needed unless we have never-delivered initial state. A stale
        // cursor (`Collapse`, in collapse-always mode) is a reconnect: re-derive
        // open views before collapsing so we serve current rows, not stale ones
        // (the per-event mail-list re-serve was retired, option iii).
        let resume = self.seq.resume(&link_id, after_seq.map(|s| s.get()));
        let is_reconnect = matches!(resume, Resume::Collapse);
        if is_reconnect {
            self.refresh_open_views(&link_id).await;
        }

        // The dedup ledger's mutation records for this link — the live
        // mutation window replayed on collapse (fetched outside the link lock).
        let mutations = self.dedup.records_for(&link_id);

        // Ordering invariant [2] — subscribe THEN snapshot: subscribe to the live
        // broadcast *before* taking the catch-up snapshot, so a settle landing in
        // the window cannot be lost (missed by a snapshot taken before it and by a
        // stream subscribed after it). The dual (a settle in both the snapshot and
        // the live stream) is a harmless duplicate — terminal notifications are
        // idempotent for the client.
        let mut receiver = {
            let links = self.lock_links();
            let link = links
                .get(&link_id)
                .ok_or_else(|| RuntimeError::not_found("runtime link not found"))?;
            link.frames.subscribe()
        };
        // [2] race window: a settle landing here must reach the live stream (we
        // already subscribed) rather than being lost.
        #[cfg(test)]
        self.fire_barrier(&self.subscribe_barrier);
        let catch_up = {
            let mut links = self.lock_links();
            let link = links
                .get_mut(&link_id)
                .ok_or_else(|| RuntimeError::not_found("runtime link not found"))?;
            let needs_initial_frames = !link.latest_snapshots.is_empty() || !mutations.is_empty();
            match resume {
                // Current (fresh or at-head): re-serve initial state on a first
                // subscribe, else nothing to catch up.
                Resume::Fresh | Resume::Replay(_) if !needs_initial_frames => Vec::new(),
                _ => {
                    let sid = link_id.clone();
                    let mut next = || RuntimeLinkSeq::new(self.seq.stamp(&sid));
                    collapse_link_frames(link, &mutations, &mut next)
                }
            }
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
                        // link's current state (idempotent for the client).
                        // A transient collapse failure must NOT kill the stream —
                        // keep looping so the next live frame still flows.
                        warn!(
                            link_id = %link_id.as_str(),
                            missed_frames = missed,
                            "link frame stream lagged; recovering with a collapsed snapshot",
                        );
                        registry.refresh_open_views(&link_id).await;
                        match registry.collapse_link(&link_id, caller_scope.as_deref()) {
                            Ok(frames) => {
                                for frame in frames {
                                    yield frame;
                                }
                            }
                            Err(error) => {
                                warn!(
                                    link_id = %link_id.as_str(),
                                    %error,
                                    "failed to collapse link after lag; continuing the stream",
                                );
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!(link_id = %link_id.as_str(), "link frame stream closed");
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

    pub(crate) fn close_link(
        &self,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
    ) -> Result<(), RuntimeError> {
        let (open_views, event_task) = {
            let mut links = self.lock_links();
            let link = links
                .get(&link_id)
                .ok_or_else(|| RuntimeError::not_found("runtime link not found"))?;
            ensure_caller_matches_link(link, caller.account_scope.as_deref())?;
            let link = links
                .remove(&link_id)
                .ok_or_else(|| RuntimeError::not_found("runtime link not found"))?;
            (link.open_views, link.event_task)
        };
        if let Some(event_task) = event_task {
            event_task.abort();
        }
        for view_id in open_views {
            let _ = self.views.close_view(&view_id);
        }
        // Drop the link's dedup ledger entries (its link closed).
        self.dedup.purge(&link_id);
        Ok(())
    }

    /// Refresh a link's idle-reaper activity tick (M28/D68) — called on every
    /// (re)subscribe so a link with an active down-stream is never reaped.
    fn mark_link_active(&self, link_id: &RuntimeLinkId, now: u64) {
        if let Some(link) = self.lock_links().get_mut(link_id) {
            link.last_active = now;
        }
    }

    /// The idle-session reaper (RFC-L2-lifecycle M28/D68): release link-
    /// connections whose SSE down-stream is gone — the `open → stream →
    /// disconnect`-without-`DELETE` leak (N9) — plus links opened but never
    /// streamed that have aged out. A link with a live down-stream
    /// (`frames.receiver_count() > 0`) is spared and its activity tick refreshed;
    /// otherwise, if idle past [`SESSION_IDLE_TTL`] since its last activity, it is
    /// torn down exactly as [`close_link`](Self::close_link) would (abort the
    /// event task, close open views, purge the dedup ledger + seq counter). Driven
    /// by the explicit `now` tick — the same D48 discipline as the dedup/sink
    /// reapers, never ambient time. Returns the reaped ids.
    pub(crate) fn reap_idle_sessions(&self, now: u64) -> Vec<RuntimeLinkId> {
        let ttl = SESSION_IDLE_TTL;
        // Collect teardown work under the links lock, then perform it (which takes
        // other locks: view registry, dedup, seq) after releasing the links lock.
        let mut teardown: Vec<(RuntimeLinkId, HashSet<ViewId>, Option<AbortHandle>)> = Vec::new();
        {
            let mut links = self.lock_links();
            links.retain(|link_id, link| {
                if link.frames.receiver_count() > 0 {
                    // A live SSE down-stream holds the link — never idle; refresh.
                    link.last_active = now;
                    true
                } else if now.saturating_sub(link.last_active) > ttl {
                    teardown.push((
                        link_id.clone(),
                        std::mem::take(&mut link.open_views),
                        link.event_task.take(),
                    ));
                    false
                } else {
                    true
                }
            });
        }
        let mut reaped = Vec::with_capacity(teardown.len());
        for (link_id, open_views, event_task) in teardown {
            if let Some(event_task) = event_task {
                event_task.abort();
            }
            for view_id in open_views {
                let _ = self.views.close_view(&view_id);
            }
            self.dedup.purge(&link_id);
            self.seq.purge(&link_id);
            debug!(link_id = %link_id.as_str(), "idle runtime link reaped (M28/D68)");
            reaped.push(link_id);
        }
        reaped
    }

    pub(crate) async fn open_view(
        self: &Arc<Self>,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
        descriptor: ViewDescriptor,
    ) -> Result<ViewSnapshot, RuntimeError> {
        let link_scope = self.link_scope(&link_id, caller.account_scope.as_deref())?;
        let snapshot = self
            .views
            .open_view(descriptor, link_scope.as_deref())
            .await?;
        let subscription = self.views.subscribe_view(
            snapshot.view_id.clone(),
            Some(snapshot.revision),
            link_scope.as_deref(),
        )?;
        self.record_open_view(&link_id, snapshot.clone())?;
        self.spawn_view_forwarder(link_id, subscription);
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
        link_id: RuntimeLinkId,
        view_id: ViewId,
        count: usize,
    ) -> Result<ViewSnapshot, RuntimeError> {
        let link_scope = self.link_scope(&link_id, caller.account_scope.as_deref())?;
        {
            let links = self.lock_links();
            let link = links
                .get(&link_id)
                .ok_or_else(|| RuntimeError::not_found("runtime link not found"))?;
            if !link.open_views.contains(&view_id) {
                return Err(RuntimeError::not_found("view is not open in this link"));
            }
        }
        self.views
            .extend_view(&view_id, count, link_scope.as_deref())
            .await
    }

    pub(crate) fn close_view(
        &self,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
        view_id: ViewId,
    ) -> Result<(), RuntimeError> {
        let mut links = self.lock_links();
        let link = links
            .get_mut(&link_id)
            .ok_or_else(|| RuntimeError::not_found("runtime link not found"))?;
        ensure_caller_matches_link(link, caller.account_scope.as_deref())?;
        link.open_views.remove(&view_id);
        link.latest_snapshots.remove(&view_id);
        let sender = link.frames.clone();
        drop(links);
        let seq = self.stamp(&link_id);
        let _ = self.views.close_view(&view_id);
        let _ = sender.send(RuntimeFrame::ViewClosed {
            link_seq: seq,
            view_id,
        });
        Ok(())
    }

    pub(crate) fn accept_mutation(
        &self,
        caller: RuntimeCaller,
        request: &MutationRequest,
    ) -> Result<MutationAcceptance, RuntimeError> {
        let link_id = request
            .link_id
            .as_ref()
            .ok_or_else(|| RuntimeError::invalid_mutation("runtime mutation requires a link id"))?;
        {
            // Caller-scope (auth) check up front; the dedup ledger is the shared
            // sub-store below (released before it is touched).
            let links = self.lock_links();
            let link = links
                .get(link_id)
                .ok_or_else(|| RuntimeError::not_found("runtime link not found"))?;
            ensure_caller_matches_link(link, caller.account_scope.as_deref())?;
        }
        // Mint the id up front so the New record carries it; on a Duplicate the
        // minted id is simply discarded (harmless).
        let mutation_id = RuntimeMutationId::new(format!(
            "mutation-{}",
            self.next_mutation_id.fetch_add(1, Ordering::Relaxed)
        ));
        let record = StoredMutation {
            mutation_id: mutation_id.clone(),
            client_mutation_id: request.client_mutation_id.clone(),
            operation: request.operation.clone(),
            state: MutationSettlementState::Accepted,
            error: None,
            output: Value::Null,
        };
        // Insert THEN revalidate ([4]): a `close_link` racing between the
        // scope check above and this insert purges the link's ledger; if we
        // inserted after that purge, the Pending record would orphan (never
        // evicted → unbounded leak). Revalidate the link still exists; if it
        // vanished, self-sweep our just-inserted record and report not_found.
        let acceptance = self
            .dedup
            .accept(link_id, &request.client_mutation_id, || record);
        // [4] race window: a close_link landing here purges the link; the
        // revalidation below must self-sweep our just-inserted record.
        #[cfg(test)]
        self.fire_barrier(&self.accept_barrier);
        if !self.lock_links().contains_key(link_id) {
            self.dedup.clear(link_id, &request.client_mutation_id);
            return Err(RuntimeError::not_found("runtime link not found"));
        }
        match acceptance {
            Accept::Duplicate(existing) => {
                if existing.operation != request.operation {
                    return Err(RuntimeError::invalid_mutation(
                        "client mutation id was already used for a different mutation",
                    ));
                }
                Ok(MutationAcceptance::Existing(existing.receipt()))
            }
            // No frame on accept: `mutation.notification` carries only terminal
            // verdicts, and the client already tracks the in-flight op in its own
            // pending set the moment it dispatches it.
            Accept::New => Ok(MutationAcceptance::New { mutation_id }),
        }
    }

    pub(crate) fn settle_mutation(
        &self,
        link_id: &RuntimeLinkId,
        mutation_id: &RuntimeMutationId,
        state: MutationSettlementState,
        error: Option<RuntimeAdapterError>,
        output: Value,
    ) -> Result<MutationReceipt, RuntimeError> {
        // The dedup ledger is keyed by `ClientMutationId`; settlement addresses
        // the runtime's own `RuntimeMutationId`, so resolve the client id from
        // the pending record (≤100 per link).
        let base = self
            .dedup
            .records_for(link_id)
            .into_iter()
            .find(|record| &record.mutation_id == mutation_id)
            .ok_or_else(|| RuntimeError::not_found("runtime mutation not found"))?;
        let client_mutation_id = base.client_mutation_id.clone();

        // Derive the receipt + terminal notification frame from the settled
        // record; the frame is emitted even for a cleared (Failed) verdict.
        let mut settled = base;
        settled.state = state.clone();
        settled.error = error.clone();
        settled.output = output.clone();
        let receipt = settled.receipt();
        let class = terminal_class_for(&state, error.as_ref());

        // Stamp the terminal-notification seq (the D48 ack target) from the shared
        // store — no link lock needed; the store owns the counter.
        let (settlement_seq, frame) = match settled.notification() {
            Some(notification) => {
                let link_seq = self.stamp(link_id);
                (
                    Some(link_seq.get()),
                    Some(RuntimeFrame::MutationNotification {
                        link_seq,
                        client_mutation_id: client_mutation_id.clone(),
                        notification,
                    }),
                )
            }
            None => (None, None),
        };

        // Ordering [2b]: ledger-settle BEFORE broadcast, so the ledger is
        // consistent before the notification is visible to any (re)subscriber.
        // D47 keep/clear: `Confirmed` and a permanent rejection are kept (D48
        // retention — evicted on ack/TTL); a transient (retryable) `Failed` is
        // cleared so a deliberate retry re-accepts and re-executes. `settlement_seq`
        // is the seq of the emitted notification, the ack target D48 evicts on.
        self.dedup.settle(
            link_id,
            &client_mutation_id,
            class,
            settlement_seq,
            now_secs(),
            |record| {
                record.state = state;
                record.error = error;
                record.output = output;
            },
        );

        // THEN broadcast onto the live stream, if the link is still open.
        if let Some(frame) = frame {
            let sender = self
                .lock_links()
                .get(link_id)
                .map(|link| link.frames.clone());
            if let Some(sender) = sender {
                let _ = sender.send(frame);
            }
        }
        Ok(receipt)
    }

    /// Send-bridge (near-node step): record a DEFERRED async-settled mutation so
    /// the co-located settlement bridge can settle it by outbox op id when the
    /// flush settles. The mutation stays `Accepted` (in-flight) — its optimistic
    /// fold is HELD on the client until the terminal verdict arrives; confirming
    /// at the authority receipt would be a false Sent (D125).
    pub(crate) fn register_deferred_settlement(
        &self,
        operation_id: OperationId,
        link_id: RuntimeLinkId,
        mutation_id: RuntimeMutationId,
    ) {
        self.deferred_settlements
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(operation_id, (link_id, mutation_id));
    }

    /// Settle a deferred async mutation from the flush's terminal outcome (the
    /// co-located send-bridge): resolve the held (link, mutation) by op id and
    /// drive [`settle_mutation`] so the terminal `mutation.notification` reaches
    /// the client — `Confirmed` (the send left Drafts; the draft-Destroy fold
    /// confirms) or `Failed` (a parked/failed send; the client REVERTS the fold,
    /// the draft returns, no false Sent — D125). A no-op for an op id this runtime
    /// never deferred (a non-Send settlement, or a different near node).
    pub(crate) fn settle_deferred_settlement(
        &self,
        operation_id: &OperationId,
        confirmed: bool,
        error: Option<RuntimeAdapterError>,
    ) {
        let held = self
            .deferred_settlements
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(operation_id);
        let Some((link_id, mutation_id)) = held else {
            return;
        };
        let state = if confirmed {
            MutationSettlementState::Confirmed
        } else {
            MutationSettlementState::Failed
        };
        // Best-effort: a closed/reaped link has nothing left to settle.
        let _ = self.settle_mutation(&link_id, &mutation_id, state, error, Value::Null);
    }

    /// Co-located send-bridge dispatch: translate an async flush's terminal
    /// settlement DomainEvent into the deferred mutation's verdict. Shares the
    /// process event bus with the authority server, so this is the co-located
    /// delivery of the same terminal class the authority server routes as a
    /// `Settlement` frame for a remote near node. A no-op for any event that is
    /// not a deferred op's terminal settlement.
    pub(crate) fn settle_deferred_from_event(&self, event: &DomainEvent) {
        match event.topic.as_str() {
            EVENT_TOPIC_OPERATION_SETTLED => {
                let Ok(settlement) =
                    serde_json::from_value::<OperationSettlement>(event.payload.clone())
                else {
                    return;
                };
                match settlement.outcome {
                    OperationOutcome::Applied => {
                        self.settle_deferred_settlement(&settlement.id, true, None)
                    }
                    OperationOutcome::Failed => self.settle_deferred_settlement(
                        &settlement.id,
                        false,
                        Some(deferred_send_error(
                            settlement
                                .error
                                .unwrap_or_else(|| "send failed".to_string()),
                        )),
                    ),
                }
            }
            EVENT_TOPIC_OPERATION_DISPATCH_UNCERTAIN => {
                let Ok(uncertain) =
                    serde_json::from_value::<OperationDispatchUncertain>(event.payload.clone())
                else {
                    return;
                };
                self.settle_deferred_settlement(
                    &uncertain.id,
                    false,
                    Some(deferred_send_error(uncertain.reason)),
                );
            }
            _ => {}
        }
    }

    /// Read the current settlement of a mutation by its **client** mutation id —
    /// the near-end reconciler's cross-link query (D44b, `RuntimeLink::
    /// mutation_settlement`). `Ok(None)` when the runtime has no record: an
    /// unknown link (closed/restarted — its ledger was purged) or an unknown
    /// / already-cleared mutation id. A known link still enforces the caller
    /// scope; an unknown one cannot leak anything (there is nothing to read).
    pub(crate) fn mutation_settlement(
        &self,
        caller: RuntimeCaller,
        link_id: &RuntimeLinkId,
        client_mutation_id: &ClientMutationId,
    ) -> Result<Option<MutationReceipt>, RuntimeError> {
        {
            let links = self.lock_links();
            if let Some(link) = links.get(link_id) {
                ensure_caller_matches_link(link, caller.account_scope.as_deref())?;
            } else {
                return Ok(None);
            }
        }
        Ok(self
            .dedup
            .verdict(link_id, client_mutation_id)
            .map(|record| record.receipt()))
    }

    pub(crate) fn link_scope(
        &self,
        link_id: &RuntimeLinkId,
        caller_scope: Option<&[String]>,
    ) -> Result<Option<Vec<String>>, RuntimeError> {
        let links = self.lock_links();
        let link = links
            .get(link_id)
            .ok_or_else(|| RuntimeError::not_found("runtime link not found"))?;
        ensure_caller_matches_link(link, caller_scope)?;
        Ok(link.account_scope.clone())
    }

    fn record_open_view(
        &self,
        link_id: &RuntimeLinkId,
        snapshot: ViewSnapshot,
    ) -> Result<(), RuntimeError> {
        let mut links = self.lock_links();
        let link = links
            .get_mut(link_id)
            .ok_or_else(|| RuntimeError::not_found("runtime link not found"))?;
        link.open_views.insert(snapshot.view_id.clone());
        link.latest_snapshots
            .insert(snapshot.view_id.clone(), snapshot);
        Ok(())
    }

    fn spawn_view_forwarder(
        self: &Arc<Self>,
        link_id: RuntimeLinkId,
        mut subscription: RuntimeViewSubscription,
    ) {
        let registry = Arc::downgrade(self);
        tokio::spawn(async move {
            if let Some(frame) = subscription.catch_up.take() {
                let Some(registry) = registry.upgrade() else {
                    return;
                };
                if !registry.forward_view_frame(&link_id, frame) {
                    return;
                }
            }
            while let Some(frame) = subscription.live.next().await {
                let Some(registry) = registry.upgrade() else {
                    return;
                };
                if !registry.forward_view_frame(&link_id, frame) {
                    return;
                }
            }
        });
    }

    fn spawn_notification_forwarder(self: &Arc<Self>, link_id: RuntimeLinkId) -> AbortHandle {
        let registry = Arc::downgrade(self);
        let mut receiver = self.event_sender.subscribe();
        let task = tokio::spawn(async move {
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        let Some(registry) = registry.upgrade() else {
                            return;
                        };
                        if !registry.forward_notification(&link_id, event) {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(missed)) => {
                        // The event bus dropped `missed` events for this
                        // forwarder — they never reached the link's frame
                        // stream. Recover by collapsing to the link's current
                        // state so the client resyncs (re-snapshots open views
                        // + replays the live mutation window) rather than
                        // silently missing them (I3, `gap-detection`).
                        let Some(registry) = registry.upgrade() else {
                            return;
                        };
                        warn!(
                            link_id = %link_id.as_str(),
                            missed_events = missed,
                            "notification forwarder lagged; collapsing the link to resync",
                        );
                        registry.refresh_open_views(&link_id).await;
                        let _ = registry.collapse_link_into_stream(&link_id);
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        task.abort_handle()
    }

    fn forward_view_frame(&self, link_id: &RuntimeLinkId, frame: ViewFrame) -> bool {
        let mut next = || RuntimeLinkSeq::new(self.seq.stamp(link_id));
        let mut links = self.lock_links();
        let Some(link) = links.get_mut(link_id) else {
            return false;
        };
        let Some(runtime_frame) = view_frame_to_runtime(link, frame, &mut next) else {
            return false;
        };
        let sender = link.frames.clone();
        drop(links);
        let _ = sender.send(runtime_frame);
        true
    }

    fn forward_notification(&self, link_id: &RuntimeLinkId, event: DomainEvent) -> bool {
        let mut links = self.lock_links();
        let Some(link) = links.get_mut(link_id) else {
            return false;
        };
        if !event_matches_link_scope(&event, link.account_scope.as_deref()) {
            return true;
        }
        let payload = match serde_json::to_value(&event) {
            Ok(payload) => payload,
            Err(_) => return false,
        };
        let frame = RuntimeFrame::Notification {
            link_seq: self.stamp(link_id),
            kind: event.topic,
            payload,
        };
        let sender = link.frames.clone();
        drop(links);
        let _ = sender.send(frame);
        true
    }

    /// Re-derive each open view fresh and refresh the link's stored snapshot,
    /// so a subsequent collapse/catch-up serves current state rather than a stale
    /// cached one. Required because the per-event mail-list re-serve was retired
    /// (option iii): the link's stored mail-list snapshot is otherwise only
    /// refreshed on open/extend, so resync would replay stale rows. Holds no lock
    /// across the async recompute; `recompute_view_if_changed` no-ops views that
    /// haven't moved (so unchanged + still-#3-served views cost nothing).
    async fn refresh_open_views(&self, link_id: &RuntimeLinkId) {
        let open_views: Vec<ViewId> = {
            let links = self.lock_links();
            match links.get(link_id) {
                Some(link) => link.open_views.iter().cloned().collect(),
                None => return,
            }
        };
        for view_id in &open_views {
            if let Ok(Some(snapshot)) = self.views.recompute_view_if_changed(view_id).await {
                let mut links = self.lock_links();
                if let Some(link) = links.get_mut(link_id) {
                    if link.open_views.contains(view_id) {
                        link.latest_snapshots.insert(view_id.clone(), snapshot);
                    }
                }
            }
        }
    }

    fn collapse_link(
        &self,
        link_id: &RuntimeLinkId,
        caller_scope: Option<&[String]>,
    ) -> Result<Vec<RuntimeFrame>, RuntimeError> {
        let mutations = self.dedup.records_for(link_id);
        let mut links = self.lock_links();
        let link = links
            .get_mut(link_id)
            .ok_or_else(|| RuntimeError::not_found("runtime link not found"))?;
        ensure_caller_matches_link(link, caller_scope)?;
        let mut next = || RuntimeLinkSeq::new(self.seq.stamp(link_id));
        Ok(collapse_link_frames(link, &mutations, &mut next))
    }

    /// Collapse the link and push the frames into its stream — the
    /// notification forwarder's recovery from an event-bus lag (it silently
    /// dropped events; the client resyncs from the collapsed snapshot). No
    /// caller-scope check: the forwarder is the link's own component.
    fn collapse_link_into_stream(&self, link_id: &RuntimeLinkId) -> Result<(), RuntimeError> {
        let mutations = self.dedup.records_for(link_id);
        let mut links = self.lock_links();
        let link = links
            .get_mut(link_id)
            .ok_or_else(|| RuntimeError::not_found("runtime link not found"))?;
        let mut next = || RuntimeLinkSeq::new(self.seq.stamp(link_id));
        collapse_link_frames_into(link, &mutations, &mut next);
        Ok(())
    }
}

/// Build the collapse catch-up frames (snapshots of open views + terminal
/// mutation notifications). `next` stamps each frame's seq from the shared store
/// (D50). This is also the runtime seam's **Reset-path payload** (D49): a stale
/// resume collapses to exactly these frames rather than a per-frame replay.
fn collapse_link_frames(
    link: &mut StoredLink,
    mutations: &[StoredMutation],
    next: &mut impl FnMut() -> RuntimeLinkSeq,
) -> Vec<RuntimeFrame> {
    let mut frames = Vec::new();
    let mut snapshots: Vec<_> = link.latest_snapshots.values().cloned().collect();
    snapshots.sort_by(|left, right| left.view_id.as_str().cmp(right.view_id.as_str()));
    for snapshot in snapshots {
        let link_seq = next();
        frames.push(RuntimeFrame::ViewSnapshot {
            link_seq,
            view_id: snapshot.view_id.clone(),
            revision: snapshot.revision,
            snapshot,
        });
    }
    let mut mutations: Vec<_> = mutations.to_vec();
    mutations.sort_by(|left, right| left.mutation_id.as_str().cmp(right.mutation_id.as_str()));
    for mutation in mutations {
        // Replay only terminal verdicts on collapse; in-flight ops are re-folded
        // by the client from its own pending set over the re-served snapshots.
        if let Some(frame) = mutation.notification_frame(next()) {
            frames.push(frame);
        }
    }
    frames
}

/// Push the link's collapsed state into its frame stream — the recovery
/// path for a notification forwarder that lagged on the event bus: it silently
/// dropped events, so collapse to a consistent snapshot the client re-applies
/// (re-snapshot open views + replay the live mutation window) rather than
/// missing them (I3, `gap-detection`). Reuses [`collapse_link_frames`].
fn collapse_link_frames_into(
    link: &mut StoredLink,
    mutations: &[StoredMutation],
    next: &mut impl FnMut() -> RuntimeLinkSeq,
) {
    let sender = link.frames.clone();
    for frame in collapse_link_frames(link, mutations, next) {
        let _ = sender.send(frame);
    }
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

fn view_frame_to_runtime(
    link: &mut StoredLink,
    frame: ViewFrame,
    next: &mut impl FnMut() -> RuntimeLinkSeq,
) -> Option<RuntimeFrame> {
    match frame {
        ViewFrame::Snapshot { snapshot } => {
            if !link.open_views.contains(&snapshot.view_id) {
                return None;
            }
            let link_seq = next();
            let view_id = snapshot.view_id.clone();
            let revision = snapshot.revision;
            link.latest_snapshots
                .insert(view_id.clone(), snapshot.clone());
            Some(RuntimeFrame::ViewSnapshot {
                link_seq,
                view_id,
                revision,
                snapshot,
            })
        }
        ViewFrame::Replace { snapshot } => {
            if !link.open_views.contains(&snapshot.view_id) {
                return None;
            }
            let link_seq = next();
            let view_id = snapshot.view_id.clone();
            let revision = snapshot.revision;
            let previous = link
                .latest_snapshots
                .insert(view_id.clone(), snapshot.clone());
            // A delta-capable link receives only the rows that changed, when
            // the change is row-local; structural changes (and non-mail-list
            // views) fall back to a whole replace (L6).
            if link.delta_capable {
                if let Some(delta) = previous
                    .as_ref()
                    .and_then(|old| mail_list_delta(old, &snapshot))
                {
                    return Some(RuntimeFrame::ViewDelta {
                        link_seq,
                        view_id,
                        revision,
                        delta,
                    });
                }
            }
            Some(RuntimeFrame::ViewReplace {
                link_seq,
                view_id,
                revision,
                snapshot,
            })
        }
        ViewFrame::Error { view_id, error, .. } => {
            if !link.open_views.contains(&view_id) {
                return None;
            }
            let link_seq = next();
            Some(RuntimeFrame::ViewError {
                link_seq,
                view_id,
                error,
            })
        }
        ViewFrame::Closed { view_id } => {
            if !link.open_views.remove(&view_id) {
                return None;
            }
            link.latest_snapshots.remove(&view_id);
            let link_seq = next();
            Some(RuntimeFrame::ViewClosed { link_seq, view_id })
        }
    }
}

/// Wall-clock seconds — the `now` tick the dedup TTL / ack reaper is driven on
/// (D48), shared with the settlement-sink reaper's second-tick unit.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The carried error for a deferred send that parked/failed. The near node maps a
/// `Failed` mutation to `MutationNotification::Rejected`, so this is what the
/// client surfaces while reverting the optimistic draft-Destroy fold.
fn deferred_send_error(message: String) -> RuntimeAdapterError {
    RuntimeError::internal(message, None).envelope().clone()
}

/// Spawn the co-located send-bridge (near-node half): a bus subscriber that
/// settles a deferred async mutation (a Send) when its outbox flush emits
/// `operation.settled`/`dispatch_uncertain` on the shared process event bus. The
/// task holds a `Weak` handle so it self-terminates when the registry drops; a
/// broadcast lag skips (the deferred record stays until a later frame or link
/// reap). Must run within a Tokio runtime.
pub(crate) fn spawn_deferred_settlement_bridge(
    links: &Arc<LinkRegistry>,
    event_sender: &broadcast::Sender<DomainEvent>,
) -> tokio::task::JoinHandle<()> {
    let weak = Arc::downgrade(links);
    let mut rx = event_sender.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let Some(links) = weak.upgrade() else {
                        break;
                    };
                    links.settle_deferred_from_event(&event);
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

/// Map a runtime settlement (state + error) to the D47 dedup terminal class.
/// `Confirmed` is kept + bounded-evicted; a `Failed` splits by the error's typed
/// [`Terminality`] — a transient failure is `Failed` (cleared, so a retry
/// re-executes), a permanent rejection (or no error) is `Rejected` (kept so a
/// reconnecting client re-observes it). `Accepted` is non-terminal and never
/// settled; treated as `Rejected` defensively.
fn terminal_class_for(
    state: &MutationSettlementState,
    error: Option<&RuntimeAdapterError>,
) -> TerminalClass {
    match state {
        MutationSettlementState::Confirmed => TerminalClass::Confirmed,
        MutationSettlementState::Failed => {
            if error
                .map(|error| error.terminality.is_transient())
                .unwrap_or(false)
            {
                TerminalClass::Failed
            } else {
                TerminalClass::Rejected
            }
        }
        MutationSettlementState::Accepted => TerminalClass::Rejected,
    }
}

fn event_matches_link_scope(event: &DomainEvent, account_scope: Option<&[String]>) -> bool {
    account_scope
        .map(|scope| {
            scope
                .iter()
                .any(|account_id| account_id == event.account_id.as_str())
        })
        .unwrap_or(true)
}

fn ensure_caller_matches_link(
    link: &StoredLink,
    caller_scope: Option<&[String]>,
) -> Result<(), RuntimeError> {
    match (link.account_scope.as_deref(), caller_scope) {
        (None, None) => Ok(()),
        (Some(_), None) => Ok(()),
        (None, Some(_)) => Err(RuntimeError::unauthorized(
            "account-scoped caller cannot access a full-scope runtime link",
        )),
        (Some(link_scope), Some(caller_scope)) if link_scope == caller_scope => Ok(()),
        (Some(_), Some(_)) => Err(RuntimeError::unauthorized(
            "caller account scope does not match runtime link",
        )),
    }
}

#[cfg(test)]
mod delta_tests {
    use super::*;
    use posthaste_contract_core::{CoverageRange, RuntimeCoverage, ViewLifecycle, ViewRevision};
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
                ..Default::default()
            },
            revision: ViewRevision::new(1),
            lifecycle: ViewLifecycle::Ready,
            read_watermark: None,
            coverage: RuntimeCoverage {
                ranges: vec![CoverageRange {
                    from: None,
                    to: None,
                }],
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
                "coverage": { "ranges": [{ "from": null, "to": null }] },
                "knownTotalCount": null,
                "anchor": { "kind": "notRequested" }
            }),
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
mod race_tests {
    //! Deterministic interleaving tests for the two links.rs ordering races
    //! (D49 (b), [2]/[4]). The barrier hooks (`subscribe_barrier`/`accept_barrier`)
    //! fire at the exact race window so the interleaving is pinned, not chanced.
    use super::*;
    use async_trait::async_trait;
    use posthaste_authority_server_link::AuthorityServerApi;
    use serde_json::json;
    use std::time::Duration;

    /// An all-default `AuthorityServerApi` — the link races never touch it.
    struct StubApi;
    #[async_trait]
    impl AuthorityServerApi for StubApi {}

    fn registry() -> Arc<LinkRegistry> {
        let (event_sender, _) = broadcast::channel(64);
        let pending_set = Arc::new(crate::near_node::AuthorityServerPendingSet::new(false));
        let reads = Arc::new(crate::read::ReadCache::passthrough(Arc::new(StubApi)));
        let views = Arc::new(ViewRegistry::new(event_sender.clone(), pending_set, reads));
        Arc::new(LinkRegistry::new(views, event_sender))
    }

    fn request(link_id: &RuntimeLinkId, client_mutation_id: &str) -> MutationRequest {
        let operation: MailOperation = serde_json::from_value(json!({
            "name": "message.setFlaggedState",
            "args": { "sourceId": "acct", "messageId": "m1", "flagged": true },
        }))
        .unwrap();
        MutationRequest {
            link_id: Some(link_id.clone()),
            operation,
            client_mutation_id: ClientMutationId::new(client_mutation_id),
            context: None,
        }
    }

    // [4]: an accept that races a close_link must self-sweep its just-inserted
    // Pending record (no orphaned, eviction-exempt leak) and report not_found.
    #[tokio::test]
    async fn accept_racing_close_self_sweeps_and_reports_not_found() {
        let reg = registry();
        let sid = reg.open_link(RuntimeCaller::test()).unwrap().link_id;

        // Fire the race exactly between the dedup insert and the revalidation.
        let reg2 = reg.clone();
        let sid2 = sid.clone();
        *reg.accept_barrier.lock().unwrap() = Some(Box::new(move || {
            reg2.close_link(RuntimeCaller::test(), sid2.clone())
                .unwrap();
        }));

        let result = reg.accept_mutation(RuntimeCaller::test(), &request(&sid, "op-race"));
        assert!(result.is_err(), "a raced-closed link must not accept");
        // The Pending record was self-swept — no orphan leaks under the closed id.
        assert!(
            reg.dedup.records_for(&sid).is_empty(),
            "the just-inserted record was self-swept (no leak)"
        );
    }

    // [2]: a settle that lands between the live subscribe and the catch-up
    // snapshot must reach the LIVE stream (subscribe-then-snapshot), never lost.
    #[tokio::test]
    async fn settle_racing_subscribe_reaches_the_live_stream() {
        let reg = registry();
        let sid = reg.open_link(RuntimeCaller::test()).unwrap().link_id;
        let mutation_id = match reg
            .accept_mutation(RuntimeCaller::test(), &request(&sid, "op-1"))
            .unwrap()
        {
            MutationAcceptance::New { mutation_id } => mutation_id,
            _ => panic!("first accept is New"),
        };

        // Fire the settle exactly between frames.subscribe() and the snapshot.
        let reg2 = reg.clone();
        let sid2 = sid.clone();
        *reg.subscribe_barrier.lock().unwrap() = Some(Box::new(move || {
            reg2.settle_mutation(
                &sid2,
                &mutation_id,
                MutationSettlementState::Confirmed,
                None,
                Value::Null,
            )
            .unwrap();
        }));

        // A fresh subscribe (no prior cursor).
        let subscription = reg
            .subscribe_frames(RuntimeCaller::test(), sid.clone(), None)
            .await
            .unwrap();

        // The settle happened after we subscribed → it is on the live stream.
        let mut live = subscription.live;
        let frame = tokio::time::timeout(Duration::from_secs(1), live.next())
            .await
            .expect("the raced settlement must be delivered live, not lost")
            .expect("a live frame");
        assert!(
            matches!(
                frame,
                RuntimeFrame::MutationNotification {
                    notification: MutationNotification::Confirmed,
                    ..
                }
            ),
            "the live stream carries the settlement that raced the subscribe"
        );
    }

    // M28/D68 gate: open → stream → disconnect-WITHOUT-DELETE is reaped after the
    // idle TTL. The leaked session (no explicit close_link) is released by the
    // idle reaper, driven on the D48 `now` tick.
    #[tokio::test]
    async fn idle_session_reaped_after_ttl() {
        let reg = registry();
        let sid = reg.open_link(RuntimeCaller::test()).unwrap().link_id;

        // Stream: the SSE down-stream attaches (its boxed stream holds the frame
        // receiver, so `receiver_count() > 0` while connected).
        let subscription = reg
            .subscribe_frames(RuntimeCaller::test(), sid.clone(), None)
            .await
            .unwrap();
        let last_active = reg
            .lock_links()
            .get(&sid)
            .map(|link| link.last_active)
            .expect("the streamed link is registered");

        // Disconnect WITHOUT a DELETE: dropping the subscription drops the live
        // stream (and its receiver), but no `close_link` ran → the entry leaks.
        drop(subscription);
        assert!(
            reg.lock_links().contains_key(&sid),
            "leaks without a reaper"
        );

        // Within the TTL the reaper spares it.
        reg.reap_idle_sessions(last_active + 1);
        assert!(reg.lock_links().contains_key(&sid), "within ttl → spared");

        // Past the TTL it is reaped: the registry entry is released.
        let reaped = reg.reap_idle_sessions(last_active + SESSION_IDLE_TTL + 1);
        assert_eq!(reaped, vec![sid.clone()], "idle session reaped past ttl");
        assert!(
            !reg.lock_links().contains_key(&sid),
            "registry entry released"
        );
    }

    // A live down-stream is never reaped — the held receiver spares the link at
    // any tick (and refreshes its activity).
    #[tokio::test]
    async fn a_live_stream_is_never_reaped() {
        let reg = registry();
        let sid = reg.open_link(RuntimeCaller::test()).unwrap().link_id;
        let _subscription = reg
            .subscribe_frames(RuntimeCaller::test(), sid.clone(), None)
            .await
            .unwrap();
        assert!(
            reg.reap_idle_sessions(SESSION_IDLE_TTL * 10).is_empty(),
            "a live down-stream is never reaped"
        );
        assert!(reg.lock_links().contains_key(&sid));
    }

    fn operation_settled_event(op_id: &str, applied: bool) -> DomainEvent {
        let settlement = OperationSettlement {
            id: OperationId::from(op_id),
            outcome: if applied {
                OperationOutcome::Applied
            } else {
                OperationOutcome::Failed
            },
            assigned_entity_id: None,
            error: (!applied).then(|| "send permanently failed".to_string()),
        };
        DomainEvent {
            seq: 1,
            account_id: posthaste_domain_model::AccountId::from("acct"),
            topic: EVENT_TOPIC_OPERATION_SETTLED.to_string(),
            occurred_at: String::new(),
            mailbox_id: None,
            message_id: None,
            payload: serde_json::to_value(&settlement).unwrap(),
        }
    }

    fn dispatch_uncertain_event(op_id: &str) -> DomainEvent {
        let uncertain = OperationDispatchUncertain {
            id: OperationId::from(op_id),
            reason: "send timed out; delivery uncertain".to_string(),
        };
        DomainEvent {
            seq: 1,
            account_id: posthaste_domain_model::AccountId::from("acct"),
            topic: EVENT_TOPIC_OPERATION_DISPATCH_UNCERTAIN.to_string(),
            occurred_at: String::new(),
            mailbox_id: None,
            message_id: None,
            payload: serde_json::to_value(&uncertain).unwrap(),
        }
    }

    async fn deferred_send_link() -> (Arc<LinkRegistry>, RuntimeLinkId, RuntimeMutationId) {
        let reg = registry();
        let sid = reg.open_link(RuntimeCaller::test()).unwrap().link_id;
        let mutation_id = match reg
            .accept_mutation(RuntimeCaller::test(), &request(&sid, "send-cmid"))
            .unwrap()
        {
            MutationAcceptance::New { mutation_id } => mutation_id,
            _ => panic!("first accept is New"),
        };
        (reg, sid, mutation_id)
    }

    // Send-bridge (near-node, co-located): a deferred send whose flush settles
    // Applied delivers a terminal `Confirmed` notification to the client (the
    // draft-Destroy fold confirms — the send left Drafts).
    #[tokio::test]
    async fn deferred_send_applied_confirms_at_the_client() {
        let (reg, sid, mutation_id) = deferred_send_link().await;
        reg.register_deferred_settlement(OperationId::from("op-send-c"), sid.clone(), mutation_id);
        // Accepted (in-flight) at subscribe — no notification yet.
        let subscription = reg
            .subscribe_frames(RuntimeCaller::test(), sid.clone(), None)
            .await
            .unwrap();
        let mut live = subscription.live;
        // The async flush settles Applied → the client confirms.
        reg.settle_deferred_from_event(&operation_settled_event("op-send-c", true));
        let frame = tokio::time::timeout(Duration::from_secs(1), live.next())
            .await
            .expect("a deferred Applied must deliver a terminal notification")
            .expect("a live frame");
        assert!(matches!(
            frame,
            RuntimeFrame::MutationNotification {
                notification: MutationNotification::Confirmed,
                ..
            }
        ));
    }

    // Send-bridge (near-node, co-located) D125: a PARKED send (DispatchUncertain)
    // delivers a terminal `Rejected` notification — the client REVERTS the
    // optimistic draft-Destroy fold (the draft returns) and there is NO false
    // Sent. A permanent Failed settlement behaves the same.
    #[tokio::test]
    async fn deferred_send_parked_reverts_at_the_client_no_false_sent() {
        let (reg, sid, mutation_id) = deferred_send_link().await;
        reg.register_deferred_settlement(OperationId::from("op-send-p"), sid.clone(), mutation_id);
        let subscription = reg
            .subscribe_frames(RuntimeCaller::test(), sid.clone(), None)
            .await
            .unwrap();
        let mut live = subscription.live;
        reg.settle_deferred_from_event(&dispatch_uncertain_event("op-send-p"));
        let frame = tokio::time::timeout(Duration::from_secs(1), live.next())
            .await
            .expect("a parked send must deliver a terminal notification")
            .expect("a live frame");
        assert!(
            matches!(
                frame,
                RuntimeFrame::MutationNotification {
                    notification: MutationNotification::Rejected { .. },
                    ..
                }
            ),
            "a parked send reverts (Rejected) — not a false Sent"
        );
    }

    // An op id this runtime never deferred is ignored (a non-Send settlement, or a
    // different near node's op) — no panic, no spurious frame.
    #[tokio::test]
    async fn a_non_deferred_settlement_is_ignored() {
        let (reg, _sid, _mutation_id) = deferred_send_link().await;
        reg.settle_deferred_from_event(&operation_settled_event("op-unknown", true));
        // No deferred record removed, nothing settled — the call is a no-op.
    }
}

#[cfg(test)]
mod collapse_tests {
    //! Collapse-frame coverage for the runtime far-end. The dedup ledger's
    //! eviction and the D47 terminal-class rule now live in — and are tested by
    //! — the shared `posthaste-link-far-end` sub-store; these tests pin what
    //! *collapse* means for this seam's frames (the mutation window replayed to a
    //! reconnecting / lag-resyncing client).
    use super::*;
    use serde_json::json;

    fn stored_mutation(id: u64, state: MutationSettlementState) -> StoredMutation {
        let operation: MailOperation = serde_json::from_value(json!({
            "name": "message.setFlaggedState",
            "args": { "sourceId": "acct", "messageId": "m1", "flagged": true },
        }))
        .expect("operation builds from the flat wire shape");
        StoredMutation {
            mutation_id: RuntimeMutationId::new(format!("mutation-{id}")),
            client_mutation_id: ClientMutationId::new(format!("client-{id}")),
            operation,
            state,
            error: None,
            output: Value::Null,
        }
    }

    fn empty_link() -> StoredLink {
        let (frames, _) = broadcast::channel(8);
        StoredLink {
            account_scope: None,
            delta_capable: false,
            frames,
            open_views: HashSet::new(),
            latest_snapshots: HashMap::new(),
            event_task: None,
            last_active: 0,
        }
    }

    /// A monotonic stamping closure for the collapse helpers under test (the
    /// production path draws the same seqs from the shared `ReplayStore`, D50).
    fn test_stamp() -> impl FnMut() -> RuntimeLinkSeq {
        let mut n = 0u64;
        move || {
            n += 1;
            RuntimeLinkSeq::new(n)
        }
    }

    #[test]
    fn collapse_replays_a_terminal_notification_per_mutation() {
        let mut link = empty_link();
        let mutations: Vec<_> = (1..=3)
            .map(|id| stored_mutation(id, MutationSettlementState::Confirmed))
            .collect();
        let mut next = test_stamp();
        let frames = collapse_link_frames(&mut link, &mutations, &mut next);
        let mutation_frames = frames
            .iter()
            .filter(|f| matches!(f, RuntimeFrame::MutationNotification { .. }))
            .count();
        assert_eq!(
            mutation_frames, 3,
            "one settlement frame per terminal mutation"
        );
    }

    #[test]
    fn collapse_into_stream_pushes_the_window_for_resync() {
        // The notification forwarder lagged on the event bus and silently dropped
        // events; `collapse_link_frames_into` pushes the link's collapsed
        // state into the frame stream so the client resyncs (I3, gap-detection).
        let (frames, mut rx) = broadcast::channel(8);
        let mut link = empty_link();
        link.frames = frames;
        let mutations: Vec<_> = (1..=3)
            .map(|id| stored_mutation(id, MutationSettlementState::Confirmed))
            .collect();

        let mut next = test_stamp();
        collapse_link_frames_into(&mut link, &mutations, &mut next);

        let mut streamed = 0;
        while let Ok(frame) = rx.try_recv() {
            if matches!(frame, RuntimeFrame::MutationNotification { .. }) {
                streamed += 1;
            }
        }
        assert_eq!(
            streamed, 3,
            "collapse streamed the mutation window into the stream"
        );
    }
}
