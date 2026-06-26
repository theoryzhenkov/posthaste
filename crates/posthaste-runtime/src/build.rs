use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use posthaste_domain::{
    AccountId, AddToMailboxCommand, AppSettings, ConfigError, DomainEvent, EventFilter, MailboxId,
    MailboxSummary, MessageId, Operation, OperationId, RemoveFromMailboxCommand,
    ReplaceMailboxesCommand, SecretStore, SendMessageRequest, ServiceError, SetKeywordsCommand,
    SmartMailboxId, StoreError, SyncMode,
};
use posthaste_link_contract::{BackendApi, BackendLink};
use posthaste_link_core::{
    MessageChangeDiff, MessageFoldState, MutationId, PendingMessageMutation,
};
use posthaste_runtime_contract::{
    AccountScopeRequest, AccountVerificationResult, CreateAccountMutation, MailQueryPage,
    MailQueryRequest, MessageResourceKind, MutationReceipt, MutationRequest,
    MutationSettlementState, PatchAccountMutation, RuntimeAccountList, RuntimeCaller, RuntimeCore,
    RuntimeError, RuntimeErrorCode, RuntimeEventSubscription, RuntimeFrameSubscription,
    RuntimeLifecycle, RuntimeResourceBytes, RuntimeSession, RuntimeSessionId, RuntimeSessionSeq,
    RuntimeStatus, RuntimeStoreStatus, RuntimeViewSubscription, ViewDescriptor, ViewId,
    ViewRevision,
};
use thiserror::Error;
use tokio::sync::broadcast;

use crate::mutation_args::{
    parse_args, MessageApplyDiffArgs, MessageMoveToMailboxArgs, MessageMoveToRoleArgs,
    MessageReplaceMailboxesArgs, MessageSetFlaggedStateArgs, MessageSetKeywordsMutationArgs,
    MessageSetReadStateArgs, MessageSetUserTagsArgs, MessageTargetArgs,
};
use crate::near_node::{named_message_assertion, RuntimeBackendOutbox};
use crate::read::ReadCache;
use crate::secret::SystemSecretStore;
use crate::sessions::{MutationAcceptance, SessionRegistry};
use crate::transport::RemoteBackend;
use crate::views::ViewRegistry;

const DEFAULT_EVENT_CHANNEL_CAPACITY: usize = 512;

/// Transport-free build inputs for the local authority runtime.
///
/// Roots are resolved by the host before construction so the runtime owns mail
/// authority state without depending on renderer storage.
///
/// spec: docs/runtime/internals/L2#runtime-builder-transport-free
/// spec: docs/runtime/internals/L1#runtime-owned-roots
pub struct RuntimeBuildConfig {
    pub config_root: PathBuf,
    pub state_root: PathBuf,
    pub cache_root: PathBuf,
    pub bootstrap_path: Option<PathBuf>,
    pub secret_store: Option<Arc<dyn SecretStore>>,
    pub event_channel_capacity: usize,
    pub poll_interval: Duration,
    /// Which transport carries the runtime↔backend link ([replication backend-link L2 §6](../replication/backend-link/L2.md)).
    /// Chosen from configuration, not at build time; the default is in-process
    /// co-located (assertion `transport-selected-by-config`).
    pub backend_transport: BackendTransportConfig,
    /// A decorator over the config-selected link transport. When set, the
    /// builder hands it the real (in-process or remote) [`BackendApi`] and uses
    /// what it returns. A host/test seam for *composing over* the transport
    /// (e.g. gating the up-channel to exercise the near-node outbox) without
    /// replacing the full backend surface — the decorator delegates everything
    /// it does not intercept to the inner transport. `None` in normal builds.
    pub backend_transport_override: Option<BackendTransportDecorator>,
}

/// A decorator over the config-selected link transport (see
/// [`RuntimeBuildConfig::backend_transport_override`]): receives the
/// real [`BackendApi`] and returns a wrapping one. Composes, so it need not
/// re-implement the whole surface — only the methods it intercepts.
pub type BackendTransportDecorator =
    Box<dyn FnOnce(Arc<dyn BackendApi>) -> Arc<dyn BackendApi> + Send>;

