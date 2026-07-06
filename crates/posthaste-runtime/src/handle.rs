//! The runtime handle + its trait impls (D29 split from `build.rs`): the shared
//! `RuntimeCoreState`, the cloneable [`RuntimeHandle`], its inherent helpers
//! (mutation dispatch, event-stream wiring), and the five trait impls that
//! realize the two surfaces extracted from `RuntimeCore` — four
//! `posthaste-runtime-api` facets ([`RuntimeAccountApi`], [`RuntimeSettingsApi`],
//! [`RuntimeMailReadApi`], [`RuntimeMailWriteApi`]) + the [`RuntimeLink`]
//! link-protocol trait from `posthaste-client-link`. `replay_events` is dropped
//! from the public surface (zero production consumers); the runtime builds its
//! subscription backlog internally via `ReadCache::replay_events`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use futures_util::StreamExt;
use posthaste_authority_server_link::AuthorityServerLinkHandle;
use posthaste_client_link::{
    RuntimeEventStream, RuntimeEventSubscription, RuntimeFrameSubscription, RuntimeLink,
};
use posthaste_contract_core::{
    AccountScopeRequest, AccountVerificationResult, ClientMutationId, CreateAccountMutation,
    MailOperation, MailQueryPage, MailQueryRequest, MessageResourceKind, MutationReceipt,
    MutationRequest, MutationSettlementState, PatchAccountMutation, RuntimeAccountList,
    RuntimeCaller, RuntimeError, RuntimeErrorCode, RuntimeLifecycle, RuntimeLinkConnection,
    RuntimeLinkId, RuntimeLinkSeq, RuntimeMutationId, RuntimeResourceBytes, RuntimeStatus,
    ViewDescriptor, ViewId,
};
use posthaste_domain_model::{
    AccountId, AccountOverview, AppSettings, CachedSenderAddress, CommandAck, CommandResult,
    DomainEvent, DraftContent, EventFilter, Identity, MailboxId, MailboxSummary, MessageId,
    Operation, OperationId, ReplyContext, SendMessageRequest, SmartMailbox, SmartMailboxId,
    SmartMailboxSummary, SyncMode, TagSummary,
};
use posthaste_replica_core::{MutationId, PendingMessageMutation};
use posthaste_runtime_api::{
    RuntimeAccountApi, RuntimeMailReadApi, RuntimeMailWriteApi, RuntimeSettingsApi,
};
use tokio::sync::broadcast;

use posthaste_link_far_end::down::{Sequenced, Tap, TapResume};

use crate::apply_ledger::{AppliedOutcome, ApplyLedger, Reserved, DRAFT_DELETE_OP, DRAFT_SAVE_OP};
use crate::far_end::links::{LinkRegistry, MutationAcceptance};
use crate::near_node::{named_message_assertion, AuthorityServerPendingSet};
use crate::read::{EventLogFactLog, ReadCache};

/// One tap subscriber's opaque server-side identity (RFC-L2-scripting §5.4): the
/// key of its reaper-managed registry entry. Each `/v1/events` (re)subscription
/// is allocated a fresh id from the runtime's monotonic counter; the entry is
/// registered on subscribe and dropped when the SSE stream ends (or reaped on
/// idle). Opaque — never surfaced on the wire.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct TapSubscriberId(u64);

/// The runtime's fact-carrying event tap (RFC-L2-scripting D52): the down-channel
/// half over the durable `event_log` via [`EventLogFactLog`], mounted on
/// `/v1/events` by [`RuntimeHandle::subscribe_events`].
pub(crate) type EventTap = Tap<EventLogFactLog, TapSubscriberId>;

/// The shared runtime core behind the cloneable handle: the authority server link, the
/// pending set, the read cache, the event bus, and the view/link registries.
pub(crate) struct RuntimeCoreState {
    // Neither the service/store nor the authority server far node is held here: every
    // authority server operation now routes through the link — `authority_server_link` for the
    // mutation up-channel and the typed write commands, `reads` for the read
    // channel. account_reads/account_supervisor are likewise not held — every
    // view (incl. AccountStatus) reads through `reads`.
    /// The runtime↔authority-server link over its (config-selected, in-process by
    /// default) transport. The mutation up-channel + typed writes go through here.
    pub(crate) authority_server_link: AuthorityServerLinkHandle,
    /// The runtime's pending set toward the authority server: forwarded-but-unconfirmed
    /// mutations, folded optimistically into served views (L4 §4.3).
    pub(crate) pending_set: Arc<AuthorityServerPendingSet>,
    /// The read-through cache over the far node (W4a: passthrough). Point reads
    /// and the mail-list base draw from here.
    pub(crate) reads: Arc<ReadCache>,
    pub(crate) event_sender: broadcast::Sender<DomainEvent>,
    // The OAuth holdout: account CRUD the lean near node can't do over the link
    // yet, so it routes to the local authority server's mutation service. Present only in
    // a `authority_server`-linked build; a lean near node has no such service.
    //
    // No direct `views: Arc<ViewRegistry>` field here (D51/M10 removed the last
    // reader, the sessionless `open_view`/`subscribe_view` trait methods): every
    // remaining view operation is link-scoped and reaches the registry through
    // `links` (`LinkRegistry` holds its own `Arc<ViewRegistry>`, the same
    // instance — see `assemble_runtime`).
    pub(crate) links: Arc<LinkRegistry>,
    /// The fact-carrying event tap mounted on `/v1/events` (RFC-L2-scripting D52):
    /// the durable-replay + gap-frame + subscriber-registry machinery behind
    /// [`RuntimeHandle::subscribe_events`]. Shares the same `event_log` (through
    /// `reads`) the live broadcast records into, so replay seqs and live seqs are
    /// one sequence.
    pub(crate) event_tap: Arc<EventTap>,
    /// Allocator for opaque [`TapSubscriberId`]s — one per `/v1/events`
    /// subscription (§5.4). Monotonic; the value is never surfaced on the wire.
    pub(crate) tap_subscriber_seq: AtomicU64,
    /// The apply-scoped idempotency ledger (RFC-L2-scripting D53 / P8 fix): makes
    /// a script's at-least-once write-back through [`RuntimeMailWriteApi::apply`]
    /// safe under redelivery when the caller supplies an idempotency key. Reuses
    /// the far-end up-half `DedupStore`; dedicated to the direct-apply path.
    pub(crate) apply_ledger: ApplyLedger,
    pub(crate) startup_status: RuntimeStatus,
    pub(crate) stopped: Arc<AtomicBool>,
}

