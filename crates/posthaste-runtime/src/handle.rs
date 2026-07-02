//! The runtime handle + its trait impls (D29 split from `build.rs`): the shared
//! `RuntimeCoreState`, the cloneable [`RuntimeHandle`], its inherent helpers
//! (mutation dispatch, event-stream wiring), and the five trait impls that
//! realize the two surfaces extracted from `RuntimeCore` — four
//! `posthaste-runtime-api` facets ([`RuntimeAccountApi`], [`RuntimeSettingsApi`],
//! [`RuntimeMailReadApi`], [`RuntimeMailWriteApi`]) + the [`RuntimeLink`]
//! link-protocol trait from `posthaste-client-link`. `replay_events` is dropped
//! from the public surface (zero production consumers); the runtime builds its
//! subscription backlog internally via `ReadCache::replay_events`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use posthaste_client_link::{
    RuntimeEventSubscription, RuntimeEventStream, RuntimeFrameSubscription, RuntimeLink,
    RuntimeViewSubscription,
};
use posthaste_contract_core::{
    AccountScopeRequest, AccountVerificationResult, CreateAccountMutation, MailOperation,
    MailQueryPage, MailQueryRequest, MessageResourceKind, MutationReceipt, MutationRequest,
    MutationSettlementState, PatchAccountMutation, RuntimeAccountList, RuntimeCaller,
    RuntimeError, RuntimeErrorCode, RuntimeLifecycle, RuntimeMutationId, RuntimeResourceBytes,
    RuntimeSession, RuntimeSessionId, RuntimeSessionSeq, RuntimeStatus, ViewDescriptor, ViewId,
    ViewRevision,
};
use posthaste_domain_service::{
    AccountId, AddToMailboxCommand, AppSettings, CommandAck, DomainEvent, EventFilter, MailboxId,
    MailboxSummary, MessageId, Operation, OperationId, RemoveFromMailboxCommand,
    ReplaceMailboxesCommand, SendMessageRequest, SmartMailboxId, SyncMode,
};
use posthaste_authority_server_link::AuthorityServerLinkHandle;
use posthaste_link_core::{MutationId, PendingMessageMutation};
use posthaste_runtime_api::{
    RuntimeAccountApi, RuntimeMailReadApi, RuntimeMailWriteApi, RuntimeSettingsApi,
};
use tokio::sync::broadcast;

use crate::near_node::{named_message_assertion, RuntimeAuthorityServerOutbox};
use crate::read::ReadCache;
use crate::sessions::{MutationAcceptance, SessionRegistry};
use crate::views::ViewRegistry;

/// The shared runtime core behind the cloneable handle: the authority server link, the
/// outbox, the read cache, the event bus, and the view/session registries.
pub(crate) struct RuntimeCoreState {
    // Neither the service/store nor the authority server far node is held here: every
    // authority server operation now routes through the link — `authority_server_link` for the
    // mutation up-channel and the typed write commands, `reads` for the read
    // channel. account_reads/account_supervisor are likewise not held — every
    // view (incl. AccountStatus) reads through `reads`.
    /// The runtime↔authority-server link over its (config-selected, in-process by
    /// default) transport. The mutation up-channel + typed writes go through here.
    pub(crate) authority_server_link: AuthorityServerLinkHandle,
    /// The runtime's outbox toward the authority server: forwarded-but-unconfirmed
    /// mutations, folded optimistically into served views (L4 §4.3).
    pub(crate) outbox: Arc<RuntimeAuthorityServerOutbox>,
    /// The read-through cache over the far node (W4a: passthrough). Point reads
    /// and the mail-list base draw from here.
    pub(crate) reads: Arc<ReadCache>,
    pub(crate) event_sender: broadcast::Sender<DomainEvent>,
    // The OAuth holdout: account CRUD the lean near node can't do over the link
    // yet, so it routes to the local authority server's mutation service. Present only in
    // a `authority_server`-linked build; a lean near node has no such service.
    pub(crate) views: Arc<ViewRegistry>,
    pub(crate) sessions: Arc<SessionRegistry>,
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
/// unbounded outbox leak. Disarmed on the normal settle paths so it no-ops when
/// the dispatch completes.
struct MutationCancelGuard {
    sessions: Arc<SessionRegistry>,
    session_id: RuntimeSessionId,
    mutation_id: RuntimeMutationId,
    armed: bool,
}

impl Drop for MutationCancelGuard {
    fn drop(&mut self) {
        if self.armed {
            self.sessions
                .settle_mutation(
                    &self.session_id,
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
                "mutation source is outside the runtime session account scope",
            ));
        }
        Ok(())
    }