/// The runtime↔backend link transport, selected by configuration.
///
/// `InProcess` (default) is the co-located far node — zero serialization, byte
/// for byte the pre-link behavior. `Remote` points the link at a backend that
/// serves the link wire (POST up + SSE down) elsewhere; switching is a config
/// change, not a rebuild ([replication backend-link L2 §6](../replication/backend-link/L2.md)).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BackendTransportConfig {
    #[default]
    InProcess,
    Remote {
        base_url: String,
        /// Bearer token presented to the backend's authenticated `link_router`
        /// (`LinkAuth::Bearer`). `None` for an unauthenticated link.
        token: Option<String>,
    },
}

impl RuntimeBuildConfig {
    pub fn new(
        config_root: impl Into<PathBuf>,
        state_root: impl Into<PathBuf>,
        cache_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            config_root: config_root.into(),
            state_root: state_root.into(),
            cache_root: cache_root.into(),
            bootstrap_path: None,
            secret_store: None,
            event_channel_capacity: DEFAULT_EVENT_CHANNEL_CAPACITY,
            poll_interval: Duration::from_secs(60),
            backend_transport: BackendTransportConfig::InProcess,
            backend_transport_override: None,
        }
    }

    /// Select the runtime↔backend link transport (default in-process).
    pub fn with_backend_transport(mut self, backend_transport: BackendTransportConfig) -> Self {
        self.backend_transport = backend_transport;
        self
    }

    /// Decorate the config-selected link transport (see
    /// [`backend_transport_override`](Self::backend_transport_override)). The
    /// closure receives the real transport and returns the one the link uses.
    pub fn with_backend_transport_override(
        mut self,
        decorator: impl FnOnce(Arc<dyn BackendApi>) -> Arc<dyn BackendApi> + Send + 'static,
    ) -> Self {
        self.backend_transport_override = Some(Box::new(decorator));
        self
    }

    pub fn with_bootstrap_path(mut self, bootstrap_path: impl Into<PathBuf>) -> Self {
        self.bootstrap_path = Some(bootstrap_path.into());
        self
    }

    pub fn with_bootstrap_path_option(mut self, bootstrap_path: Option<PathBuf>) -> Self {
        self.bootstrap_path = bootstrap_path;
        self
    }

    pub fn with_secret_store(mut self, secret_store: Arc<dyn SecretStore>) -> Self {
        self.secret_store = Some(secret_store);
        self
    }

    pub fn with_event_channel_capacity(mut self, event_channel_capacity: usize) -> Self {
        self.event_channel_capacity = event_channel_capacity;
        self
    }

    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }
}

/// Result of building the authority runtime.
///
/// spec: docs/runtime/internals/L1#runtime-handle-transport-neutral
pub struct RemoteRuntimeBuild {
    pub handle: RuntimeHandle,
    pub shutdown: RuntimeShutdownHandle,
    pub runtime_status: RuntimeStatus,
    /// The runtime's own secret store, for its `/v1` client auth.
    pub secret_store: Arc<dyn SecretStore>,
}