/// Cloneable authority runtime handle used by transport adapters.
///
/// spec: docs/runtime/internals/L1#runtime-handle-transport-neutral
/// spec: docs/authority-server/L2#handle-methods-transport-free
#[derive(Clone)]
pub struct RuntimeHandle {
    pub(crate) core: Arc<RuntimeCoreState>,
}

/// Drop-guard ensuring a runtime mutation reaches a terminal settlement even
/// when its dispatch future is cancelled (e.g. the client disconnects mid-
/// `run_message_mutation`). Without it, a dropped `forward.await` skips both the
/// `Confirmed` and `Failed` branches, leaving the mutation stuck `Accepted`
/// forever — never pruned, no terminal `mutation.notification` frame, an
/// unbounded pending-set leak. Disarmed on the normal settle paths so it no-ops when
/// the dispatch completes.
struct MutationCancelGuard {
    links: Arc<LinkRegistry>,
    link_id: RuntimeLinkId,
    mutation_id: RuntimeMutationId,
    armed: bool,
}

impl Drop for MutationCancelGuard {
    fn drop(&mut self) {
        if self.armed {
            self.links
                .settle_mutation(
                    &self.link_id,
                    &self.mutation_id,
                    MutationSettlementState::Failed,
                    Some(
                        RuntimeError::internal(
                            "runtime mutation dispatch was cancelled \
                             (client disconnect mid-dispatch)",
                            None,
                        )
                        .envelope()
                        .clone(),
                    ),
                    serde_json::Value::Null,
                )
                .ok();
        }
    }
}

impl RuntimeHandle {
    /// The current event-log head seq — the **snapshot-attach** consistency token
    /// (RFC-L2-scripting §5.3): the seq a fresh `/v1/events` subscriber attaches
    /// at, so a read stamped with it can be followed by a gap-free tap tail from
    /// exactly that point. Sourced from the S2 event-log bounds query
    /// (`MAX(seq)`), the same value `subscribe_events` seeds a fresh cursor with.
    /// Best-effort: `None` when the bounds query is unavailable (the read then
    /// omits `asOfSeq` rather than failing).
    pub async fn current_event_seq(&self) -> Option<u64> {
        self.core
            .reads
            .event_log_bounds()
            .await
            .ok()
            .flatten()
            .map(|bounds| bounds.newest.max(0) as u64)
    }

    /// The runtime's local status: lifecycle + the build-time store snapshot.
    /// The live account count is layered on in `runtime_status` via the link.
    fn current_status(&self) -> RuntimeStatus {
        // Runtime-local status only (lifecycle + the build-time store snapshot);
        // the live account count is layered on in `runtime_status` via the link.
        let mut status = self.core.startup_status.clone();
        if self.core.stopped.load(Ordering::SeqCst) {
            status.lifecycle = RuntimeLifecycle::Stopped;
        }
        status
    }

    fn ensure_runtime_active(&self) -> Result<(), RuntimeError> {
        let lifecycle = self.current_status().lifecycle;
        if matches!(
            lifecycle,
            RuntimeLifecycle::Ready | RuntimeLifecycle::Degraded
        ) {
            return Ok(());
        }
        let message = format!("runtime is {}", runtime_lifecycle_label(&lifecycle));
        Err(RuntimeError::with_details(
            RuntimeErrorCode::RuntimeNotReady,
            message,
            serde_json::json!({ "lifecycle": lifecycle }),
        ))
    }

    fn ensure_account_in_scope(
        account_id: &str,
        account_scope: Option<&[String]>,
    ) -> Result<(), RuntimeError> {
        if account_scope.is_some_and(|scope| !scope.iter().any(|id| id == account_id)) {
            return Err(RuntimeError::unauthorized(
                "mutation source is outside the runtime link account scope",
            ));
        }
        Ok(())
    }

    /// Accept a named message mutation onto the link (idempotency), forward it
    /// up the authority server link, and settle the link stream from the authority server's
    /// receipt. The `forward` future is the link's up-channel
    /// (`AuthorityServerLinkHandle::forward_mutation`); its receipt carries the command's
    /// events as `output` and the authority server's confirmation id. Scope is the
    /// runtime's (near-node) concern, resolved before this call; the undo-history
    /// diff is captured in `dispatch_named_mutation` after settlement.
    ///
    /// @spec docs/runtime/mutations/L1#mutation-pipeline-and-catalog
    async fn run_message_mutation<Fut>(
        &self,
        caller: RuntimeCaller,
        request: &MutationRequest,
        forward: Fut,
    ) -> Result<MutationReceipt, RuntimeError>
    where
        Fut: std::future::Future<Output = Result<MutationReceipt, RuntimeError>>,
    {
        let link_id = request
            .link_id
            .clone()
            .ok_or_else(|| RuntimeError::invalid_mutation("runtime mutation requires a link id"))?;
        let mutation_id = match self.core.links.accept_mutation(caller, request)? {
            MutationAcceptance::New { mutation_id, .. } => mutation_id,
            MutationAcceptance::Existing(receipt) => return Ok(receipt),
        };
        // Arm the cancel-guard before awaiting the authority server: if `forward.await`
        // is dropped (client disconnect mid-dispatch), neither branch below
        // runs, and the guard's Drop settles `Failed` so the mutation doesn't
        // leak `Accepted` forever. Disarmed on each normal settle path.
        let mut guard = MutationCancelGuard {
            links: self.core.links.clone(),
            link_id: link_id.clone(),
            mutation_id: mutation_id.clone(),
            armed: true,
        };
        match forward.await {
            Ok(authority_server_receipt) => {
                guard.armed = false;
                // Send-bridge (near-node step 2): an async-flush-settled op (a
                // Send) has NO terminal verdict at the authority receipt — it
                // returns `Accepted`, its confirm/park/fail rides the async-flush
                // Settlement bridge (D125/D126). HOLD the optimistic draft-Destroy
                // fold: register the deferred settlement (keyed by the outbox op id
                // carried on the receipt) and leave the mutation `Accepted` — do
                // NOT settle `Confirmed` here (that would be a false Sent — the
                // send has not actually left the provider yet).
                if matches!(request.operation, MailOperation::Send(_)) {
                    if let Some(operation_id) = authority_server_receipt
                        .output
                        .get("deferredOperationId")
                        .and_then(|value| value.as_str())
                    {
                        self.core.links.register_deferred_settlement(
                            OperationId::from(operation_id),
                            link_id.clone(),
                            mutation_id.clone(),
                        );
                    }
                    return Ok(authority_server_receipt);
                }
                // The authority server already serialized the command's events as the
                // receipt output (state-before-event: the effect is applied
                // before the receipt returns); settle the link with it.
                self.core.links.settle_mutation(
                    &link_id,
                    &mutation_id,
                    MutationSettlementState::Confirmed,
                    None,
                    authority_server_receipt.output,
                )
            }
            Err(error) => {
                guard.armed = false;
                let envelope = error.envelope().clone();
                self.core.links.settle_mutation(
                    &link_id,
                    &mutation_id,
                    MutationSettlementState::Failed,
                    Some(envelope),
                    serde_json::Value::Null,
                )
            }
        }
    }