    /// Accept a named message mutation onto the session (idempotency), forward it
    /// up the authority server link, and settle the session stream from the authority server's
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
        let session_id = request.session_id.clone().ok_or_else(|| {
            RuntimeError::invalid_mutation("runtime mutation requires a session id")
        })?;
        let mutation_id = match self.core.sessions.accept_mutation(caller, request)? {
            MutationAcceptance::New { mutation_id, .. } => mutation_id,
            MutationAcceptance::Existing(receipt) => return Ok(receipt),
        };
        // Arm the cancel-guard before awaiting the authority server: if `forward.await`
        // is dropped (client disconnect mid-dispatch), neither branch below
        // runs, and the guard's Drop settles `Failed` so the mutation doesn't
        // leak `Accepted` forever. Disarmed on each normal settle path.
        let mut guard = MutationCancelGuard {
            sessions: self.core.sessions.clone(),
            session_id: session_id.clone(),
            mutation_id: mutation_id.clone(),
            armed: true,
        };
        match forward.await {
            Ok(authority_server_receipt) => {
                guard.armed = false;
                // The authority server already serialized the command's events as the
                // receipt output (state-before-event: the effect is applied
                // before the receipt returns); settle the session with it.
                self.core.sessions.settle_mutation(
                    &session_id,
                    &mutation_id,
                    MutationSettlementState::Confirmed,
                    None,
                    authority_server_receipt.output,
                )
            }
            Err(error) => {
                guard.armed = false;
                let envelope = error.envelope().clone();
                self.core.sessions.settle_mutation(
                    &session_id,
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

    fn live_event_stream(
        mut receiver: broadcast::Receiver<DomainEvent>,
        filter: EventFilter,
        replayed_through: Option<i64>,
    ) -> RuntimeEventStream {
        let stream = async_stream::stream! {
            loop {
                match receiver.recv().await {
                    Ok(event)
                        if replayed_through.is_none_or(|seq| event.seq > seq)
                            && Self::event_matches_filter(&event, &filter) =>
                    {
                        yield event;
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        };
        stream.boxed()
    }

    /// Route a single named message mutation through the shared accept → forward
    /// → settle flow, folding its optimistic assertion into the outbox, and — for
    /// a diff-eligible user mutation — record the invertible change-diff onto the
    /// session's undo history once the authority server confirms it. `message.applyDiff`
    /// (undo/redo) goes through [`run_apply_diff`] instead, which wraps this flow
    /// with history navigation.
    ///
    /// @spec docs/runtime/mutations/L1#mutation-pipeline-and-catalog
    async fn dispatch_named_mutation(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        request: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        let session_scope = self
            .core
            .sessions
            .session_scope(&session_id, caller.account_scope.as_deref())?;
        // Phase 2: `revCursor` is a control operation (not a message mutation) —
        // it carries no message target + has no outbox optimism. Route it
        // directly to the authority server (which validates + applies the cursor).
        // The typed variant carries `RevCursorArgs`, so there is no per-site arg
        // re-parse (D22).
        // @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
        if let MailOperation::RevCursor(args) = &request.operation {
            let account_id = args.account_id.clone();
            return self
                .dispatch_rev_cursor(caller, session_scope.as_deref(), account_id, request)
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
        Self::ensure_account_in_scope(&source_id, session_scope.as_deref())?;
        // Accept the mutation into the runtime's outbox toward the authority server so
        // recomputed views fold it optimistically while it is in flight. It is
        // settled from the receipt below: co-located it retires on receipt (the
        // forward confirms synchronously, so the outbox is empty between
        // mutations and the overlay is a pass-through, `colocated-unchanged`);
        // remote it retires by absorption when the down-channel base assertion
        // arrives, so a receipt that outruns the `message.updated` propagation
        // does not recompute against a stale base (the near-node flicker).
        let optimistic = named_message_assertion(&request).map(|(message_id, assertion)| {
            let id = MutationId(request.client_mutation_id.as_str().to_string());
            self.core.outbox.accept(PendingMessageMutation {
                id: id.clone(),
                key: message_id,
                effect: assertion,
            });
            id
        });
        // Up-channel: forward the named mutation to the authority server far node.
        let forward = self.core.authority_server_link.forward_mutation(request.clone());
        let result = self.run_message_mutation(caller, &request, forward).await;
        if let Some(id) = optimistic {
            // An authority server rejection settles as `Ok(receipt)` carrying a `Failed`
            // state (the verdict is on `error.code`), so the confirm signal is
            // the receipt state, not `is_ok()`.
            let confirmed = matches!(
                &result,
                Ok(receipt) if receipt.state == MutationSettlementState::Confirmed
            );
            self.core.outbox.settle_receipt(&id, confirmed);
        }
        result
    }

    /// Phase 2: route a `revCursor` control operation to the authority server.
    /// The account (for the scope check) is read from the already-typed
    /// `RevCursorArgs` — no per-site arg re-parse (D22) — then the request is
    /// forwarded through the normal accept → forward → settle flow (no outbox
    /// optimism — a cursor move has no message assertion to fold). The authority
    /// server validates the referenced steps exist + applies the cursor + emits
    /// the recompute trigger.
    ///
    /// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
    async fn dispatch_rev_cursor(
        &self,
        caller: RuntimeCaller,
        session_scope: Option<&[String]>,
        account_id: String,
        request: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        Self::ensure_account_in_scope(&account_id, session_scope)?;
        let forward = self.core.authority_server_link.forward_mutation(request.clone());
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
    ) -> Result<posthaste_domain_service::AccountOverview, RuntimeError> {
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
    ) -> Result<posthaste_domain_service::AccountOverview, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.authority_server_link.create_account(mutation).await
    }

    async fn patch_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        mutation: PatchAccountMutation,
    ) -> Result<posthaste_domain_service::AccountOverview, RuntimeError> {
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
        self.core.authority_server_link.delete_account(account_id).await
    }

    async fn verify_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<AccountVerificationResult, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.authority_server_link.verify_account(account_id).await
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
        self.core.authority_server_link.sync_account(account_id, mode).await
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
        self.core.authority_server_link.patch_app_settings(mutation).await
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
    ) -> Result<
        std::collections::BTreeMap<AccountId, Vec<posthaste_domain_service::MailboxSummary>>,
        RuntimeError,
    > {
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

    async fn list_smart_mailboxes(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<Vec<posthaste_domain_service::SmartMailboxSummary>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_smart_mailboxes().await
    }

    async fn get_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<posthaste_domain_service::SmartMailbox, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.get_smart_mailbox(smart_mailbox_id).await
    }

    async fn create_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        mutation: posthaste_contract_core::CreateSmartMailboxMutation,
    ) -> Result<posthaste_domain_service::SmartMailbox, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.authority_server_link.create_smart_mailbox(mutation).await
    }

    async fn patch_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
        mutation: posthaste_contract_core::PatchSmartMailboxMutation,
    ) -> Result<posthaste_domain_service::SmartMailbox, RuntimeError> {
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
    ) -> Result<Vec<posthaste_domain_service::SmartMailboxSummary>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.authority_server_link.reset_default_smart_mailboxes().await
    }