/// Build a backend-less runtime near node over a remote backend link. Requires
/// [`BackendTransportConfig::Remote`] (a near node has no in-process backend to
/// fall back to). Must run within a Tokio runtime: it spawns the down-channel
/// bridge that keeps the read cache + views live from the backend's assertions.
pub fn build_remote_runtime(
    config: RuntimeBuildConfig,
) -> Result<RemoteRuntimeBuild, RuntimeBuildError> {
    if config.event_channel_capacity == 0 {
        return Err(RuntimeBuildError::InvalidConfig(
            "event_channel_capacity must be greater than zero".to_string(),
        ));
    }
    let RuntimeBuildConfig {
        secret_store,
        event_channel_capacity,
        backend_transport,
        backend_transport_override,
        ..
    } = config;

    let (base_url, token) = match backend_transport {
        BackendTransportConfig::Remote { base_url, token } => (base_url, token),
        BackendTransportConfig::InProcess => {
            return Err(RuntimeBuildError::InvalidConfig(
                "a remote runtime requires a remote backend transport".to_string(),
            ));
        }
    };

    let secret_store = secret_store.unwrap_or_else(|| Arc::new(SystemSecretStore));
    let (event_sender, _) = broadcast::channel(event_channel_capacity);

    // The link transport is the remote backend (optionally decorated by a test
    // seam); there is no in-process far node to fall back to.
    let base: Arc<dyn BackendApi> = Arc::new(RemoteBackend::with_token(base_url, token));
    let transport = match backend_transport_override {
        Some(decorate) => decorate(base),
        None => base,
    };
    let backend_link = BackendLink::new(transport);
    let reads = Arc::new(ReadCache::retaining(backend_link.transport().clone()));

    // No local store; the live account count comes through the link on the
    // `runtime_status` read.
    let runtime_status = RuntimeStatus {
        lifecycle: RuntimeLifecycle::Ready,
        store: RuntimeStoreStatus {
            config_loaded: true,
            state_store_open: false,
            cache_root_ready: false,
        },
        account_count: 0,
    };

    // A near node drives its cache + views from the backend down-channel (it has
    // no local event bus of its own).
    let composed = assemble_runtime(RuntimeAssembly {
        backend_link,
        reads,
        event_sender,
        startup_status: runtime_status.clone(),
        drive_down_channel: true,
    });

    Ok(RemoteRuntimeBuild {
        handle: composed.handle,
        shutdown: composed.shutdown,
        runtime_status,
        secret_store,
    })
}

/// Inputs for [`assemble_runtime`]: a link to the backend plus the read cache
/// over it. The far-node crate builds these around an in-process `LocalBackend`;
/// [`build_remote_runtime`] builds them around a [`RemoteBackend`].
pub struct RuntimeAssembly {
    /// The runtime↔backend link over its (config-selected) transport.
    pub backend_link: BackendLink,
    /// The read-through cache over the same transport the link uses.
    pub reads: Arc<ReadCache>,
    /// The runtime's domain-event bus. In-process this is the backend's bus; a
    /// remote near node owns its own and the down-channel republishes onto it.
    pub event_sender: broadcast::Sender<DomainEvent>,
    /// The startup status snapshot the handle reports until live reads layer on.
    pub startup_status: RuntimeStatus,
    /// Spawn the backend down-channel bridge (a remote near node: evict on
    /// assertions and republish so views recompute). In-process the runtime
    /// shares the backend's bus, so no bridge is needed.
    pub drive_down_channel: bool,
}

/// The handle + shutdown produced by [`assemble_runtime`].
pub struct ComposedRuntime {
    pub handle: RuntimeHandle,
    pub shutdown: RuntimeShutdownHandle,
}

/// Assemble a runtime near node over a backend link: the outbox, view/session
/// registries, and the handle. The far-node crate calls this to compose an
/// in-process runtime over a `LocalBackend`; [`build_remote_runtime`] calls it
/// over a [`RemoteBackend`]. Must run within a Tokio runtime when
/// `drive_down_channel` is set (it spawns the down-channel bridge).
pub fn assemble_runtime(assembly: RuntimeAssembly) -> ComposedRuntime {
    let RuntimeAssembly {
        backend_link,
        reads,
        event_sender,
        startup_status,
        drive_down_channel,
    } = assembly;

    let stopped = Arc::new(AtomicBool::new(false));
    let outbox = Arc::new(RuntimeBackendOutbox::new());
    if drive_down_channel {
        tokio::spawn(crate::read::run_backend_down_channel(
            backend_link.clone(),
            reads.clone(),
            event_sender.clone(),
        ));
    }
    let views = Arc::new(ViewRegistry::new(
        event_sender.clone(),
        outbox.clone(),
        reads.clone(),
    ));
    let sessions = Arc::new(SessionRegistry::new(views.clone(), event_sender.clone()));
    let core = Arc::new(RuntimeCoreState {
        backend_link,
        outbox,
        reads,
        event_sender,
        views,
        sessions,
        startup_status,
        stopped: stopped.clone(),
    });

    ComposedRuntime {
        handle: RuntimeHandle { core },
        shutdown: RuntimeShutdownHandle { stopped },
    }
}