    fn event_matches_filter(event: &DomainEvent, filter: &EventFilter) -> bool {
        if let Some(account_id) = &filter.account_id {
            if &event.account_id != account_id {
                return false;
            }
        }
        if let Some(after_seq) = filter.after_seq {
            if event.seq <= after_seq {
                return false;
            }
        }
        if let Some(topic) = &filter.topic {
            if &event.topic != topic {
                return false;
            }
        }
        if let Some(mailbox_id) = &filter.mailbox_id {
            if event.mailbox_id.as_ref() != Some(mailbox_id) {
                return false;
            }
        }
        true
    }

    /// The tap's live half (RFC-L2-scripting D52): the `event_log`-recording
    /// broadcast feed, tailed behind the durable prelude. A single `cursor` (the
    /// highest seq the consumer has been advanced to, seeded from the prelude)
    /// gates delivery and anchors durable recovery.
    ///
    /// **N8 closed** (§3): a broadcast overflow (`Lagged`) is *not* silently
    /// swallowed. Facts recover by durable replay — the missed range is re-read
    /// from the `event_log` after the cursor and re-delivered, then the live tail
    /// resumes. A fact stream never drops a fact on the floor.
    fn live_event_stream(
        mut receiver: broadcast::Receiver<DomainEvent>,
        filter: EventFilter,
        reads: Arc<ReadCache>,
        unsubscribe: TapUnsubscribeGuard,
        replayed_through: Option<i64>,
    ) -> RuntimeEventStream {
        let stream = async_stream::stream! {
            // Held for the stream's lifetime: drops the tap's subscriber-registry
            // entry when the SSE stream ends (§5.4 — deterministic teardown, the
            // idle reaper is the backstop).
            let _unsubscribe = unsubscribe;
            let mut cursor = replayed_through;
            loop {
                match receiver.recv().await {
                    Ok(event)
                        if cursor.is_none_or(|seq| event.seq > seq)
                            && Self::event_matches_filter(&event, &filter) =>
                    {
                        cursor = Some(event.seq);
                        yield event;
                    }
                    Ok(event) => {
                        // Covered by the prelude or filtered out — not delivered,
                        // but still advance the durable-replay anchor past it so a
                        // later `Lagged` re-replay does not rescan it.
                        if cursor.is_none_or(|seq| event.seq > seq) {
                            cursor = Some(event.seq);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // The durable resume the tap exists for: re-read the facts
                        // the overflow dropped and re-deliver them (at-least-once,
                        // deduped by id downstream), then continue live.
                        let mut replay_filter = filter.clone();
                        replay_filter.after_seq = cursor;
                        match reads.replay_events(replay_filter).await {
                            Ok(events) => {
                                for event in events {
                                    if cursor.is_none_or(|seq| event.seq > seq)
                                        && Self::event_matches_filter(&event, &filter)
                                    {
                                        cursor = Some(event.seq);
                                        yield event;
                                    }
                                }
                            }
                            Err(error) => tracing::warn!(
                                %error,
                                "event tap durable re-replay after a broadcast lag failed; \
                                 the live tail continues from the current cursor"
                            ),
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        };
        stream.boxed()
    }

    /// Route a single named message mutation through the shared accept → forward
    /// → settle flow, folding its optimistic assertion into the pending set, and — for
    /// a diff-eligible user mutation — record the invertible change-diff onto the
    /// link's undo history once the authority server confirms it. `message.applyDiff`
    /// (undo/redo) goes through [`run_apply_diff`] instead, which wraps this flow
    /// with history navigation.
    ///
    /// @spec docs/runtime/mutations/L1#mutation-pipeline-and-catalog
    async fn dispatch_named_mutation(
        &self,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
        request: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        let link_scope = self
            .core
            .links
            .link_scope(&link_id, caller.account_scope.as_deref())?;
        // Phase 2: `revCursor` is a control operation (not a message mutation) —
        // it carries no message target + has no pending-set optimism. Route it
        // directly to the authority server (which validates + applies the cursor).
        // The typed variant carries `RevCursorArgs`, so there is no per-site arg
        // re-parse (D22).
        // @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
        if let MailOperation::RevCursor(args) = &request.operation {
            let account_id = args.account_id.clone();
            return self
                .dispatch_rev_cursor(caller, link_scope.as_deref(), account_id, request)
                .await;
        }
        // Runtime (near-node) concern: scope enforcement per mutation. The
        // command application is the authority server's; it is forwarded up the link
        // below, uniform across operations. Undo/redo history is client-owned
        // (@spec docs/eph/DESIGN-L2-undo-redo-synced-history): the runtime no
        // longer records change-diffs or navigates a history stack — an undo
        // or redo is an ordinary `message.applyDiff` operation that flows
        // through this same path.
        let source_id = request.operation.account_id().to_string();
        Self::ensure_account_in_scope(&source_id, link_scope.as_deref())?;
        // Accept the mutation into the runtime's pending set toward the authority server so
        // recomputed views fold it optimistically while it is in flight. It is
        // settled from the receipt below: co-located it retires on receipt (the
        // forward confirms synchronously, so the pending set is empty between
        // mutations and the overlay is a pass-through, `colocated-unchanged`);
        // remote it retires by absorption when the down-channel base assertion
        // arrives, so a receipt that outruns the `message.updated` propagation
        // does not recompute against a stale base (the near-node flicker).
        let optimistic = named_message_assertion(&request).map(|(message_id, assertion)| {
            let id = MutationId(request.client_mutation_id.as_str().to_string());
            self.core.pending_set.accept(PendingMessageMutation {
                id: id.clone(),
                key: message_id,
                effect: assertion,
            });
            id
        });
        // Up-channel: forward the named mutation to the authority server far node.
        let forward = self
            .core
            .authority_server_link
            .forward_mutation(request.clone());
        let result = self.run_message_mutation(caller, &request, forward).await;
        if let Some(id) = optimistic {
            // An authority server rejection settles as `Ok(receipt)` carrying a `Failed`
            // state (the verdict is on `error.code`), so the confirm signal is
            // the receipt state, not `is_ok()`.
            let confirmed = matches!(
                &result,
                Ok(receipt) if receipt.state == MutationSettlementState::Confirmed
            );
            self.core.pending_set.settle_receipt(&id, confirmed);
        }
        result
    }

    /// Phase 2: route a `revCursor` control operation to the authority server.
    /// The account (for the scope check) is read from the already-typed
    /// `RevCursorArgs` — no per-site arg re-parse (D22) — then the request is
    /// forwarded through the normal accept → forward → settle flow (no pending-set
    /// optimism — a cursor move has no message assertion to fold). The authority
    /// server validates the referenced steps exist + applies the cursor + emits
    /// the recompute trigger.
    ///
    /// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
    async fn dispatch_rev_cursor(
        &self,
        caller: RuntimeCaller,
        link_scope: Option<&[String]>,
        account_id: String,
        request: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        Self::ensure_account_in_scope(&account_id, link_scope)?;
        let forward = self
            .core
            .authority_server_link
            .forward_mutation(request.clone());
        self.run_message_mutation(caller, &request, forward).await
    }
}

fn runtime_lifecycle_label(lifecycle: &RuntimeLifecycle) -> &'static str {
    match lifecycle {
        RuntimeLifecycle::Starting => "starting",
        RuntimeLifecycle::Ready => "ready",
        RuntimeLifecycle::Degraded => "degraded",
        RuntimeLifecycle::Stopping => "stopping",
        RuntimeLifecycle::Stopped => "stopped",
    }
}

#[async_trait]
impl RuntimeAccountApi for RuntimeHandle {
    async fn runtime_status(&self, _caller: RuntimeCaller) -> Result<RuntimeStatus, RuntimeError> {
        let mut status = self.current_status();
        // The live account count is authority server state; read it through the link
        // (best-effort — a status read never fails on a count miss).
        if let Ok(Some(account_count)) = self.core.reads.account_count().await {
            status.account_count = account_count;
        }
        Ok(status)
    }

    async fn list_accounts(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<RuntimeAccountList, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_accounts().await
    }

    async fn get_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<AccountOverview, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .reads
            .get_account(account_id)
            .await?
            .ok_or_else(|| RuntimeError::not_found("account not found"))
    }

    async fn resolve_account_scope(
        &self,
        _caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<Vec<AccountId>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.resolve_account_scope(scope).await
    }

    async fn create_account(
        &self,
        _caller: RuntimeCaller,
        mutation: CreateAccountMutation,
    ) -> Result<AccountOverview, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .create_account(mutation)
            .await
    }

    async fn patch_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        mutation: PatchAccountMutation,
    ) -> Result<AccountOverview, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .patch_account(account_id, mutation)
            .await
    }