    async fn list_tags(
        &self,
        _caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<Vec<posthaste_domain_service::TagSummary>, RuntimeError> {
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
    ) -> Result<posthaste_domain_service::CommandResult, RuntimeError> {
        self.ensure_runtime_active()?;
        // Body-free: the detail read serves header + cached attachments only and
        // never loads the body (it is the separate `/body` lazy resource), so
        // opening a message neither provider-fetches nor materializes the body.
        let detail = self
            .core
            .reads
            .message_detail(&account_id, &message_id)
            .await?;
        Ok(posthaste_domain_service::CommandResult {
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

#[async_trait]
impl RuntimeMailWriteApi for RuntimeHandle {
    async fn get_identity(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<posthaste_domain_service::Identity, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.get_identity(account_id).await
    }

    async fn list_sender_addresses(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<Vec<posthaste_domain_service::CachedSenderAddress>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_sender_addresses().await
    }

    async fn get_reply_context(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<posthaste_domain_service::ReplyContext, RuntimeError> {
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
    ) -> Result<posthaste_domain_service::DraftContent, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .reads
            .get_draft_content(account_id, message_id)
            .await
    }

    async fn send_message(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        request: SendMessageRequest,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .send_message(account_id, request)
            .await
    }

    /// Save a draft local-first, returning the enqueued operation. `draft_id` is
    /// `None` for a new draft or the existing draft's id for an edit.
    ///
    /// @spec docs/L1-outbox#operation-model
    async fn save_draft(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        draft_id: Option<MessageId>,
        request: SendMessageRequest,
    ) -> Result<Operation, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .save_draft(account_id, draft_id, request)
            .await
    }

    /// Delete a draft local-first, returning the enqueued operation.
    ///
    /// @spec docs/L1-outbox#operation-model
    async fn delete_draft(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        draft_id: MessageId,
    ) -> Result<Operation, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .authority_server_link
            .delete_draft(account_id, draft_id)
            .await
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
        self.core.authority_server_link.discard_operation(operation_id).await
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
    /// not replicas: there is no outbox, no optimistic fold, and no
    /// `ClientMutationId` dedup on this path — the op is applied and its ack
    /// returned. Idempotency on retry is a property of the *operations* (keyword
    /// set, mailbox add/remove/replace, destroy are all state-idempotent), not of
    /// a dedup ledger; the replica path (`forward_mutation`) is where per-op
    /// idempotency lives.
    ///
    /// Only the typed command subset the REST surface emits is dispatched here;
    /// operations that exist solely on the optimistic forward path (role moves,
    /// snooze/unsnooze, applyDiff, the `revCursor` control op) have no direct
    /// authority command and are rejected — they must flow through
    /// `forward_mutation`.
    async fn apply(
        &self,
        _caller: RuntimeCaller,
        op: MailOperation,
    ) -> Result<CommandAck, RuntimeError> {
        self.ensure_runtime_active()?;
        let account = AccountId(op.account_id().to_string());
        let message = op
            .message_id()
            .map(|id| MessageId(id.to_string()))
            .ok_or_else(|| {
                RuntimeError::invalid_mutation(format!(
                    "operation '{}' has no direct-apply command surface",
                    op.name()
                ))
            })?;
        match op {
            MailOperation::SetKeywords(args) => {
                self.core
                    .authority_server_link
                    .set_keywords(account, message, args.command)
                    .await
            }
            MailOperation::ReplaceMailboxes(args) => {
                self.core
                    .authority_server_link
                    .replace_mailboxes(
                        account,
                        message,
                        ReplaceMailboxesCommand {
                            mailbox_ids: args.mailbox_ids.into_iter().map(MailboxId).collect(),
                        },
                    )
                    .await
            }
            MailOperation::AddToMailbox(args) => {
                self.core
                    .authority_server_link
                    .add_to_mailbox(
                        account,
                        message,
                        AddToMailboxCommand {
                            mailbox_id: MailboxId(args.mailbox_id),
                        },
                    )
                    .await
            }
            MailOperation::RemoveFromMailbox(args) => {
                self.core
                    .authority_server_link
                    .remove_from_mailbox(
                        account,
                        message,
                        RemoveFromMailboxCommand {
                            mailbox_id: MailboxId(args.mailbox_id),
                        },
                    )
                    .await
            }
            MailOperation::Destroy(_) => {
                self.core
                    .authority_server_link
                    .destroy_message(account, message)
                    .await
            }
            other => Err(RuntimeError::invalid_mutation(format!(
                "operation '{}' has no direct-apply command surface; forward it as a mutation",
                other.name()
            ))),
        }
    }
}

#[async_trait]
impl RuntimeLink for RuntimeHandle {
    async fn open_session(&self, caller: RuntimeCaller) -> Result<RuntimeSession, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.sessions.open_session(caller)
    }

    async fn close_session(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.sessions.close_session(caller, session_id)
    }

    async fn subscribe_runtime_frames(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        after_seq: Option<RuntimeSessionSeq>,
    ) -> Result<RuntimeFrameSubscription, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .sessions
            .subscribe_frames(caller, session_id, after_seq)
            .await
    }

    async fn open_session_view(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        descriptor: ViewDescriptor,
    ) -> Result<posthaste_contract_core::ViewSnapshot, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .sessions
            .open_view(caller, session_id, descriptor)
            .await
    }

    async fn close_session_view(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        view_id: ViewId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.sessions.close_view(caller, session_id, view_id)
    }

    /// Grow an open windowed session view by `count` rows, returning the
    /// extended snapshot (also broadcast as a `ViewReplace` frame).
    async fn extend_session_view(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        view_id: ViewId,
        count: usize,
    ) -> Result<posthaste_contract_core::ViewSnapshot, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .sessions
            .extend_view(caller, session_id, view_id, count)
            .await
    }