/// The shared runtime core behind the cloneable handle: the backend link, the
/// outbox, the read cache, the event bus, and the view/session registries.
struct RuntimeCoreState {
    // Neither the service/store nor the backend far node is held here: every
    // backend operation now routes through the link — `backend_link` for the
    // mutation up-channel and the typed write commands, `reads` for the read
    // channel. account_reads/account_supervisor are likewise not held — every
    // view (incl. AccountStatus) reads through `reads`.
    /// The runtime↔backend link over its (config-selected, in-process by
    /// default) transport. The mutation up-channel + typed writes go through here.
    backend_link: BackendLink,
    /// The runtime's outbox toward the backend: forwarded-but-unconfirmed
    /// mutations, folded optimistically into served views (L4 §4.3).
    outbox: Arc<RuntimeBackendOutbox>,
    /// The read-through cache over the far node (W4a: passthrough). Point reads
    /// and the mail-list base draw from here.
    reads: Arc<ReadCache>,
    event_sender: broadcast::Sender<DomainEvent>,
    // The OAuth holdout: account CRUD the lean near node can't do over the link
    // yet, so it routes to the local backend's mutation service. Present only in
    // a `backend`-linked build; a lean near node has no such service.
    views: Arc<ViewRegistry>,
    sessions: Arc<SessionRegistry>,
    startup_status: RuntimeStatus,
    stopped: Arc<AtomicBool>,
}

/// Cloneable authority runtime handle used by transport adapters.
///
/// spec: docs/runtime/internals/L1#runtime-handle-transport-neutral
/// spec: docs/backend/L2#handle-methods-transport-free
#[derive(Clone)]
pub struct RuntimeHandle {
    core: Arc<RuntimeCoreState>,
}