    async fn delete_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .delete_account(account_id)
            .await
    }

    async fn verify_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<AccountVerificationResult, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .verify_account(account_id)
            .await
    }

    async fn set_account_enabled(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        enabled: bool,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .set_account_enabled(account_id, enabled)
            .await
    }

    async fn reload_config(&self, _caller: RuntimeCaller) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.authority_server_link.reload_config().await
    }

    async fn sync_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        mode: SyncMode,
    ) -> Result<usize, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .sync_account(account_id, mode)
            .await
    }
}

#[async_trait]
impl RuntimeSettingsApi for RuntimeHandle {
    async fn get_app_settings(&self, _caller: RuntimeCaller) -> Result<AppSettings, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.app_settings().await
    }

    async fn patch_app_settings(
        &self,
        _caller: RuntimeCaller,
        mutation: posthaste_contract_core::PatchAppSettingsMutation,
    ) -> Result<AppSettings, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .patch_app_settings(mutation)
            .await
    }

    async fn preview_automation_rule(
        &self,
        _caller: RuntimeCaller,
        mutation: posthaste_contract_core::AutomationRulePreviewMutation,
    ) -> Result<posthaste_contract_core::AutomationRulePreviewResult, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .preview_automation_rule(mutation)
            .await
    }
}

#[async_trait]
impl RuntimeMailReadApi for RuntimeHandle {
    async fn list_mailboxes(
        &self,
        _caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<std::collections::BTreeMap<AccountId, Vec<MailboxSummary>>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_mailboxes(scope).await
    }

    async fn set_mailbox_role(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        mailbox_id: MailboxId,
        role: Option<String>,
    ) -> Result<Vec<MailboxSummary>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .set_mailbox_role(account_id, mailbox_id, role)
            .await
    }

    async fn create_mailbox(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        name: String,
    ) -> Result<Vec<MailboxSummary>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .create_mailbox(account_id, name)
            .await
    }

    async fn list_smart_mailboxes(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<Vec<SmartMailboxSummary>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_smart_mailboxes().await
    }

    async fn get_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<SmartMailbox, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.get_smart_mailbox(smart_mailbox_id).await
    }

    async fn create_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        mutation: posthaste_contract_core::CreateSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .create_smart_mailbox(mutation)
            .await
    }

    async fn patch_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
        mutation: posthaste_contract_core::PatchSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .patch_smart_mailbox(smart_mailbox_id, mutation)
            .await
    }

    async fn delete_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .delete_smart_mailbox(smart_mailbox_id)
            .await
    }

    async fn reset_default_smart_mailboxes(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<Vec<SmartMailboxSummary>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .reset_default_smart_mailboxes()
            .await
    }

    async fn list_tags(
        &self,
        _caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<Vec<TagSummary>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_tags(scope).await
    }

    async fn query_mail_page(
        &self,
        _caller: RuntimeCaller,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.query_mail_page(request).await
    }

    async fn get_message_detail(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<CommandResult, RuntimeError> {
        self.ensure_runtime_active()?;
        // Body-free: the detail read serves header + cached attachments only and
        // never loads the body (it is the separate `/body` lazy resource), so
        // opening a message neither provider-fetches nor materializes the body.
        let detail = self
            .core
            .reads
            .message_detail(&account_id, &message_id)
            .await?;
        Ok(CommandResult {
            detail,
            events: Vec::new(),
        })
    }

    /// Resolve a message's lazy bytes (attachment blob or body) as raw bytes +
    /// content type. The single entry point for every deferred message resource.
    async fn get_message_resource(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        kind: MessageResourceKind,
    ) -> Result<RuntimeResourceBytes, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .reads
            .get_message_resource(account_id, message_id, kind)
            .await
    }
}