    async fn forward_mutation(
        &self,
        caller: RuntimeCaller,
        request: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        self.ensure_runtime_active()?;
        let session_id = request.session_id.clone().ok_or_else(|| {
            RuntimeError::invalid_mutation("runtime mutation requires a session id")
        })?;
        // Undo/redo history is client-owned: an undo or redo arrives as an
        // ordinary `message.applyDiff` mutation and flows through the same
        // dispatch path as any user action — no runtime-owned history stack to
        // navigate.
        //
        // @spec docs/runtime/mutations/L1#mutation-pipeline-and-catalog
        self.dispatch_named_mutation(caller, session_id, request)
            .await
    }

    async fn open_view(
        &self,
        caller: RuntimeCaller,
        descriptor: ViewDescriptor,
    ) -> Result<posthaste_contract_core::ViewSnapshot, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .views
            .open_view(descriptor, caller.account_scope.as_deref())
            .await
    }

    async fn subscribe_view(
        &self,
        caller: RuntimeCaller,
        view_id: ViewId,
        after_revision: Option<ViewRevision>,
    ) -> Result<RuntimeViewSubscription, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .views
            .subscribe_view(view_id, after_revision, caller.account_scope.as_deref())
    }

    async fn subscribe_events(
        &self,
        _caller: RuntimeCaller,
        filter: EventFilter,
    ) -> Result<RuntimeEventSubscription, RuntimeError> {
        self.ensure_runtime_active()?;
        let receiver = self.core.event_sender.subscribe();
        let replay = if filter.after_seq.is_some() {
            // `replay_events` was dropped from the public trait (zero production
            // consumers); the runtime builds its subscription backlog internally
            // via `ReadCache::replay_events` (the private fn that stays).
            self.core
                .reads
                .replay_events(filter.clone())
                .await?
                .into_iter()
                .filter(|event| Self::event_matches_filter(event, &filter))
                .collect()
        } else {
            Vec::new()
        };
        let replayed_through = replay.last().map(|event| event.seq).or(filter.after_seq);
        let live = Self::live_event_stream(receiver, filter, replayed_through);
        Ok(RuntimeEventSubscription { replay, live })
    }
}