impl RuntimeHandle {
    /// MIGRATION(api-runtime-wrapper): create a runtime handle around existing
    /// test/API parts until all router state is produced by the authority
    /// runtime builder.
    ///
    /// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#appstate-has-runtime-handle
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
    /// up the backend link, and settle the session stream from the backend's
    /// receipt. The `forward` future is the link's up-channel
    /// (`BackendLink::forward_mutation`); its receipt carries the command's
    /// events as `output` and the backend's confirmation id. Scope is the
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
        match forward.await {
            Ok(backend_receipt) => {
                // The backend already serialized the command's events as the
                // receipt output (state-before-event: the effect is applied
                // before the receipt returns); settle the session with it.
                self.core.sessions.settle_mutation(
                    &session_id,
                    &mutation_id,
                    MutationSettlementState::Confirmed,
                    None,
                    backend_receipt.output,
                )
            }
            Err(error) => {
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

    /// Read a message's current fold state (keywords + mailbox membership)
    /// straight through the backend, bypassing the summary cache. Used to capture
    /// a mutation's before/after diff: the cache is evicted by the down-channel,
    /// but that eviction races the post-apply read, so a diff read must not serve
    /// a stale pre-apply entry. `None` when the message is not held.
    async fn read_fold_state(
        &self,
        source_id: &str,
        message_id: &str,
    ) -> Result<Option<MessageFoldState>, RuntimeError> {
        Ok(self
            .core
            .reads
            .fresh_summary(
                &AccountId(source_id.to_string()),
                &MessageId(message_id.to_string()),
            )
            .await?
            .map(|summary| MessageFoldState {
                keywords: summary.keywords,
                mailbox_ids: summary
                    .mailbox_ids
                    .into_iter()
                    .map(|id| id.as_str().to_string())
                    .collect(),
            }))
    }

    /// The invertible change-diff a confirmed mutation produced, from a
    /// before/after read of the message's fold state. `None` when the before or
    /// after state is unavailable (the message is not held). An empty diff (a
    /// no-op mutation) is returned so the caller can skip recording it.
    async fn capture_diff(
        &self,
        source_id: &str,
        message_id: &str,
        before: Option<MessageFoldState>,
    ) -> Result<Option<MessageChangeDiff>, RuntimeError> {
        let Some(before) = before else {
            return Ok(None);
        };
        let Some(after) = self.read_fold_state(source_id, message_id).await? else {
            return Ok(None);
        };
        Ok(Some(MessageChangeDiff::from_before_after(&before, &after)))
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
    ) -> posthaste_runtime_contract::RuntimeEventStream {
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
    /// session's undo history once the backend confirms it. `message.applyDiff`
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
        // Runtime (near-node) concern: scope enforcement per mutation. The
        // command application is the backend's; it is forwarded up the link
        // below, uniform across names. `diff_eligible` marks mutations whose
        // change-diff is recorded for undo (destroy is non-invertible; applyDiff
        // is the undo/redo replay and never records a fresh diff).
        let (source_id, message_id, diff_eligible) = match request.name.as_str() {
            "message.setKeywords" => {
                let args: MessageSetKeywordsMutationArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                (args.source_id, args.message_id, true)
            }
            "message.setReadState" => {
                let args: MessageSetReadStateArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                (args.source_id, args.message_id, true)
            }
            "message.setFlaggedState" => {
                let args: MessageSetFlaggedStateArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                (args.source_id, args.message_id, true)
            }
            "message.setUserTags" => {
                let args: MessageSetUserTagsArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                (args.source_id, args.message_id, true)
            }
            "message.moveToMailbox" => {
                let args: MessageMoveToMailboxArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                (args.source_id, args.message_id, true)
            }
            "message.replaceMailboxes" => {
                let args: MessageReplaceMailboxesArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                (args.source_id, args.message_id, true)
            }
            "message.moveToRole" => {
                let args: MessageMoveToRoleArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                (args.source_id, args.message_id, true)
            }
            "message.archive" | "message.trash" | "message.restoreToInbox" => {
                let args: MessageTargetArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                (args.source_id, args.message_id, true)
            }
            "message.destroy" => {
                let args: MessageTargetArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                // Destroy is the one non-invertible message mutation.
                (args.source_id, args.message_id, false)
            }
            "message.applyDiff" => {
                let args: MessageApplyDiffArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                // applyDiff is the undo/redo vehicle; its history navigation is
                // owned by `run_apply_diff`. It never records a fresh diff.
                (args.source_id, args.message_id, false)
            }
            _ => {
                return Err(RuntimeError::invalid_mutation(format!(
                    "unknown runtime mutation '{}'",
                    request.name
                )))
            }
        };
        // Before-read for the diff (diff-eligible mutations only). Read before
        // forwarding so the diff captures the pre-apply state.
        let before = if diff_eligible {
            self.read_fold_state(&source_id, &message_id).await?
        } else {
            None
        };
        // Accept the mutation into the runtime's outbox toward the backend so
        // recomputed views fold it optimistically while it is in flight; retire
        // it once the backend confirms (or fails). In the in-process default the
        // forward confirms synchronously, so the outbox is empty between
        // mutations and the overlay is a pass-through (`colocated-unchanged`).
        let optimistic = named_message_assertion(&request).map(|(message_id, assertion)| {
            let id = MutationId(request.client_mutation_id.as_str().to_string());
            self.core.outbox.accept(PendingMessageMutation {
                id: id.clone(),
                message_id,
                assertion,
            });
            id
        });
        // Up-channel: forward the named mutation to the backend far node.
        let forward = self.core.backend_link.forward_mutation(request.clone());
        let result = self.run_message_mutation(caller, &request, forward).await;
        if let Some(id) = optimistic {
            self.core.outbox.retire(&id);
        }
        // Record the invertible change-diff once the backend has confirmed, so
        // undo/redo can replay it. A no-op mutation (empty diff, e.g. adding a
        // keyword that was already present) is not recorded.
        if diff_eligible && result.is_ok() {
            if let Some(diff) = self.capture_diff(&source_id, &message_id, before).await? {
                if !diff.is_empty() {
                    self.core
                        .sessions
                        .record_diff(&session_id, source_id, message_id, diff)?;
                }
            }
        }
        result
    }

    /// Run an undo/redo as an ordinary `message.applyDiff` mutation — its diff is
    /// folded into the outbox and forwarded like any user action — while
    /// navigating the runtime-owned diff history around it: the step named by
    /// `undoOf`/`redoOf` is popped (held) before execution and committed to the
    /// opposite stack on success, restored on failure (a desktop editor's
    /// rollback). Execution itself is uniform (outbox + replay guard +
    /// last-writer-wins); the seq hints are history-bookkeeping only.
    ///
    /// @spec docs/runtime/mutations/L1#mutation-pipeline-and-catalog
    async fn run_apply_diff(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        request: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        let args: MessageApplyDiffArgs = parse_args(&request)?;
        let session_scope = self
            .core
            .sessions
            .session_scope(&session_id, caller.account_scope.as_deref())?;
        Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
        // Pop the named step off the runtime's history (hold it across
        // execution). A stale `undoOf`/`redoOf` (no matching step) still runs the
        // applyDiff — last-writer-wins — but does not navigate; the next history
        // frame resyncs the client.
        let (is_undo, held) = if let Some(seq) = args.undo_of {
            (true, self.core.sessions.pop_undo_by_seq(&session_id, seq)?)
        } else if let Some(seq) = args.redo_of {
            (false, self.core.sessions.pop_redo_by_seq(&session_id, seq)?)
        } else {
            (false, None)
        };
        let result = self
            .dispatch_named_mutation(caller, session_id.clone(), request)
            .await;
        if let Some(step) = held {
            // Commit: undo → redo, redo → undo. Restore to the source stack on
            // failure so the step stays navigable.
            let push_to_redo = match (is_undo, result.is_ok()) {
                (true, true) => true,   // undo succeeded: step becomes redoable
                (true, false) => false, // undo failed: restore to undo
                (false, true) => false, // redo succeeded: step becomes undoable
                (false, false) => true, // redo failed: restore to redo
            };
            if push_to_redo {
                self.core.sessions.push_redo(&session_id, step)?;
            } else {
                self.core.sessions.push_undo(&session_id, step)?;
            }
            self.core.sessions.emit_history_frame(&session_id)?;
        }
        result
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
impl RuntimeCore for RuntimeHandle {
    async fn runtime_status(&self, _caller: RuntimeCaller) -> Result<RuntimeStatus, RuntimeError> {
        let mut status = self.current_status();
        // The live account count is backend state; read it through the link
        // (best-effort — a status read never fails on a count miss).
        if let Ok(Some(account_count)) = self.core.reads.account_count().await {
            status.account_count = account_count;
        }
        Ok(status)
    }

    async fn get_app_settings(&self, _caller: RuntimeCaller) -> Result<AppSettings, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.app_settings().await
    }

    async fn patch_app_settings(
        &self,
        _caller: RuntimeCaller,
        mutation: posthaste_runtime_contract::PatchAppSettingsMutation,
    ) -> Result<AppSettings, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.backend_link.patch_app_settings(mutation).await
    }

    async fn preview_automation_rule(
        &self,
        _caller: RuntimeCaller,
        mutation: posthaste_runtime_contract::AutomationRulePreviewMutation,
    ) -> Result<posthaste_runtime_contract::AutomationRulePreviewResult, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .preview_automation_rule(mutation)
            .await
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
    ) -> Result<posthaste_domain::AccountOverview, RuntimeError> {
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

    async fn list_mailboxes(
        &self,
        _caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<
        std::collections::BTreeMap<AccountId, Vec<posthaste_domain::MailboxSummary>>,
        RuntimeError,
    > {
        self.ensure_runtime_active()?;
        self.core.reads.list_mailboxes(scope).await
    }

    async fn list_smart_mailboxes(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<Vec<posthaste_domain::SmartMailboxSummary>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_smart_mailboxes().await
    }

    async fn get_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<posthaste_domain::SmartMailbox, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.get_smart_mailbox(smart_mailbox_id).await
    }

    async fn create_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        mutation: posthaste_runtime_contract::CreateSmartMailboxMutation,
    ) -> Result<posthaste_domain::SmartMailbox, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.backend_link.create_smart_mailbox(mutation).await
    }