/// The canonical apply-ledger operation name for a keyed send (RFC-L2-scripting
/// ruling 24). A send is not a [`MailOperation`], so it carries its own stable
/// name in the shared ledger's `(scope, key) → op_name` slot; it is distinct
/// from every `MailOperation::name()` value AND from `draft.save`/`draft.delete`,
/// so a key reused across a send and a message-command or draft op is correctly a
/// `Conflict`.
const SEND_OP_NAME: &str = "message.send";

#[async_trait]
impl RuntimeMailWriteApi for RuntimeHandle {
    async fn get_identity(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<Identity, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.get_identity(account_id).await
    }

    async fn list_sender_addresses(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<Vec<CachedSenderAddress>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_sender_addresses().await
    }

    async fn get_reply_context(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<ReplyContext, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .reads
            .get_reply_context(account_id, message_id)
            .await
    }

    async fn get_draft_content(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<DraftContent, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .reads
            .get_draft_content(account_id, message_id)
            .await
    }

    async fn send_message(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        request: SendMessageRequest,
        idempotency_key: Option<ClientMutationId>,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        // Keyless send keeps the pre-existing behavior: no ledger, so idempotency
        // is only M32's outbox-level exactly-once for a single operation retried
        // (the header stays optional — an absent key behaves exactly as before).
        let Some(key) = idempotency_key else {
            return self
                .core
                .authority_server_link
                .send_message(account_id, request)
                .await;
        };
        // Keyed send (RFC-L2-scripting ruling 24): honor the client
        // `Idempotency-Key` so a redelivered rule webhook / hook-serve script /
        // retried agent that calls reply/send twice under the SAME key enqueues
        // exactly ONE outbox send — a redelivery re-observes the first outcome
        // instead of creating a second operation. This reuses the SAME
        // apply-ledger the five message-command + draft routes use (D53/S4/D128),
        // keyed by `SEND_OP_NAME`; the concurrent-duplicate race is handled by the
        // ledger's existing reservation guarantee (a second in-flight request
        // under the same key gets an in-flight `Conflict`, never a second send),
        // not a new parallel mechanism.
        //
        // Composition with M32 (this does NOT duplicate it): this ledger guards
        // the HTTP boundary — one `Idempotency-Key` ⇒ one outbox operation
        // CREATED; M32's outbox-level exactly-once (deterministic
        // `phsend-<operation-id>` identity + `DispatchUncertain`) then guards
        // provider-side duplicates for that ONE operation retried. They stack:
        // key → one operation (here) → one provider submission (M32).
        match self.core.apply_ledger.reserve(&caller, &key, SEND_OP_NAME) {
            // A send carries no events, so its ledger slot is an empty
            // `CommandAck` (Ack-shaped, like the message commands — not an
            // `Operation` like the draft routes); the redelivery just needs
            // "succeeded before", which this outcome witnesses.
            Reserved::Return(result) => result
                .map(|outcome| *outcome)
                .and_then(AppliedOutcome::into_ack)
                .map(|_ack| ()),
            Reserved::Execute => {
                let result = self
                    .core
                    .authority_server_link
                    .send_message(account_id, request)
                    .await;
                // Fold the unit outcome into the ledger's `CommandAck` slot; `settle`
                // then applies D47 retention: `Confirmed` kept, a permanent rejection
                // re-observed, a transient failure cleared so a deliberate retry
                // re-executes.
                self.core.apply_ledger.settle(
                    &caller,
                    &key,
                    result
                        .as_ref()
                        .map(|()| AppliedOutcome::Ack(CommandAck { events: vec![] })),
                );
                result
            }
        }
    }

    /// Save a draft local-first, returning the enqueued operation. `draft_id` is
    /// `None` for a new draft or the existing draft's id for an edit.
    ///
    /// @spec docs/L1-outbox#operation-model
    async fn save_draft(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        draft_id: Option<MessageId>,
        idempotency_key: Option<ClientMutationId>,
        request: SendMessageRequest,
    ) -> Result<Operation, RuntimeError> {
        self.ensure_runtime_active()?;
        // Keyless save keeps the pre-existing behavior; the enqueued operation's
        // own id is the only idempotency (a redelivery duplicates the draft).
        let Some(key) = idempotency_key else {
            return self
                .core
                .authority_server_link
                .save_draft(account_id, draft_id, request)
                .await;
        };
        // Keyed save (D128): a redelivery under the same key re-observes the
        // ORIGINAL operation (its id and response), never enqueuing a second
        // draft version. `draft.save` is a distinct op-name so reusing a key for
        // a delete (or any other command) Conflicts on the op-name guard.
        match self.core.apply_ledger.reserve(&caller, &key, DRAFT_SAVE_OP) {
            Reserved::Return(result) => result
                .map(|outcome| *outcome)
                .and_then(AppliedOutcome::into_draft),
            Reserved::Execute => {
                let result = self
                    .core
                    .authority_server_link
                    .save_draft(account_id, draft_id, request)
                    .await;
                self.core.apply_ledger.settle(
                    &caller,
                    &key,
                    result.as_ref().map(|op| AppliedOutcome::Draft(op.clone())),
                );
                result
            }
        }
    }

    /// Delete a draft local-first, returning the enqueued operation.
    ///
    /// @spec docs/L1-outbox#operation-model
    async fn delete_draft(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        idempotency_key: Option<ClientMutationId>,
        draft_id: MessageId,
    ) -> Result<Operation, RuntimeError> {
        self.ensure_runtime_active()?;
        let Some(key) = idempotency_key else {
            return self
                .core
                .authority_server_link
                .delete_draft(account_id, draft_id)
                .await;
        };
        // Keyed delete (D128): idempotent under redelivery; `draft.delete` is a
        // distinct op-name from `draft.save` so cross-op key reuse Conflicts.
        match self
            .core
            .apply_ledger
            .reserve(&caller, &key, DRAFT_DELETE_OP)
        {
            Reserved::Return(result) => result
                .map(|outcome| *outcome)
                .and_then(AppliedOutcome::into_draft),
            Reserved::Execute => {
                let result = self
                    .core
                    .authority_server_link
                    .delete_draft(account_id, draft_id)
                    .await;
                self.core.apply_ledger.settle(
                    &caller,
                    &key,
                    result.as_ref().map(|op| AppliedOutcome::Draft(op.clone())),
                );
                result
            }
        }
    }