#[cfg(test)]
mod outbox_lifecycle_tests {
    use super::*;
    use posthaste_contract_core::ClientMutationId;
    use posthaste_domain_service::MessageSummary;
    use posthaste_authority_server_link::{AuthorityServerLink, DownStream, LinkCoverage};

    // A never-invoked `AuthorityServerLink`: the outbox-lifecycle paths under test touch
    // only the session registry, never the authority server, so the stub's methods are
    // inert. (Only the 4 non-defaulted trait methods need bodies; the rest
    // inherit defaults.)
    struct NoopAuthorityServerLink;
    #[async_trait]
    impl AuthorityServerLink for NoopAuthorityServerLink {
        async fn forward_mutation(
            &self,
            _: MutationRequest,
        ) -> Result<MutationReceipt, RuntimeError> {
            unimplemented!("outbox-lifecycle tests do not dispatch")
        }
        async fn subscribe(&self, _: LinkCoverage) -> Result<DownStream, RuntimeError> {
            Ok(Box::pin(futures_util::stream::empty()))
        }
        async fn query_mail_page(
            &self,
            _: MailQueryRequest,
        ) -> Result<MailQueryPage, RuntimeError> {
            unimplemented!("outbox-lifecycle tests do not query")
        }
        async fn current_summary(
            &self,
            _: AccountId,
            _: MessageId,
        ) -> Result<Option<MessageSummary>, RuntimeError> {
            Ok(None)
        }
    }