    async fn patch_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
        mutation: posthaste_runtime_contract::PatchSmartMailboxMutation,
    ) -> Result<posthaste_domain::SmartMailbox, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
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
            .backend_link
            .delete_smart_mailbox(smart_mailbox_id)
            .await
    }

    async fn reset_default_smart_mailboxes(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<Vec<posthaste_domain::SmartMailboxSummary>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.backend_link.reset_default_smart_mailboxes().await
    }

    async fn list_tags(
        &self,
        _caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<Vec<posthaste_domain::TagSummary>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_tags(scope).await
    }

    async fn get_identity(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<posthaste_domain::Identity, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.get_identity(account_id).await
    }

    async fn list_sender_addresses(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<Vec<posthaste_domain::CachedSenderAddress>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_sender_addresses().await
    }

    async fn get_reply_context(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<posthaste_domain::ReplyContext, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .reads
            .get_reply_context(account_id, message_id)
            .await
    }

    async fn query_mail_page(
        &self,
        _caller: RuntimeCaller,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.query_mail_page(request).await
    }

    async fn open_session(&self, caller: RuntimeCaller) -> Result<RuntimeSession, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.sessions.open_session(caller)
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
    }

    async fn close_session(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.sessions.close_session(caller, session_id)
    }

    async fn open_session_view(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        descriptor: ViewDescriptor,
    ) -> Result<posthaste_runtime_contract::ViewSnapshot, RuntimeError> {
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

    async fn extend_session_view(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        view_id: ViewId,
        count: usize,
    ) -> Result<posthaste_runtime_contract::ViewSnapshot, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .sessions
            .extend_view(caller, session_id, view_id, count)
            .await
    }

    async fn run_mutation(
        &self,
        caller: RuntimeCaller,
        request: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        self.ensure_runtime_active()?;
        let session_id = request.session_id.clone().ok_or_else(|| {
            RuntimeError::invalid_mutation("runtime mutation requires a session id")
        })?;
        // Undo/redo are ordinary `message.applyDiff` mutations that also navigate
        // the runtime-owned diff history; every other mutation is a fresh user
        // action whose change-diff is recorded onto it.
        //
        // @spec docs/runtime/mutations/L1#mutation-pipeline-and-catalog
        match request.name.as_str() {
            "message.applyDiff" => self.run_apply_diff(caller, session_id, request).await,
            _ => {
                self.dispatch_named_mutation(caller, session_id, request)
                    .await
            }
        }
    }

    async fn open_view(
        &self,
        caller: RuntimeCaller,
        descriptor: ViewDescriptor,
    ) -> Result<posthaste_runtime_contract::ViewSnapshot, RuntimeError> {
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

    async fn send_message(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        request: SendMessageRequest,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .send_message(account_id, request)
            .await
    }

    async fn save_draft(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        draft_id: Option<MessageId>,
        request: SendMessageRequest,
    ) -> Result<Operation, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .save_draft(account_id, draft_id, request)
            .await
    }

    async fn delete_draft(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        draft_id: MessageId,
    ) -> Result<Operation, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .delete_draft(account_id, draft_id)
            .await
    }

    async fn list_pending_operations(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<Vec<Operation>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_pending_operations(account_id).await
    }

    async fn discard_operation(
        &self,
        _caller: RuntimeCaller,
        _account_id: AccountId,
        operation_id: OperationId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.backend_link.discard_operation(operation_id).await
    }

    async fn retry_operation(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        operation_id: OperationId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .retry_operation(account_id, operation_id)
            .await
    }

    async fn set_message_keywords(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: SetKeywordsCommand,
    ) -> Result<posthaste_domain::CommandAck, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .set_keywords(account_id, message_id, command)
            .await
    }

    async fn add_message_to_mailbox(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: AddToMailboxCommand,
    ) -> Result<posthaste_domain::CommandAck, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .add_to_mailbox(account_id, message_id, command)
            .await
    }

    async fn remove_message_from_mailbox(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: RemoveFromMailboxCommand,
    ) -> Result<posthaste_domain::CommandAck, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .remove_from_mailbox(account_id, message_id, command)
            .await
    }

    async fn replace_message_mailboxes(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: ReplaceMailboxesCommand,
    ) -> Result<posthaste_domain::CommandAck, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .replace_mailboxes(account_id, message_id, command)
            .await
    }

    async fn destroy_message(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<posthaste_domain::CommandAck, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .destroy_message(account_id, message_id)
            .await
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
            .backend_link
            .set_mailbox_role(account_id, mailbox_id, role)
            .await
    }

    async fn get_message_detail(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<posthaste_domain::CommandResult, RuntimeError> {
        self.ensure_runtime_active()?;
        // Body-free: the detail read serves header + cached attachments only and
        // never loads the body (it is the separate `/body` lazy resource), so
        // opening a message neither provider-fetches nor materializes the body.
        let detail = self
            .core
            .reads
            .message_detail(&account_id, &message_id)
            .await?;
        Ok(posthaste_domain::CommandResult {
            detail,
            events: Vec::new(),
        })
    }

    async fn get_draft_content(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<posthaste_domain::DraftContent, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .reads
            .get_draft_content(account_id, message_id)
            .await
    }

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

    async fn sync_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        mode: SyncMode,
    ) -> Result<usize, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.backend_link.sync_account(account_id, mode).await
    }

    async fn replay_events(
        &self,
        _caller: RuntimeCaller,
        filter: EventFilter,
    ) -> Result<Vec<DomainEvent>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.replay_events(filter).await
    }

    async fn subscribe_events(
        &self,
        _caller: RuntimeCaller,
        filter: EventFilter,
    ) -> Result<RuntimeEventSubscription, RuntimeError> {
        self.ensure_runtime_active()?;
        let receiver = self.core.event_sender.subscribe();
        let replay = if filter.after_seq.is_some() {
            self.replay_events(RuntimeCaller::system(), filter.clone())
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

    async fn create_account(
        &self,
        _caller: RuntimeCaller,
        mutation: CreateAccountMutation,
    ) -> Result<posthaste_domain::AccountOverview, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.backend_link.create_account(mutation).await
    }

    async fn patch_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        mutation: PatchAccountMutation,
    ) -> Result<posthaste_domain::AccountOverview, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .patch_account(account_id, mutation)
            .await
    }

    async fn delete_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.backend_link.delete_account(account_id).await
    }

    async fn verify_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<AccountVerificationResult, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.backend_link.verify_account(account_id).await
    }

    async fn set_account_enabled(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        enabled: bool,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .set_account_enabled(account_id, enabled)
            .await
    }

    async fn reload_config(&self, _caller: RuntimeCaller) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.backend_link.reload_config().await
    }
}