    /// List an account's non-terminal outbox operations (pending/failed work),
    /// oldest first, for optimistic hydration and pending/failed UI.
    ///
    /// @spec docs/L1-outbox#operation-model
    async fn list_pending_operations(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<Vec<Operation>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_pending_operations(account_id).await
    }

    /// Remove a queued or failed outbox operation (a user escape hatch for a
    /// dead op). In-flight operations cannot be discarded.
    async fn discard_operation(
        &self,
        _caller: RuntimeCaller,
        _account_id: AccountId,
        operation_id: OperationId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .discard_operation(operation_id)
            .await
    }

    /// Re-arm a failed outbox operation so the next flush re-attempts it.
    async fn retry_operation(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        operation_id: OperationId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .retry_operation(account_id, operation_id)
            .await
    }

    /// Direct-apply a mail operation at the authority (D21/D34). REST callers are
    /// not replicas: there is no pending set, no optimistic fold, and no
    /// `ClientMutationId` dedup on this path — the op is applied and its ack
    /// returned. Idempotency on retry is a property of the *operations* (keyword
    /// set, mailbox add/remove/replace, destroy are all state-idempotent), not of
    /// a dedup ledger; the replica path (`forward_mutation`) is where per-op
    /// idempotency lives.
    ///
    /// The op flows typed through `AuthorityServerApi::apply` (M5b): the
    /// op→command dispatch lives with the link contract
    /// (`MailCommandRequest::from_operation`), which also rejects operations
    /// that exist solely on the optimistic forward path (role moves,
    /// snooze/unsnooze, applyDiff, the `revCursor` control op) — those must
    /// flow through `forward_mutation`.
    async fn apply(
        &self,
        caller: RuntimeCaller,
        op: MailOperation,
        idempotency_key: Option<ClientMutationId>,
    ) -> Result<CommandAck, RuntimeError> {
        self.ensure_runtime_active()?;
        // Keyless direct-apply keeps the pre-existing behavior (idempotency is
        // then only the operations' inherent set-semantics — P8's residual risk).
        let Some(key) = idempotency_key else {
            return self.core.authority_server_link.apply(op).await;
        };
        // Keyed direct-apply (D53 / P8 fix): dedupe at-least-once write-back so a
        // redelivery re-observes the first outcome instead of re-executing.
        match self.core.apply_ledger.reserve(&caller, &key, op.name()) {
            Reserved::Return(result) => result
                .map(|outcome| *outcome)
                .and_then(AppliedOutcome::into_ack),
            Reserved::Execute => {
                let result = self.core.authority_server_link.apply(op).await;
                self.core.apply_ledger.settle(
                    &caller,
                    &key,
                    result.as_ref().map(|ack| AppliedOutcome::Ack(ack.clone())),
                );
                result
            }
        }
    }
}

#[async_trait]
impl RuntimeLink for RuntimeHandle {
    async fn open_link(
        &self,
        caller: RuntimeCaller,
    ) -> Result<RuntimeLinkConnection, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.links.open_link(caller)
    }

    async fn close_link(
        &self,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.links.close_link(caller, link_id)
    }

    async fn subscribe_runtime_frames(
        &self,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
        after_seq: Option<RuntimeLinkSeq>,
    ) -> Result<RuntimeFrameSubscription, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .links
            .subscribe_frames(caller, link_id, after_seq)
            .await
    }

    async fn open_link_view(
        &self,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
        descriptor: ViewDescriptor,
    ) -> Result<posthaste_contract_core::ViewSnapshot, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.links.open_view(caller, link_id, descriptor).await
    }

    async fn close_link_view(
        &self,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
        view_id: ViewId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.links.close_view(caller, link_id, view_id)
    }

    /// Grow an open windowed link view by `count` rows, returning the
    /// extended snapshot (also broadcast as a `ViewReplace` frame).
    async fn extend_link_view(
        &self,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
        view_id: ViewId,
        count: usize,
    ) -> Result<posthaste_contract_core::ViewSnapshot, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .links
            .extend_view(caller, link_id, view_id, count)
            .await
    }

    async fn forward_mutation(
        &self,
        caller: RuntimeCaller,
        request: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        self.ensure_runtime_active()?;
        let link_id = request
            .link_id
            .clone()
            .ok_or_else(|| RuntimeError::invalid_mutation("runtime mutation requires a link id"))?;
        // Undo/redo history is client-owned: an undo or redo arrives as an
        // ordinary `message.applyDiff` mutation and flows through the same
        // dispatch path as any user action — no runtime-owned history stack to
        // navigate.
        //
        // @spec docs/runtime/mutations/L1#mutation-pipeline-and-catalog
        self.dispatch_named_mutation(caller, link_id, request).await
    }

    async fn mutation_settlement(
        &self,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
        client_mutation_id: ClientMutationId,
    ) -> Result<Option<MutationReceipt>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .links
            .mutation_settlement(caller, &link_id, &client_mutation_id)
    }

    /// Mount `/v1/events` on the fact-carrying tap (RFC-L2-scripting D52 / S2):
    /// `subscribe(after_seq, filter)` → the durable **prelude** (a replay of the
    /// facts after the cursor, or the **gap frame** when the cursor fell before the
    /// log's oldest retained seq) → the **live** tail. The wire shape existing
    /// consumers parse is preserved (replay frames + live frames are `DomainEvent`s
    /// exactly as before); the gap frame is the one new element, surfaced through
    /// [`RuntimeEventSubscription::gap`].
    async fn subscribe_events(
        &self,
        _caller: RuntimeCaller,
        filter: EventFilter,
    ) -> Result<RuntimeEventSubscription, RuntimeError> {
        self.ensure_runtime_active()?;
        // Subscribe to the live broadcast BEFORE resolving the prelude so nothing
        // emitted during the prelude read is lost — the live half's cursor drops
        // anything the prelude already covered.
        let receiver = self.core.event_sender.subscribe();

        // One opaque subscriber id + a `now` tick drive the tap's reaper-managed
        // registry entry (§5.4). The cursor rides the `after_seq` slot.
        let id = TapSubscriberId(self.core.tap_subscriber_seq.fetch_add(1, Ordering::Relaxed));
        let after = filter
            .after_seq
            .filter(|seq| *seq >= 0)
            .map(|seq| seq as u64);
        let resume = self
            .core
            .event_tap
            .subscribe(&id, after, Some(filter.clone()), now_tick())
            .await
            .map_err(fact_log_error_to_runtime)?;

        let (replay, gap, replayed_through) = match resume {
            // Fresh attach: no replay. The live tail resumes strictly after the
            // head at attach (§5.3 snapshot-attach), so it never re-serves the
            // history the consumer reads via the Api as-of that seq.
            TapResume::Fresh => {
                let head = self
                    .core
                    .event_tap
                    .highest_seq()
                    .await
                    .map_err(fact_log_error_to_runtime)?;
                (Vec::new(), None, Some(head as i64))
            }
            // Durable replay: the facts after the cursor, seq-ordered.
            TapResume::Replay(frames) => {
                let last = frames.last().map(Sequenced::seq).map(|seq| seq as i64);
                let replay: Vec<DomainEvent> = frames
                    .into_iter()
                    .filter_map(sequenced_into_event)
                    .collect();
                (replay, None, last.or(filter.after_seq))
            }
            // The cursor fell before the log's oldest retained seq (§3, N8):
            // signal the gap — never a silent drop — but STILL serve the facts
            // that ARE retained after the cursor. In practice the only thing that
            // shortens `event_log` is a purge (an account deletion drops that
            // account's rows), which can move the global oldest seq without
            // touching a surviving fact like the trailing `account.deleted`;
            // dropping it would be data loss. The consumer sees the gap, then the
            // retained tail, and dedupes by id.
            TapResume::Gap { highest_seq } => {
                let mut replay_filter = filter.clone();
                replay_filter.after_seq = after.map(|seq| seq as i64).or(filter.after_seq);
                let retained = self.core.reads.replay_events(replay_filter).await?;
                let last = retained.last().map(|event| event.seq);
                (
                    retained,
                    Some(highest_seq),
                    last.or(Some(highest_seq as i64)),
                )
            }
        };

        let unsubscribe = TapUnsubscribeGuard {
            tap: self.core.event_tap.clone(),
            id,
        };
        let live = Self::live_event_stream(
            receiver,
            filter,
            self.core.reads.clone(),
            unsubscribe,
            replayed_through,
        );
        Ok(RuntimeEventSubscription { replay, gap, live })
    }
}