    fn test_session_registry() -> Arc<SessionRegistry> {
        let event_sender = broadcast::channel(16).0;
        let outbox = Arc::new(RuntimeAuthorityServerOutbox::new(false));
        let reads = Arc::new(ReadCache::passthrough(Arc::new(NoopAuthorityServerLink)));
        let views = Arc::new(ViewRegistry::new(event_sender.clone(), outbox, reads));
        Arc::new(SessionRegistry::new(views, event_sender))
    }

    fn accept(
        sessions: &Arc<SessionRegistry>,
        caller: &RuntimeCaller,
        session_id: &RuntimeSessionId,
        client_mutation_id: &ClientMutationId,
    ) -> RuntimeMutationId {
        let request: MutationRequest = serde_json::from_value(serde_json::json!({
            "sessionId": session_id.as_str(),
            "name": "message.setKeywords",
            "args": {
                "sourceId": "outbox-acct",
                "messageId": "m-1",
                "command": {"add": ["$flagged"], "remove": []},
            },
            "clientMutationId": client_mutation_id.as_str(),
        }))
        .expect("request builds from the flat wire shape");
        match sessions.accept_mutation(caller.clone(), &request).unwrap() {
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
        let sessions = test_session_registry();
        let caller = RuntimeCaller::test();
        let session = sessions
            .open_session(caller.clone())
            .expect("session opens");
        let session_id = session.session_id;
        let client_mutation_id = ClientMutationId::new("cancel-cmid");
        let mutation_id = accept(&sessions, &caller, &session_id, &client_mutation_id);

        assert_eq!(
            sessions.mutation_state(&session_id, &client_mutation_id),
            Some(MutationSettlementState::Accepted),
            "mutation is Accepted once dispatched, before any verdict"
        );

        // Simulate the dispatch future being dropped mid-await (client disconnect
        // mid-forward): the armed guard's Drop settles `Failed`.
        {
            let _guard = MutationCancelGuard {
                sessions: sessions.clone(),
                session_id: session_id.clone(),
                mutation_id,
                armed: true,
            };
        }
        assert_eq!(
            sessions.mutation_state(&session_id, &client_mutation_id),
            Some(MutationSettlementState::Failed),
            "cancelled dispatch must settle Failed, not leak Accepted"
        );
    }

    // Outbox C: a `Failed` (Rejected) verdict is retired only by delivering its
    // frame — the base never absorbs a rejection — so it must NOT be evicted by
    // the `Confirmed` pruning cap. Otherwise a disconnect-stranded client never
    // reverts its optimistic row (no recovery path).
    #[tokio::test]
    async fn rejected_verdict_survives_the_confirmed_eviction_window() {
        let sessions = test_session_registry();
        let caller = RuntimeCaller::test();
        let session = sessions
            .open_session(caller.clone())
            .expect("session opens");
        let session_id = session.session_id;

        let rejected_cmid = ClientMutationId::new("rej-1");
        let rejected_mid = accept(&sessions, &caller, &session_id, &rejected_cmid);
        sessions
            .settle_mutation(
                &session_id,
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
            let mid = accept(&sessions, &caller, &session_id, &cmid);
            sessions
                .settle_mutation(
                    &session_id,
                    &mid,
                    MutationSettlementState::Confirmed,
                    None,
                    serde_json::Value::Null,
                )
                .unwrap();
        }

        assert_eq!(
            sessions.mutation_state(&session_id, &rejected_cmid),
            Some(MutationSettlementState::Failed),
            "Rejected verdict must be retained across the Confirmed eviction window"
        );
    }
}