/// Shutdown ownership for authority runtime tasks and resources.
///
/// The first extraction slice owns no long-lived account tasks yet; this handle
/// records shutdown state so adapters already depend on the runtime-owned
/// shutdown seam instead of tearing resources down themselves.
///
/// spec: docs/runtime/internals/L2#runtime-shutdown-handle
pub struct RuntimeShutdownHandle {
    stopped: Arc<AtomicBool>,
}

impl RuntimeShutdownHandle {
    // Async by contract: shutdown is part of the runtime's async lifecycle
    // (start/await, shutdown/await) and will await task joins as it grows.
    #[allow(clippy::unused_async)]
    pub async fn shutdown(self) -> Result<(), RuntimeShutdownError> {
        self.stopped.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum RuntimeBuildError {
    #[error("config error: {0}")]
    Config(#[from] ConfigError),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("service error: {0}")]
    Service(#[from] ServiceError),
    #[error("invalid runtime build config: {0}")]
    InvalidConfig(String),
    #[error("io error for {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("failed to read bootstrap config {path}: {source}")]
    BootstrapRead { path: PathBuf, source: io::Error },
    #[error("failed to parse bootstrap config {path}: {message}")]
    BootstrapParse { path: PathBuf, message: String },
    #[error("failed to read runtime clock: {0}")]
    Clock(String),
}

#[derive(Debug, Error)]
pub enum RuntimeShutdownError {
    #[error("runtime shutdown failed: {0}")]
    Failed(String),
}