/// A coarse monotonic `now` tick (unix seconds) driving the tap's TTL registry.
/// The reaper compares ticks, so second-granularity is ample; the mount owns the
/// tick (the tap never reads ambient time itself).
fn now_tick() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_secs())
        .unwrap_or(0)
}

/// Map a [`FactLogError`] surfaced by the tap into a runtime error for the Api
/// boundary. `ReadOnly` cannot arise on a subscribe (read path), so both map to
/// an internal error carrying the backing message.
fn fact_log_error_to_runtime(error: posthaste_link_far_end::down::FactLogError) -> RuntimeError {
    RuntimeError::internal(format!("event tap: {error}"), None)
}

/// Lift one durable prelude frame to its `DomainEvent`; a gap/reset control
/// element carries no fact (it never appears inside a [`TapResume::Replay`], so
/// this only ever yields `Some`).
fn sequenced_into_event(frame: Sequenced<DomainEvent>) -> Option<DomainEvent> {
    match frame {
        Sequenced::Frame { frame, .. } => Some(frame),
        Sequenced::Reset { .. } => None,
    }
}

/// Drops a tap subscriber's registry entry when its SSE stream ends (§5.4). The
/// deterministic teardown the mount owns; the tap's idle reaper is the backstop
/// for a consumer that vanishes without a clean close.
struct TapUnsubscribeGuard {
    tap: Arc<EventTap>,
    id: TapSubscriberId,
}

impl Drop for TapUnsubscribeGuard {
    fn drop(&mut self) {
        self.tap.unsubscribe(&self.id);
    }
}

#[cfg(test)]
mod pending_set_lifecycle_tests {
    use super::*;
    use crate::far_end::view_registry::ViewRegistry;
    use posthaste_authority_server_link::AuthorityServerApi;
    use posthaste_contract_core::ClientMutationId;
    use posthaste_domain_model::MessageSummary;

    // A never-invoked authority-server Api half: the pending-set-lifecycle paths
    // under test touch only the link registry, never the authority server,
    // so the stub's methods are inert. (Only the reads the view registry may
    // touch get bodies; the rest inherit the erroring defaults.)
    struct NoopAuthorityServerLink;
    #[async_trait]
    impl AuthorityServerApi for NoopAuthorityServerLink {
        async fn query_mail_page(
            &self,
            _: MailQueryRequest,
        ) -> Result<MailQueryPage, RuntimeError> {
            unimplemented!("pending-set-lifecycle tests do not query")
        }
        async fn current_summary(
            &self,
            _: AccountId,
            _: MessageId,
        ) -> Result<Option<MessageSummary>, RuntimeError> {
            Ok(None)
        }
    }

    fn test_link_registry() -> Arc<LinkRegistry> {
        let event_sender = broadcast::channel(16).0;
        let pending_set = Arc::new(AuthorityServerPendingSet::new(false));
        let reads = Arc::new(ReadCache::passthrough(Arc::new(NoopAuthorityServerLink)));
        let views = Arc::new(ViewRegistry::new(event_sender.clone(), pending_set, reads));
        Arc::new(LinkRegistry::new(views, event_sender))
    }

    /// The tests' state probe over the production settlement query
    /// (`mutation_settlement`, D44b): receipt → bare settlement state.
    fn mutation_state(
        links: &Arc<LinkRegistry>,
        link_id: &RuntimeLinkId,
        client_mutation_id: &ClientMutationId,
    ) -> Option<MutationSettlementState> {
        links
            .mutation_settlement(RuntimeCaller::test(), link_id, client_mutation_id)
            .expect("settlement query succeeds")
            .map(|receipt| receipt.state)
    }

    fn accept(
        links: &Arc<LinkRegistry>,
        caller: &RuntimeCaller,
        link_id: &RuntimeLinkId,
        client_mutation_id: &ClientMutationId,
    ) -> RuntimeMutationId {
        let request: MutationRequest = serde_json::from_value(serde_json::json!({
            "linkId": link_id.as_str(),
            "name": "message.setKeywords",
            "args": {
                "sourceId": "outbox-acct",
                "messageId": "m-1",
                "command": {"add": ["$flagged"], "remove": []},
            },
            "clientMutationId": client_mutation_id.as_str(),
        }))
        .expect("request builds from the flat wire shape");
        match links.accept_mutation(caller.clone(), &request).unwrap() {
            MutationAcceptance::New { mutation_id } => mutation_id,
            MutationAcceptance::Existing(_) => panic!("expected a new mutation"),
        }
    }

    // Outbox B: a mutation whose dispatch future is cancelled (client disconnect
    // mid-forward) must still reach a terminal `Failed` settlement via the
    // drop-guard, not leak `Accepted` forever — never pruned, no terminal frame.
    // The guard is constructed + dropped by hand to stand in for the cancelled
    // `forward.await`; `run_message_mutation`'s arm/disarm wiring is reviewed
    // alongside (4 lines, documented at the call site).
    #[tokio::test]
    async fn cancelled_dispatch_guard_settles_failed_not_accepted() {
        let links = test_link_registry();
        let caller = RuntimeCaller::test();
        let link = links.open_link(caller.clone()).expect("link opens");
        let link_id = link.link_id;
        let client_mutation_id = ClientMutationId::new("cancel-cmid");
        let mutation_id = accept(&links, &caller, &link_id, &client_mutation_id);

        assert_eq!(
            mutation_state(&links, &link_id, &client_mutation_id),
            Some(MutationSettlementState::Accepted),
            "mutation is Accepted once dispatched, before any verdict"
        );

        // Simulate the dispatch future being dropped mid-await (client disconnect
        // mid-forward): the armed guard's Drop settles `Failed`.
        {
            let _guard = MutationCancelGuard {
                links: links.clone(),
                link_id: link_id.clone(),
                mutation_id,
                armed: true,
            };
        }
        assert_eq!(
            mutation_state(&links, &link_id, &client_mutation_id),
            Some(MutationSettlementState::Failed),
            "cancelled dispatch must settle Failed, not leak Accepted"
        );
    }

    // D47 (runtime seam): a transient (retryable) `Failed` settlement CLEARS the
    // dedup ledger entry, so a deliberate retry with the same `ClientMutationId`
    // re-accepts as New and re-executes. The former path kept every failure and
    // deduped the retry into the stale failure — this is the runtime-seam half of
    // D47's fix, landing in the shared dedup sub-store.
    #[tokio::test]
    async fn retryable_failure_clears_the_ledger_so_a_retry_re_executes() {
        let links = test_link_registry();
        let caller = RuntimeCaller::test();
        let link = links.open_link(caller.clone()).expect("link opens");
        let link_id = link.link_id;
        let cmid = ClientMutationId::new("retry-cmid");
        let mid = accept(&links, &caller, &link_id, &cmid);
        links
            .settle_mutation(
                &link_id,
                &mid,
                MutationSettlementState::Failed,
                Some(
                    RuntimeError::retryable(
                        RuntimeErrorCode::TransportDisconnected,
                        "link transiently down",
                    )
                    .envelope()
                    .clone(),
                ),
                serde_json::Value::Null,
            )
            .unwrap();
        assert!(
            mutation_state(&links, &link_id, &cmid).is_none(),
            "a transient failure clears the ledger entry"
        );
        // `accept` asserts the retry re-accepts as New (re-executes); it would
        // panic if the retry deduped into the cleared failure.
        let _ = accept(&links, &caller, &link_id, &cmid);
    }

    // Outbox C / D47 (permanent rejection): a non-retryable `Failed` verdict is
    // KEPT and exempt from the `Confirmed` eviction window — a rejection is
    // retired only by delivering its frame (the base never absorbs it), so a
    // disconnect-stranded client can always revert its optimistic row.
    #[tokio::test]
    async fn rejected_verdict_survives_the_confirmed_eviction_window() {
        let links = test_link_registry();
        let caller = RuntimeCaller::test();
        let link = links.open_link(caller.clone()).expect("link opens");
        let link_id = link.link_id;

        let rejected_cmid = ClientMutationId::new("rej-1");
        let rejected_mid = accept(&links, &caller, &link_id, &rejected_cmid);
        links
            .settle_mutation(
                &link_id,
                &rejected_mid,
                MutationSettlementState::Failed,
                None,
                serde_json::Value::Null,
            )
            .unwrap();

        // Bury the rejection under well over the `Confirmed` pruning cap
        // (MAX_LATEST_MUTATIONS = 100).
        for i in 0..105 {
            let cmid = ClientMutationId::new(format!("cf-{i}"));
            let mid = accept(&links, &caller, &link_id, &cmid);
            links
                .settle_mutation(
                    &link_id,
                    &mid,
                    MutationSettlementState::Confirmed,
                    None,
                    serde_json::Value::Null,
                )
                .unwrap();
        }

        assert_eq!(
            mutation_state(&links, &link_id, &rejected_cmid),
            Some(MutationSettlementState::Failed),
            "Rejected verdict must be retained across the Confirmed eviction window"
        );
    }

    // V14 follow-up knob (runtime assembly): this seam's Rejected ledger is
    // UNBOUNDED — a client link's lifetime bounds it naturally, and a
    // stranded client must always be able to re-observe every rejection verdict
    // it never saw. (The AS assembly bounds its window; see
    // `runtime_registry::tests::the_rejected_window_is_bounded_at_this_seam`.)
    #[tokio::test]
    async fn the_rejected_ledger_is_unbounded_at_the_client_seam() {
        let links = test_link_registry();
        let caller = RuntimeCaller::test();
        let link = links.open_link(caller.clone()).expect("link opens");
        let link_id = link.link_id;

        // Bury the first rejection under well over any bounded window's cap
        // (the AS seam evicts at 100).
        for i in 0..120 {
            let cmid = ClientMutationId::new(format!("rej-{i}"));
            let mid = accept(&links, &caller, &link_id, &cmid);
            links
                .settle_mutation(
                    &link_id,
                    &mid,
                    MutationSettlementState::Failed,
                    None,
                    serde_json::Value::Null,
                )
                .unwrap();
        }

        assert_eq!(
            mutation_state(&links, &link_id, &ClientMutationId::new("rej-0")),
            Some(MutationSettlementState::Failed),
            "the client seam keeps every Rejected verdict for the link's life"
        );
    }
}
