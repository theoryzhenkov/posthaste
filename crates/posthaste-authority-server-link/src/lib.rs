//! The runtime↔authority-server link surface — one coherent link's contract.
//!
//! A coherent link ([replication L1 §2](../replication/L1.md)) carries exactly
//! two channels between a near node and a far node: named mutations forwarded
//! **up**, authoritative base assertions + per-mutation confirmation streamed
//! **down**. This crate holds the transport-neutral definition of the
//! runtime↔authority-server instantiation of that shape — *not* the full
//! [`RuntimeCore`] surface, only its replication subset, and not the
//! client↔runtime link's own surface (that lives in `posthaste-client-link`)
//! ([replication authority-server-link L1](../replication/authority-server-link/L1.md)).
//! The two links share the `MutationRequest`/`MutationReceipt` vocabulary from
//! `posthaste-contract-core`, not this crate's traits.
//!
//! The far-node seam is two traits (D33 seam symmetry, mirroring the
//! client↔runtime `RuntimeApi`/`RuntimeLink` pair):
//!
//! - [`AuthorityServerApi`] — the typed request/response surface: all reads,
//!   the compose/catalog/settings/account operations, and the single
//!   direct-apply message-command entry [`apply`](AuthorityServerApi::apply)
//!   (D34; the five per-command RPCs collapsed into it).
//! - [`AuthorityServerLink`] — the coherent-link mechanics: `forward_mutation`
//!   up, `subscribe` down, and the outbox op-lifecycle mutations
//!   (`retry_operation`/`discard_operation`).
//!
//! One transport implements both; it is selected by configuration (in-process
//! co-located by default, remote when split —
//! [replication authority-server-link L2 §6](../replication/authority-server-link/L2.md)). The transport is what
//! varies across deployments; the contract above it does not. This is the seam
//! the `one-link-transport` assertion guards: one shared contract + one Rust
//! transport abstraction, never a second bespoke mechanism.
//!
//! @spec docs/replication/authority-server-link/L1#3-the-backendapi-contract
//! @spec docs/replication/authority-server-link/L2#2-backendapi-implementations-localbackend-remotebackend

use std::sync::Arc;

use async_trait::async_trait;
use futures_util::stream::BoxStream;
use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;

use posthaste_contract_core::{
    mutation_args::keyword_toggle, AccountScopeRequest, AccountVerificationResult,
    AutomationRulePreviewMutation, AutomationRulePreviewResult, CreateAccountMutation,
    CreateSmartMailboxMutation, MailOperation, MailQueryPage, MailQueryRequest,
    MessageResourceKind, MutationReceipt, MutationRequest, PatchAccountMutation,
    PatchAppSettingsMutation, PatchSmartMailboxMutation, RuntimeAccountList, RuntimeError,
    RuntimeErrorCode, RuntimeResourceBytes,
};
use posthaste_domain_model::{
    AccountId, AccountOverview, AddToMailboxCommand, AppSettings, CachedSenderAddress, CommandAck,
    ConversationId, ConversationView, DomainEvent, DraftContent, EventFilter, EventLogBounds,
    Identity, MailboxId, MailboxSummary, MessageDetail, MessageId, MessageSummary, Operation,
    OperationId, RemoveFromMailboxCommand, ReplaceMailboxesCommand, ReplyContext, RevLogSnapshot,
    SendMessageRequest, SetKeywordsCommand, SmartMailbox, SmartMailboxId, SmartMailboxSummary,
    SyncMode, TagSummary,
};
use posthaste_replica_core::{MessageFoldState, MutationId, SettlementOutcome};

/// Wire path for the link up-channel: a remote near node `POST`s a
/// [`MutationRequest`] (JSON) here and receives a [`MutationReceipt`]. Shared by
/// the remote transport client and the far-node HTTP surface so the two cannot
/// drift ([replication authority-server-link L2 §2](../replication/authority-server-link/L2.md)).
pub const LINK_FORWARD_MUTATION_PATH: &str = "/v1/link/mutations";

/// Wire path for the link down-channel: a remote near node opens an SSE stream
/// here whose `data:` frames are JSON [`AuthorityServerFrame`]s.
pub const LINK_SUBSCRIBE_PATH: &str = "/v1/link/subscribe";

/// Wire path for the read channel's mail-list query: a remote near node `POST`s
/// a [`MailQueryRequest`] and receives a [`MailQueryPage`]. The query engine
/// runs at the far node (the authority); the near node reads through to it.
pub const LINK_QUERY_PATH: &str = "/v1/link/query";

/// Wire path for the read channel's point read: the current [`MessageSummary`]
/// of one message (the read behind undo-history). `POST`ed as `{accountId,
/// messageId}`, returns the summary or null.
pub const LINK_SUMMARY_PATH: &str = "/v1/link/summary";

/// Wire path for the read channel's message detail (the `messageDetail` view).
/// `POST`ed as `{accountId, messageId}`, returns the detail or null.
pub const LINK_DETAIL_PATH: &str = "/v1/link/detail";

/// Wire path for the read channel's conversation (the `conversation` view).
/// `POST`ed as `{conversationId}`, returns the folded conversation.
pub const LINK_CONVERSATION_PATH: &str = "/v1/link/conversation";

/// One authoritative base update for a single message, carried on the
/// down-channel ([replication L1 §5.1](../replication/L1.md)). The near node
/// rebases its base cache on each: a new asserted confirmed state, or a removal.
///
/// This is the wire-shaped, serializable twin of `replica_core::MessageBaseUpdate`
/// (which is internal to the convergence engine and not `Serialize`). The near
/// node maps between the two when applying a frame to its `MessageReplica`
/// (W2); keeping the wire type here lets the remote transport (W3) serialize it
/// without leaking the engine's internal enum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BaseUpdate {
    /// The message's confirmed canonical state is now this.
    Present(MessageFoldState),
    /// The message left the served base (authoritative removal).
    Removed,
}

/// An authoritative before/after state assertion over one message
/// ([replication authority-server-link L1 §3](../replication/authority-server-link/L1.md)). Ordered within a frame; the near
/// node applies them to its base cache in order, then recomputes its derived
/// views (never invalidates).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseAssertion {
    /// The account the message belongs to. Carried so a near node can scope the
    /// change (cache eviction, view recompute) to the right account rather than
    /// matching on the bare message id.
    pub account_id: String,
    pub message_id: String,
    pub update: BaseUpdate,
    /// The authoritative `message.updated` [`DomainEvent`] this assertion was
    /// derived from, carried whole so a split (remote) runtime republishes the
    /// SAME enriched event the co-located bus delivers — `payload.projection`
    /// (the body-free `MessageSummary`) included — instead of hand-building a
    /// bare `{changes: {...}}` one that leaves the client's mail-list rows
    /// waiting for a re-serve. Counts ride no event
    /// (RFC-L2-count-unification); clients invalidate + refetch them. `None`
    /// on frames from an older far node (or synthetic test frames); the near
    /// node then falls back to the bare synthesized event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<DomainEvent>,
}

/// One frame on the link's down-channel ([replication authority-server-link L1 §3](../replication/authority-server-link/L1.md)).
///
/// The "confirmation watermark" (how far the far node has confirmed the near
/// node's forwarded mutations) is realized **per mutation** as
/// [`AuthorityServerFrame::Settlement`] — the shape the contract already serves on the
/// client↔runtime wire (`RuntimeFrame::MutationSettlement`) — rather than as a
/// scalar high-water mark. By the state-before-event rule the matching base
/// assertion arrives first, so a confirmed settlement is a visual no-op; a
/// failed one drives the near node's recompute back to authoritative state
/// ([replication L1 §5.3, §5.5](../replication/L1.md)).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AuthorityServerFrame {
    /// A batch of ordered authoritative base updates to apply to the base cache.
    Base { assertions: Vec<BaseAssertion> },
    /// A forwarded mutation reached its terminal outcome at the far node — the
    /// per-mutation confirmation watermark. Retires the matching pending-set entry.
    Settlement {
        /// The engine's mutation id, carried directly on the wire (D12 — no
        /// serde mirror type; `MutationId` is already serde and this crate
        /// depends on `replica-core`).
        mutation_id: MutationId,
        outcome: WireSettlementOutcome,
    },
    /// Liveness only; carries no state. Lets a remote transport keep the
    /// down-stream open without implying a base change.
    Heartbeat,
}

/// Stable identity of a runtime near node at the runtime↔authority-server link
/// ([replication authority-server-link L1 §3.1](../replication/authority-server-link/L1.md)).
///
/// A remote runtime establishes its `AuthorityServerLinkId` with the authority server at link
/// establishment (derived from its authenticated credential); the authority server scopes
/// mutation-id idempotency, the confirmation watermark, and settlement routing
/// per `AuthorityServerLinkId`, so two runtimes may independently mint the same
/// `ClientMutationId` without collision. There is no single-runtime special
/// case — the co-located runtime is simply the one in-process runtime (X=1),
/// with a real minted id rather than a sentinel.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AuthorityServerLinkId(pub String);

impl AuthorityServerLinkId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Serializable mirror of [`SettlementOutcome`] for the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WireSettlementOutcome {
    Confirmed,
    Failed,
}

impl From<SettlementOutcome> for WireSettlementOutcome {
    fn from(outcome: SettlementOutcome) -> Self {
        match outcome {
            SettlementOutcome::Confirmed => Self::Confirmed,
            SettlementOutcome::Failed => Self::Failed,
        }
    }
}

impl From<WireSettlementOutcome> for SettlementOutcome {
    fn from(outcome: WireSettlementOutcome) -> Self {
        match outcome {
            WireSettlementOutcome::Confirmed => SettlementOutcome::Confirmed,
            WireSettlementOutcome::Failed => SettlementOutcome::Failed,
        }
    }
}

/// What slice of the far node's base the near node subscribes to
/// ([replication authority-server-link L1 §3](../replication/authority-server-link/L1.md)). The co-located runtime requests
/// [`Complete`](LinkCoverage::Complete) coverage (it serves the whole working
/// set); a split runtime may request a [`WorkingSet`](LinkCoverage::WorkingSet)
/// so it can distinguish "absent because unchanged" from "absent because not
/// held". The working-set shape is left open for the split-runtime slice (W4);
/// today only `Complete` is exercised.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LinkCoverage {
    /// The far node serves its complete base down — the co-located default.
    #[default]
    Complete,
    /// The far node serves only a working set. `descriptor` names it; its shape
    /// is defined when split runtimes land (W4).
    WorkingSet {
        #[serde(default)]
        descriptor: serde_json::Value,
    },
}

/// This seam's down-channel envelope: an [`AuthorityServerFrame`] carried in the
/// **generic** [`Sequenced`](posthaste_link_far_end::down::Sequenced) wire envelope
/// owned by the engine crate. [`AuthorityServerFrame`] stays the canonical,
/// emitter-named frame vocabulary (D1/D39/XIV); the seq rides *alongside* it
/// (`{ "seq": N, "frame": { .. } }`), never inside it. A resubscribing near node
/// passes the last seq it saw as `after_seq` and the far node replays from there
/// (coverage says WHAT to stream, seq says WHERE to resume). The envelope is
/// generic over the frame precisely so the client↔runtime seam can reuse it.
pub type SequencedFrame = posthaste_link_far_end::down::Sequenced<AuthorityServerFrame>;

/// The ordered down-channel: authoritative base assertions + confirmation, each
/// stamped with its per-subscriber seq per [`SequencedFrame`].
pub type DownStream = BoxStream<'static, SequencedFrame>;

/// One link's two channels + the outbox op-lifecycle, transport-neutral (the
/// Link half of the D33 seam). The transport is the only thing that varies
/// across deployments — in-process and co-located by default (W1,
/// behavior-preserving), remote when the far node lives elsewhere (W3) — and is
/// selected by configuration, never at build time
/// ([replication authority-server-link L2 §6](../replication/authority-server-link/L2.md), assertion `transport-selected-by-config`).
///
/// `forward_mutation` shares its name (not a supertrait) with the client-link
/// seam's up-channel: the signatures differ for real reasons — `RuntimeLink`
/// threads a per-call `RuntimeCaller` (one runtime multiplexes many client
/// links) while this seam scopes identity per connection (the `*_for`
/// variants for fan-in). D35b verdict: same-name convention, no shared
/// supertrait.
///
/// Every transport implements this alongside [`AuthorityServerApi`] (assertion
/// `one-link-transport`); the runtime holds the pair via
/// [`AuthorityServerLinkHandle`].
#[async_trait]
pub trait AuthorityServerLink: Send + Sync {
    /// Up-channel. Forward a (possibly client-originated) named mutation toward
    /// the far node with a stable mutation id; the receipt carries the far
    /// node's `RuntimeMutationId` for the confirmation join. Idempotent on the
    /// mutation id ([replication authority-server-link L1 §3](../replication/authority-server-link/L1.md)).
    async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError>;

    /// Up-channel, runtime-aware variant of [`forward_mutation`](Self::forward_mutation):
    /// the far node scopes mutation-id idempotency and confirmation under
    /// `runtime_id`
    /// ([replication authority-server-link L1 §3.1](../replication/authority-server-link/L1.md)). The
    /// link server derives `runtime_id` from the connecting runtime's credential
    /// and threads it here; an in-process caller reaches the same path with its
    /// minted id. The default delegates to `forward_mutation` (ignoring the id)
    /// for transports that carry no per-runtime registry — test stubs, and the
    /// client side which is the runtime itself and has no id of its own to
    /// present.
    async fn forward_mutation_for(
        &self,
        runtime_id: &AuthorityServerLinkId,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        let _ = runtime_id;
        self.forward_mutation(mutation).await
    }

    /// Down-channel. Subscribe to the far node's ordered stream of base
    /// assertions + per-mutation confirmation for a coverage. The near node
    /// rebases its base cache on each frame and recomputes its derived views.
    ///
    /// `after_seq` is the resume cursor (D46): `None` opens a fresh stream;
    /// `Some(seq)` asks the far node to replay from just after `seq` (or collapse
    /// to current state when that point has fallen out of the backlog). Coverage
    /// says WHAT to stream, `after_seq` says WHERE to resume.
    async fn subscribe(
        &self,
        coverage: LinkCoverage,
        after_seq: Option<u64>,
    ) -> Result<DownStream, RuntimeError>;

    /// Down-channel, runtime-aware variant of [`subscribe`](Self::subscribe): as
    /// `subscribe` but the far node routes `AuthorityServerFrame::Settlement` onto the
    /// originating `runtime_id`'s stream only (merged with the broadcast `Base`)
    /// — `settlement-routed-to-origin-runtime`. The link server derives
    /// `runtime_id` from the connecting runtime's credential. The default
    /// delegates to `subscribe` (ignoring the id) for transports that carry no
    /// per-runtime sink.
    async fn subscribe_for(
        &self,
        runtime_id: &AuthorityServerLinkId,
        coverage: LinkCoverage,
        after_seq: Option<u64>,
    ) -> Result<DownStream, RuntimeError> {
        let _ = runtime_id;
        self.subscribe(coverage, after_seq).await
    }

    /// Op-lifecycle: discard a pending outbox operation (a user escape hatch
    /// for a dead op).
    async fn discard_operation(&self, operation_id: OperationId) -> Result<(), RuntimeError> {
        let _ = operation_id;
        Err(write_channel_unsupported())
    }

    /// Op-lifecycle: re-arm a failed outbox operation to pending.
    async fn retry_operation(
        &self,
        account_id: AccountId,
        operation_id: OperationId,
    ) -> Result<(), RuntimeError> {
        let _ = (account_id, operation_id);
        Err(write_channel_unsupported())
    }
}

/// The typed request/response surface of the far node (the Api half of the D33
/// seam): what the nearer tier may *request* — every read (all mirrored 1:1 by
/// the runtime's `ReadCache`), the compose-outbox creation trio, the
/// catalog/settings/smart-mailbox/account operations, `sync_account`,
/// `reload_config`, and the single direct-apply message-command entry
/// [`apply`](Self::apply) (D34).
///
/// Identity is per-connection on this seam (D35b): unlike the client↔runtime
/// `RuntimeApi` facets there is no per-call caller parameter — a remote runtime
/// is authenticated once at the link boundary (`link_router`).
///
/// Every method defaults to a typed error so a transport that does not carry a
/// channel (e.g. a write-only test stub) is simply not a source/sink for it.
#[async_trait]
pub trait AuthorityServerApi: Send + Sync {
    /// Read channel: compute a page of a mail-list query at the far node (the
    /// query engine is the authority's, [authority-server-link L3](../replication/authority-server-link/L3.md)).
    /// A near node reads through here on a cache miss. The default errors: a
    /// transport that does not carry the read channel (e.g. a write-only test
    /// stub) is simply not a read source.
    async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        let _ = request;
        Err(read_channel_unsupported())
    }

    /// Read channel: the current canonical summary of one message (the point
    /// read behind undo-history). `None` when the far node does not hold it.
    async fn current_summary(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<Option<MessageSummary>, RuntimeError> {
        let _ = (account_id, message_id);
        Err(read_channel_unsupported())
    }

    /// Read channel: a message's detail (header + attachments) for the
    /// `messageDetail` view.
    async fn message_detail(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        let _ = (account_id, message_id);
        Err(read_channel_unsupported())
    }

    /// Read channel: an overlay-folded conversation for the `conversation` view.
    async fn conversation(
        &self,
        conversation_id: ConversationId,
    ) -> Result<ConversationView, RuntimeError> {
        let _ = conversation_id;
        Err(read_channel_unsupported())
    }

    /// Read channel: the account list (ids + enabled + overviews).
    async fn list_accounts(&self) -> Result<RuntimeAccountList, RuntimeError> {
        Err(read_channel_unsupported())
    }

    /// Read channel: one account's overview, `None` when absent.
    async fn get_account(
        &self,
        account_id: AccountId,
    ) -> Result<Option<AccountOverview>, RuntimeError> {
        let _ = account_id;
        Err(read_channel_unsupported())
    }

    /// Read channel: the application settings.
    async fn app_settings(&self) -> Result<AppSettings, RuntimeError> {
        Err(read_channel_unsupported())
    }

    /// Read channel: resolve an account scope to concrete account ids.
    async fn resolve_account_scope(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<Vec<AccountId>, RuntimeError> {
        let _ = scope;
        Err(read_channel_unsupported())
    }

    /// Read channel: mailboxes per account for a scope.
    async fn list_mailboxes(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<BTreeMap<AccountId, Vec<MailboxSummary>>, RuntimeError> {
        let _ = scope;
        Err(read_channel_unsupported())
    }

    /// Read channel: the smart-mailbox summaries.
    async fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailboxSummary>, RuntimeError> {
        Err(read_channel_unsupported())
    }

    /// Read channel: one smart mailbox.
    async fn get_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<SmartMailbox, RuntimeError> {
        let _ = smart_mailbox_id;
        Err(read_channel_unsupported())
    }

    /// Read channel: the tag summaries for a scope.
    async fn list_tags(&self, scope: AccountScopeRequest) -> Result<Vec<TagSummary>, RuntimeError> {
        let _ = scope;
        Err(read_channel_unsupported())
    }

    /// Read channel: the account's sender identity (provider-backed, falling
    /// back to configured sender). Resolves a gateway at the far node.
    async fn get_identity(&self, account_id: AccountId) -> Result<Identity, RuntimeError> {
        let _ = account_id;
        Err(read_channel_unsupported())
    }

    /// Read channel: reply/forward metadata for composing a response to one
    /// message. Resolves a gateway at the far node.
    async fn get_reply_context(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<ReplyContext, RuntimeError> {
        let _ = (account_id, message_id);
        Err(read_channel_unsupported())
    }

    /// Read channel: the cached sender addresses (the compose autocomplete set).
    async fn list_sender_addresses(&self) -> Result<Vec<CachedSenderAddress>, RuntimeError> {
        Err(read_channel_unsupported())
    }

    /// Read channel: an account's pending outbox operations.
    async fn list_pending_operations(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<Operation>, RuntimeError> {
        let _ = account_id;
        Err(read_channel_unsupported())
    }

    /// Read channel: replay the authoritative event log for a filter.
    async fn replay_events(&self, filter: EventFilter) -> Result<Vec<DomainEvent>, RuntimeError> {
        let _ = filter;
        Err(read_channel_unsupported())
    }

    /// Read channel: the cheap `event_log` seq bounds (`MIN`/`MAX(seq)`) for the
    /// fact-carrying tap's head/truncation queries (RFC-L2-scripting D52 / S2),
    /// avoiding a full replay scan. `None` when the log is empty. The default
    /// errors like every other read-channel op: a transport that does not carry
    /// the read channel is not a bounds source (the runtime binding falls back to
    /// a replay scan on this error).
    async fn event_log_bounds(&self) -> Result<Option<EventLogBounds>, RuntimeError> {
        Err(read_channel_unsupported())
    }

    /// Read channel: compose-ready content for resuming an existing draft. May
    /// lazily fetch and cache the body at the far node (publishing the resulting
    /// events on the down-channel).
    async fn get_draft_content(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<DraftContent, RuntimeError> {
        let _ = (account_id, message_id);
        Err(read_channel_unsupported())
    }

    /// Read channel: the raw bytes of a message resource (an attachment blob or
    /// the HTML/text body). Body resources return raw bytes; the serve layer
    /// applies the per-kind transform. May lazily fetch at the far node.
    async fn get_message_resource(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        kind: MessageResourceKind,
    ) -> Result<RuntimeResourceBytes, RuntimeError> {
        let _ = (account_id, message_id, kind);
        Err(read_channel_unsupported())
    }

    /// Read channel: the authority server's count of live (running) accounts, for the
    /// runtime status. `None` when the authority server does not track it.
    async fn account_count(&self) -> Result<Option<usize>, RuntimeError> {
        Err(read_channel_unsupported())
    }

    /// Read channel: the per-account undo/redo `rev_log` + cursor (Phase 2
    /// server-authoritative history). Serves the `RevLog` synced view, which
    /// mirrors the log + cursor to every device. The default errors: a transport
    /// that does not carry the read channel is not a read source for history.
    ///
    /// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
    async fn rev_log_snapshot(
        &self,
        account_id: AccountId,
    ) -> Result<RevLogSnapshot, RuntimeError> {
        let _ = account_id;
        Err(read_channel_unsupported())
    }

    // ===== Write channel: typed authority server commands =====
    //
    // The named up-channel (`AuthorityServerLink::forward_mutation`) carries
    // link-originated message mutations through the replica; these typed
    // commands are the direct (REST) command surface, applied at the far node
    // and returning the typed ack. Default-erroring so a transport that does not
    // carry the write channel is simply not a command sink (the remote wire is
    // wired per-op alongside the reads).

    /// Write: apply a mail operation authoritatively and return its command ack
    /// (D21/D34 — the five per-command message RPCs collapsed into one typed
    /// entry). This is the **direct-apply** command surface: REST callers are
    /// not replicas and hold no pending set, so there is no optimistic fold or
    /// `ClientMutationId` dedup here. The replica (optimistic) path forwards the
    /// same [`MailOperation`] through `AuthorityServerLink::forward_mutation`
    /// instead.
    ///
    /// Covers the typed command subset the REST surface exposes (set-keywords,
    /// add/remove-to-mailbox, replace-mailboxes, destroy); operations that only
    /// exist on the replica forward path (role moves, snooze, applyDiff, the
    /// `revCursor` control op) are rejected with `InvalidMutation` — see
    /// [`MailCommandRequest::from_operation`].
    async fn apply(&self, op: MailOperation) -> Result<CommandAck, RuntimeError> {
        let _ = op;
        Err(write_channel_unsupported())
    }

    /// Write: set (or clear) a mailbox's role, returning the account's mailboxes.
    async fn set_mailbox_role(
        &self,
        account_id: AccountId,
        mailbox_id: MailboxId,
        role: Option<String>,
    ) -> Result<Vec<MailboxSummary>, RuntimeError> {
        let _ = (account_id, mailbox_id, role);
        Err(write_channel_unsupported())
    }

    /// Write: create a new top-level mailbox, returning the account's mailboxes.
    async fn create_mailbox(
        &self,
        account_id: AccountId,
        name: String,
    ) -> Result<Vec<MailboxSummary>, RuntimeError> {
        let _ = (account_id, name);
        Err(write_channel_unsupported())
    }

    /// Write: destroy a mailbox, returning the account's mailboxes. `remove_emails`
    /// is the confirmed safety flag (the service refuses a non-empty mailbox
    /// without it — M2 confirm-with-count gate).
    async fn destroy_mailbox(
        &self,
        account_id: AccountId,
        mailbox_id: MailboxId,
        remove_emails: bool,
    ) -> Result<Vec<MailboxSummary>, RuntimeError> {
        let _ = (account_id, mailbox_id, remove_emails);
        Err(write_channel_unsupported())
    }

    /// Write: queue a local-first send for an account.
    async fn send_message(
        &self,
        account_id: AccountId,
        request: SendMessageRequest,
    ) -> Result<(), RuntimeError> {
        let _ = (account_id, request);
        Err(write_channel_unsupported())
    }

    /// Write: save (create or update) a draft.
    async fn save_draft(
        &self,
        account_id: AccountId,
        draft_id: Option<MessageId>,
        request: SendMessageRequest,
    ) -> Result<Operation, RuntimeError> {
        let _ = (account_id, draft_id, request);
        Err(write_channel_unsupported())
    }

    /// Write: delete a draft.
    async fn delete_draft(
        &self,
        account_id: AccountId,
        draft_id: MessageId,
    ) -> Result<Operation, RuntimeError> {
        let _ = (account_id, draft_id);
        Err(write_channel_unsupported())
    }

    /// Write: drive an explicit account sync, returning the number of changes.
    async fn sync_account(
        &self,
        account_id: AccountId,
        mode: SyncMode,
    ) -> Result<usize, RuntimeError> {
        let _ = (account_id, mode);
        Err(write_channel_unsupported())
    }

    // ===== Write channel: account + config mutations =====
    //
    // The account/config write surface (account CRUD + enable/verify, app
    // settings, smart mailboxes, automation preview, config reload) is authority server
    // authority; the runtime forwards it over the link. (OAuth account creation
    // stays host-driven for now — its provider types live above this contract.)

    /// Write: patch the application settings.
    async fn patch_app_settings(
        &self,
        mutation: PatchAppSettingsMutation,
    ) -> Result<AppSettings, RuntimeError> {
        let _ = mutation;
        Err(write_channel_unsupported())
    }

    /// Write: preview an automation rule against current messages.
    async fn preview_automation_rule(
        &self,
        mutation: AutomationRulePreviewMutation,
    ) -> Result<AutomationRulePreviewResult, RuntimeError> {
        let _ = mutation;
        Err(write_channel_unsupported())
    }

    /// Write: create a smart mailbox.
    async fn create_smart_mailbox(
        &self,
        mutation: CreateSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError> {
        let _ = mutation;
        Err(write_channel_unsupported())
    }

    /// Write: patch a smart mailbox.
    async fn patch_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
        mutation: PatchSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError> {
        let _ = (smart_mailbox_id, mutation);
        Err(write_channel_unsupported())
    }

    /// Write: delete a smart mailbox.
    async fn delete_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<(), RuntimeError> {
        let _ = smart_mailbox_id;
        Err(write_channel_unsupported())
    }

    /// Write: reset the default smart mailboxes.
    async fn reset_default_smart_mailboxes(
        &self,
    ) -> Result<Vec<SmartMailboxSummary>, RuntimeError> {
        Err(write_channel_unsupported())
    }

    /// Write: create an account.
    async fn create_account(
        &self,
        mutation: CreateAccountMutation,
    ) -> Result<AccountOverview, RuntimeError> {
        let _ = mutation;
        Err(write_channel_unsupported())
    }

    /// Write: patch an account.
    async fn patch_account(
        &self,
        account_id: AccountId,
        mutation: PatchAccountMutation,
    ) -> Result<AccountOverview, RuntimeError> {
        let _ = (account_id, mutation);
        Err(write_channel_unsupported())
    }

    /// Write: delete an account.
    async fn delete_account(&self, account_id: AccountId) -> Result<(), RuntimeError> {
        let _ = account_id;
        Err(write_channel_unsupported())
    }

    /// Write: verify an account's connectivity/credentials.
    async fn verify_account(
        &self,
        account_id: AccountId,
    ) -> Result<AccountVerificationResult, RuntimeError> {
        let _ = account_id;
        Err(write_channel_unsupported())
    }

    /// Write: enable or disable an account.
    async fn set_account_enabled(
        &self,
        account_id: AccountId,
        enabled: bool,
    ) -> Result<(), RuntimeError> {
        let _ = (account_id, enabled);
        Err(write_channel_unsupported())
    }

    /// Write: reload configuration from disk.
    async fn reload_config(&self) -> Result<(), RuntimeError> {
        Err(write_channel_unsupported())
    }
}

fn read_channel_unsupported() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorCode::Internal,
        "link transport does not carry the read channel",
    )
}

fn write_channel_unsupported() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorCode::Internal,
        "link transport does not carry the write channel",
    )
}

/// The runtime↔authority-server link ([replication authority-server-link L1 §3](../replication/authority-server-link/L1.md)): the
/// runtime's typed handle to the authority server, carrying the seam's two
/// trait halves over ONE swappable transport (D33) — the [`AuthorityServerApi`]
/// request/response surface and the [`AuthorityServerLink`] replication
/// channels. Both `Arc`s point at the same config-selected transport object;
/// carrying them separately is what lets a subset consumer (the runtime's
/// `ReadCache`, the pending-set forwarding path) hold only the half it uses.
///
/// This is the runtime↔authority-server *instantiation* of the shared contract. The
/// client↔runtime link is the same contract carried by the same transport
/// abstraction, so there is one mechanism, two consumers.
#[derive(Clone)]
pub struct AuthorityServerLinkHandle {
    api: Arc<dyn AuthorityServerApi>,
    link: Arc<dyn AuthorityServerLink>,
}

impl AuthorityServerLinkHandle {
    /// Build an authority-server link over a transport implementing both trait
    /// halves. The transport is config-selected upstream
    /// ([replication authority-server-link L2 §6](../replication/authority-server-link/L2.md)); this type does not
    /// know or care which one it holds.
    pub fn new<T>(transport: Arc<T>) -> Self
    where
        T: AuthorityServerApi + AuthorityServerLink + 'static,
    {
        Self {
            api: transport.clone(),
            link: transport,
        }
    }

    /// Build a handle from separately held halves — the decorator seam: a test
    /// wrapper may intercept one half (e.g. gate the up-channel) while the
    /// other keeps pointing at the real transport.
    pub fn from_parts(
        api: Arc<dyn AuthorityServerApi>,
        link: Arc<dyn AuthorityServerLink>,
    ) -> Self {
        Self { api, link }
    }

    /// The Api half, for consumers that only request (the runtime's `ReadCache`).
    pub fn api(&self) -> &Arc<dyn AuthorityServerApi> {
        &self.api
    }

    /// The Link half, for consumers that only replicate (forwarding, subscribe,
    /// the op-lifecycle) — and for `link_router` to serve the wire.
    pub fn link(&self) -> &Arc<dyn AuthorityServerLink> {
        &self.link
    }

    /// Forward a named mutation up to the authority server (up-channel).
    pub async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        self.link.forward_mutation(mutation).await
    }

    /// Subscribe to the authority server's authoritative base-assertion stream
    /// (down-channel). `after_seq` is the resume cursor (D46): `None` for a fresh
    /// stream, `Some(seq)` to resume from just after the last seq seen.
    pub async fn subscribe(
        &self,
        coverage: LinkCoverage,
        after_seq: Option<u64>,
    ) -> Result<DownStream, RuntimeError> {
        self.link.subscribe(coverage, after_seq).await
    }

    /// Op-lifecycle: discard a pending outbox operation at the authority server.
    pub async fn discard_operation(&self, operation_id: OperationId) -> Result<(), RuntimeError> {
        self.link.discard_operation(operation_id).await
    }

    /// Op-lifecycle: re-arm a failed outbox operation at the authority server.
    pub async fn retry_operation(
        &self,
        account_id: AccountId,
        operation_id: OperationId,
    ) -> Result<(), RuntimeError> {
        self.link.retry_operation(account_id, operation_id).await
    }

    /// Apply a mail operation authoritatively (the direct-apply command entry,
    /// D34) and return its ack.
    pub async fn apply(&self, op: MailOperation) -> Result<CommandAck, RuntimeError> {
        self.api.apply(op).await
    }

    /// Read channel: read a mail-list query page through to the authority server (the
    /// authority owns the query engine). A near node reads through here on a
    /// cache miss.
    pub async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        self.api.query_mail_page(request).await
    }

    /// Read channel: the current summary of one message through to the authority server.
    pub async fn current_summary(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<Option<MessageSummary>, RuntimeError> {
        self.api.current_summary(account_id, message_id).await
    }

    /// Read channel: a message's detail through to the authority server.
    pub async fn message_detail(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        self.api.message_detail(account_id, message_id).await
    }

    /// Read channel: a conversation through to the authority server.
    pub async fn conversation(
        &self,
        conversation_id: ConversationId,
    ) -> Result<ConversationView, RuntimeError> {
        self.api.conversation(conversation_id).await
    }
}

/// Generate `AuthorityServerLinkHandle`'s per-op delegations from the shared
/// Api-op table: each forwards straight to the wrapped transport's Api half, so
/// the handle surface cannot drift from [`AuthorityServerApi`]. The Link half
/// (`forward_mutation`/`subscribe`/`discard_operation`/`retry_operation`), the
/// direct-apply entry (`apply`), the four read-channel methods that are not
/// table rows (`query_mail_page`/`current_summary`/`message_detail`/
/// `conversation`), and the constructors/accessors stay hand-written above.
macro_rules! authority_server_api_delegations {
    ($($method:ident => $path:literal => $req:ident { $($field:ident : $fty:ty),* $(,)? } => $ret:ty;)*) => {
        impl AuthorityServerLinkHandle {
            $(
                pub async fn $method(&self, $($field: $fty),*) -> Result<$ret, RuntimeError> {
                    self.api.$method($($field),*).await
                }
            )*
        }
    };
}

/// The canonical runtime↔authority-server **Api-op** table — one source of truth
/// for the remote wire's [`AuthorityServerApi`] rows
/// ([replication authority-server-link L2 §2](../replication/authority-server-link/L2.md)). Each row is
/// `method => "path" => RequestStruct { field: Type, .. } => ReturnType`.
///
/// This is an *x-macro*: invoke it with an emitter macro and it expands to the
/// emitter applied to the whole table. Three emitters consume it, so the wire
/// cannot drift — the request structs (here), the [`RemoteAuthorityServer`] client
/// methods (`posthaste-runtime`), and the `link_router` handlers + routes
/// (`posthaste-authority-server`) are all generated from this one list. Types are
/// written fully-qualified so the table expands correctly in every crate.
///
/// Only the Api request/response ops live here. The link mechanics
/// (`forward_mutation`/`subscribe`) keep their bespoke handlers; the two
/// op-lifecycle rows live in [`for_each_link_lifecycle_op`]; the five
/// message-command routes are served by [`MailCommandRequest`] through
/// [`AuthorityServerApi::apply`] (their paths + request structs are hand-kept
/// below, wire-identical).
#[macro_export]
macro_rules! for_each_link_api_op {
    ($emit:ident) => {
        $emit! {
            // ===== reads =====
            list_accounts => "/v1/link/accounts" => ListAccountsRequest {}
                => $crate::reexport::RuntimeAccountList;
            get_account => "/v1/link/account" => GetAccountRequest {
                account_id: $crate::reexport::AccountId
            } => Option<$crate::reexport::AccountOverview>;
            app_settings => "/v1/link/app-settings" => AppSettingsRequest {}
                => $crate::reexport::AppSettings;
            resolve_account_scope => "/v1/link/resolve-scope" => ResolveAccountScopeRequest {
                scope: $crate::reexport::AccountScopeRequest
            } => Vec<$crate::reexport::AccountId>;
            list_mailboxes => "/v1/link/mailboxes" => ListMailboxesRequest {
                scope: $crate::reexport::AccountScopeRequest
            } => std::collections::BTreeMap<$crate::reexport::AccountId, Vec<$crate::reexport::MailboxSummary>>;
            list_smart_mailboxes => "/v1/link/smart-mailboxes" => ListSmartMailboxesRequest {}
                => Vec<$crate::reexport::SmartMailboxSummary>;
            get_smart_mailbox => "/v1/link/smart-mailbox" => GetSmartMailboxRequest {
                smart_mailbox_id: $crate::reexport::SmartMailboxId
            } => $crate::reexport::SmartMailbox;
            list_tags => "/v1/link/tags" => ListTagsRequest {
                scope: $crate::reexport::AccountScopeRequest
            } => Vec<$crate::reexport::TagSummary>;
            get_identity => "/v1/link/identity" => GetIdentityRequest {
                account_id: $crate::reexport::AccountId
            } => $crate::reexport::Identity;
            get_reply_context => "/v1/link/reply-context" => GetReplyContextRequest {
                account_id: $crate::reexport::AccountId,
                message_id: $crate::reexport::MessageId
            } => $crate::reexport::ReplyContext;
            list_sender_addresses => "/v1/link/sender-addresses" => ListSenderAddressesRequest {}
                => Vec<$crate::reexport::CachedSenderAddress>;
            list_pending_operations => "/v1/link/pending-operations" => ListPendingOperationsRequest {
                account_id: $crate::reexport::AccountId
            } => Vec<$crate::reexport::Operation>;
            replay_events => "/v1/link/events" => ReplayEventsRequest {
                filter: $crate::reexport::EventFilter
            } => Vec<$crate::reexport::DomainEvent>;
            get_draft_content => "/v1/link/draft-content" => GetDraftContentRequest {
                account_id: $crate::reexport::AccountId,
                message_id: $crate::reexport::MessageId
            } => $crate::reexport::DraftContent;
            get_message_resource => "/v1/link/message-resource" => GetMessageResourceRequest {
                account_id: $crate::reexport::AccountId,
                message_id: $crate::reexport::MessageId,
                kind: $crate::reexport::MessageResourceKind
            } => $crate::reexport::RuntimeResourceBytes;
            account_count => "/v1/link/account-count" => AccountCountRequest {}
                => Option<usize>;

            // ===== writes =====
            set_mailbox_role => "/v1/link/set-mailbox-role" => SetMailboxRoleRequest {
                account_id: $crate::reexport::AccountId,
                mailbox_id: $crate::reexport::MailboxId,
                role: Option<String>
            } => Vec<$crate::reexport::MailboxSummary>;
            create_mailbox => "/v1/link/create-mailbox" => CreateMailboxRequest {
                account_id: $crate::reexport::AccountId,
                name: String
            } => Vec<$crate::reexport::MailboxSummary>;
            destroy_mailbox => "/v1/link/destroy-mailbox" => DestroyMailboxRequest {
                account_id: $crate::reexport::AccountId,
                mailbox_id: $crate::reexport::MailboxId,
                remove_emails: bool
            } => Vec<$crate::reexport::MailboxSummary>;
            send_message => "/v1/link/send-message" => SendMessageLinkRequest {
                account_id: $crate::reexport::AccountId,
                request: $crate::reexport::SendMessageRequest
            } => ();
            save_draft => "/v1/link/save-draft" => SaveDraftRequest {
                account_id: $crate::reexport::AccountId,
                draft_id: Option<$crate::reexport::MessageId>,
                request: $crate::reexport::SendMessageRequest
            } => $crate::reexport::Operation;
            delete_draft => "/v1/link/delete-draft" => DeleteDraftRequest {
                account_id: $crate::reexport::AccountId,
                draft_id: $crate::reexport::MessageId
            } => $crate::reexport::Operation;
            sync_account => "/v1/link/sync-account" => SyncAccountRequest {
                account_id: $crate::reexport::AccountId,
                mode: $crate::reexport::SyncMode
            } => usize;
            patch_app_settings => "/v1/link/patch-app-settings" => PatchAppSettingsLinkRequest {
                mutation: $crate::reexport::PatchAppSettingsMutation
            } => $crate::reexport::AppSettings;
            preview_automation_rule => "/v1/link/preview-automation-rule" => PreviewAutomationRuleRequest {
                mutation: $crate::reexport::AutomationRulePreviewMutation
            } => $crate::reexport::AutomationRulePreviewResult;
            create_smart_mailbox => "/v1/link/create-smart-mailbox" => CreateSmartMailboxLinkRequest {
                mutation: $crate::reexport::CreateSmartMailboxMutation
            } => $crate::reexport::SmartMailbox;
            patch_smart_mailbox => "/v1/link/patch-smart-mailbox" => PatchSmartMailboxLinkRequest {
                smart_mailbox_id: $crate::reexport::SmartMailboxId,
                mutation: $crate::reexport::PatchSmartMailboxMutation
            } => $crate::reexport::SmartMailbox;
            delete_smart_mailbox => "/v1/link/delete-smart-mailbox" => DeleteSmartMailboxRequest {
                smart_mailbox_id: $crate::reexport::SmartMailboxId
            } => ();
            reset_default_smart_mailboxes => "/v1/link/reset-smart-mailboxes" => ResetDefaultSmartMailboxesRequest {}
                => Vec<$crate::reexport::SmartMailboxSummary>;
            create_account => "/v1/link/create-account" => CreateAccountLinkRequest {
                mutation: $crate::reexport::CreateAccountMutation
            } => $crate::reexport::AccountOverview;
            patch_account => "/v1/link/patch-account" => PatchAccountLinkRequest {
                account_id: $crate::reexport::AccountId,
                mutation: $crate::reexport::PatchAccountMutation
            } => $crate::reexport::AccountOverview;
            delete_account => "/v1/link/delete-account" => DeleteAccountRequest {
                account_id: $crate::reexport::AccountId
            } => ();
            verify_account => "/v1/link/verify-account" => VerifyAccountRequest {
                account_id: $crate::reexport::AccountId
            } => $crate::reexport::AccountVerificationResult;
            set_account_enabled => "/v1/link/set-account-enabled" => SetAccountEnabledRequest {
                account_id: $crate::reexport::AccountId,
                enabled: bool
            } => ();
            reload_config => "/v1/link/reload-config" => ReloadConfigRequest {}
                => ();
        }
    };
}

/// The [`AuthorityServerLink`] op-lifecycle rows of the wire — the two outbox
/// lifecycle *mutations* that ride the Link half (D33's op-lifecycle rule);
/// same row shape and emitter protocol as [`for_each_link_api_op`], kept as a
/// separate table so each emitter can route to the right trait half.
#[macro_export]
macro_rules! for_each_link_lifecycle_op {
    ($emit:ident) => {
        $emit! {
            discard_operation => "/v1/link/discard-operation" => DiscardOperationRequest {
                operation_id: $crate::reexport::OperationId
            } => ();
            retry_operation => "/v1/link/retry-operation" => RetryOperationRequest {
                account_id: $crate::reexport::AccountId,
                operation_id: $crate::reexport::OperationId
            } => ();
        }
    };
}

/// Re-exports so [`for_each_link_api_op`] (and its sibling tables) can name
/// contract types with a single stable path that resolves in every crate that
/// expands the table (`posthaste_contract_core` may not be a direct dependency
/// name everywhere, but `posthaste_authority_server_link` always is).
pub mod reexport {
    pub use posthaste_contract_core::{
        AccountScopeRequest, AccountVerificationResult, AutomationRulePreviewMutation,
        AutomationRulePreviewResult, CreateAccountMutation, CreateSmartMailboxMutation,
        MessageResourceKind, PatchAccountMutation, PatchAppSettingsMutation,
        PatchSmartMailboxMutation, RuntimeAccountList, RuntimeResourceBytes,
    };
    pub use posthaste_domain_model::{
        AccountId, AccountOverview, AddToMailboxCommand, AppSettings, CachedSenderAddress,
        CommandAck, DomainEvent, DraftContent, EventFilter, Identity, MailboxId, MailboxSummary,
        MessageId, Operation, OperationId, RemoveFromMailboxCommand, ReplaceMailboxesCommand,
        ReplyContext, SendMessageRequest, SetKeywordsCommand, SmartMailbox, SmartMailboxId,
        SmartMailboxSummary, SyncMode, TagSummary,
    };
}

/// Generate the shared request struct for every link op (one per row of
/// [`for_each_link_api_op`] / [`for_each_link_lifecycle_op`]). Both the client
/// and the server deserialize the same type, so the wire shape has a single
/// definition.
macro_rules! define_link_request_structs {
    ($($method:ident => $path:literal => $req:ident { $($field:ident : $fty:ty),* $(,)? } => $ret:ty;)*) => {
        $(
            #[derive(Debug, Serialize, Deserialize)]
            #[serde(rename_all = "camelCase")]
            pub struct $req { $(pub $field: $fty),* }
        )*
    };
}
for_each_link_api_op!(define_link_request_structs);
for_each_link_lifecycle_op!(define_link_request_structs);
for_each_link_api_op!(authority_server_api_delegations);

// ===== The message-command wire (M5b) =====
//
// The five per-command message routes survive the D33/D34 apply-collapse
// byte-for-byte — same paths, same request/response JSON — but the trait entry
// is the single `AuthorityServerApi::apply(op)`. `MailCommandRequest` is the
// bridge: the typed-op ⇄ per-command-wire mapping lives here, once, so the
// remote client (`RemoteAuthorityServer::apply`) and the far-node handlers
// (`link_wire`) cannot drift.

/// Wire path for the `message.setKeywords` direct-apply command.
pub const LINK_SET_KEYWORDS_PATH: &str = "/v1/link/set-keywords";
/// Wire path for the `message.addToMailbox` direct-apply command.
pub const LINK_ADD_TO_MAILBOX_PATH: &str = "/v1/link/add-to-mailbox";
/// Wire path for the `message.removeFromMailbox` direct-apply command.
pub const LINK_REMOVE_FROM_MAILBOX_PATH: &str = "/v1/link/remove-from-mailbox";
/// Wire path for the `message.replaceMailboxes` direct-apply command.
pub const LINK_REPLACE_MAILBOXES_PATH: &str = "/v1/link/replace-mailboxes";
/// Wire path for the `message.destroy` direct-apply command.
pub const LINK_DESTROY_MESSAGE_PATH: &str = "/v1/link/destroy-message";

/// `POST /v1/link/set-keywords` request (wire-identical to the pre-split row).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetKeywordsRequest {
    pub account_id: AccountId,
    pub message_id: MessageId,
    pub command: SetKeywordsCommand,
}

/// `POST /v1/link/add-to-mailbox` request (wire-identical to the pre-split row).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddToMailboxRequest {
    pub account_id: AccountId,
    pub message_id: MessageId,
    pub command: AddToMailboxCommand,
}

/// `POST /v1/link/remove-from-mailbox` request (wire-identical to the pre-split row).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveFromMailboxRequest {
    pub account_id: AccountId,
    pub message_id: MessageId,
    pub command: RemoveFromMailboxCommand,
}

/// `POST /v1/link/replace-mailboxes` request (wire-identical to the pre-split row).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceMailboxesRequest {
    pub account_id: AccountId,
    pub message_id: MessageId,
    pub command: ReplaceMailboxesCommand,
}

/// `POST /v1/link/destroy-message` request (wire-identical to the pre-split row).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DestroyMessageRequest {
    pub account_id: AccountId,
    pub message_id: MessageId,
}

/// One typed message-command wire op: the projection of a direct-apply
/// [`MailOperation`] onto the five preserved per-command routes. Untagged, so
/// serializing a variant emits exactly its request struct's JSON — the wire
/// bytes are unchanged from the pre-split per-command RPCs.
///
/// Every implementor of [`AuthorityServerApi::apply`] routes through
/// [`from_operation`](Self::from_operation), so the op→command dispatch (and
/// the rejection of replica-only operations) has exactly one home.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum MailCommandRequest {
    SetKeywords(SetKeywordsRequest),
    AddToMailbox(AddToMailboxRequest),
    RemoveFromMailbox(RemoveFromMailboxRequest),
    ReplaceMailboxes(ReplaceMailboxesRequest),
    Destroy(DestroyMessageRequest),
}

impl MailCommandRequest {
    /// Project a direct-apply operation onto its command wire shape. Operations
    /// that only exist on the replica forward path (role moves, snooze,
    /// applyDiff, the `revCursor` control op) have no direct authority command
    /// and are rejected with `InvalidMutation` — they must flow through
    /// `AuthorityServerLink::forward_mutation`.
    pub fn from_operation(op: MailOperation) -> Result<Self, RuntimeError> {
        let account_id = AccountId(op.account_id().to_string());
        let message_id = MessageId(
            op.message_id()
                .ok_or_else(|| {
                    RuntimeError::invalid_mutation(format!(
                        "operation '{}' has no direct-apply command surface",
                        op.name()
                    ))
                })?
                .to_string(),
        );
        Ok(match op {
            MailOperation::SetKeywords(args) => Self::SetKeywords(SetKeywordsRequest {
                account_id,
                message_id,
                command: args.command,
            }),
            // The keyword-shaped semantic operations are pure projections onto
            // the SetKeywords command — the same folding `apply_operation` does
            // on the far node. Without these arms a direct-apply caller (the
            // rule engine's tag/markRead/flag actions) would be rejected as
            // "replica-only" even though the effect is a plain keyword write.
            MailOperation::SetUserTags(args) => Self::SetKeywords(SetKeywordsRequest {
                account_id,
                message_id,
                command: SetKeywordsCommand {
                    add: args.add,
                    remove: args.remove,
                },
            }),
            MailOperation::SetReadState(args) => Self::SetKeywords(SetKeywordsRequest {
                account_id,
                message_id,
                command: keyword_toggle("$seen", args.read),
            }),
            MailOperation::SetFlaggedState(args) => Self::SetKeywords(SetKeywordsRequest {
                account_id,
                message_id,
                command: keyword_toggle("$flagged", args.flagged),
            }),
            MailOperation::AddToMailbox(args) => Self::AddToMailbox(AddToMailboxRequest {
                account_id,
                message_id,
                command: AddToMailboxCommand {
                    mailbox_id: MailboxId(args.mailbox_id),
                },
            }),
            MailOperation::RemoveFromMailbox(args) => {
                Self::RemoveFromMailbox(RemoveFromMailboxRequest {
                    account_id,
                    message_id,
                    command: RemoveFromMailboxCommand {
                        mailbox_id: MailboxId(args.mailbox_id),
                    },
                })
            }
            MailOperation::ReplaceMailboxes(args) => {
                Self::ReplaceMailboxes(ReplaceMailboxesRequest {
                    account_id,
                    message_id,
                    command: ReplaceMailboxesCommand {
                        mailbox_ids: args.mailbox_ids.into_iter().map(MailboxId).collect(),
                    },
                })
            }
            MailOperation::Destroy(_) => Self::Destroy(DestroyMessageRequest {
                account_id,
                message_id,
            }),
            other => {
                return Err(RuntimeError::invalid_mutation(format!(
                    "operation '{}' has no direct-apply command surface; forward it as a mutation",
                    other.name()
                )))
            }
        })
    }

    /// The wire path this command POSTs to (the pre-split per-command route).
    pub fn path(&self) -> &'static str {
        match self {
            Self::SetKeywords(_) => LINK_SET_KEYWORDS_PATH,
            Self::AddToMailbox(_) => LINK_ADD_TO_MAILBOX_PATH,
            Self::RemoveFromMailbox(_) => LINK_REMOVE_FROM_MAILBOX_PATH,
            Self::ReplaceMailboxes(_) => LINK_REPLACE_MAILBOXES_PATH,
            Self::Destroy(_) => LINK_DESTROY_MESSAGE_PATH,
        }
    }
}

/// The server-side inverse of [`MailCommandRequest::from_operation`]: rebuild
/// the typed [`MailOperation`] from a decoded per-command request so the far
/// node's handlers route through [`AuthorityServerApi::apply`] (one dispatch
/// per implementor, not one per route).
impl SetKeywordsRequest {
    pub fn into_operation(self) -> MailOperation {
        MailOperation::SetKeywords(
            posthaste_contract_core::mutation_args::MessageSetKeywordsMutationArgs {
                source_id: self.account_id.as_str().to_string(),
                message_id: self.message_id.as_str().to_string(),
                command: self.command,
            },
        )
    }
}

impl AddToMailboxRequest {
    pub fn into_operation(self) -> MailOperation {
        MailOperation::AddToMailbox(
            posthaste_contract_core::mutation_args::MessageMailboxMembershipArgs {
                source_id: self.account_id.as_str().to_string(),
                message_id: self.message_id.as_str().to_string(),
                mailbox_id: self.command.mailbox_id.as_str().to_string(),
            },
        )
    }
}

impl RemoveFromMailboxRequest {
    pub fn into_operation(self) -> MailOperation {
        MailOperation::RemoveFromMailbox(
            posthaste_contract_core::mutation_args::MessageMailboxMembershipArgs {
                source_id: self.account_id.as_str().to_string(),
                message_id: self.message_id.as_str().to_string(),
                mailbox_id: self.command.mailbox_id.as_str().to_string(),
            },
        )
    }
}

impl ReplaceMailboxesRequest {
    pub fn into_operation(self) -> MailOperation {
        MailOperation::ReplaceMailboxes(
            posthaste_contract_core::mutation_args::MessageReplaceMailboxesArgs {
                source_id: self.account_id.as_str().to_string(),
                message_id: self.message_id.as_str().to_string(),
                mailbox_ids: self
                    .command
                    .mailbox_ids
                    .into_iter()
                    .map(|id| id.as_str().to_string())
                    .collect(),
            },
        )
    }
}

impl DestroyMessageRequest {
    pub fn into_operation(self) -> MailOperation {
        MailOperation::Destroy(posthaste_contract_core::mutation_args::MessageTargetArgs {
            source_id: self.account_id.as_str().to_string(),
            message_id: self.message_id.as_str().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keyword-shaped semantic ops (`setUserTags`, `setReadState`,
    /// `setFlaggedState`) project onto the SetKeywords direct-apply command —
    /// this is what lets the rule engine's tag / markRead / flag actions run
    /// through `AuthorityServerApi::apply` (they used to be rejected as
    /// "replica-only", which silently failed every Level-0 tag rule).
    #[test]
    fn keyword_shaped_operations_project_onto_set_keywords() {
        use posthaste_contract_core::mutation_args::{
            MessageSetFlaggedStateArgs, MessageSetReadStateArgs, MessageSetUserTagsArgs,
        };

        let tags = MailCommandRequest::from_operation(MailOperation::SetUserTags(
            MessageSetUserTagsArgs {
                source_id: "acct".into(),
                message_id: "m1".into(),
                add: vec!["receipt".into()],
                remove: vec!["todo".into()],
            },
        ))
        .expect("setUserTags must have a direct-apply projection");
        match tags {
            MailCommandRequest::SetKeywords(request) => {
                assert_eq!(request.command.add, vec!["receipt".to_string()]);
                assert_eq!(request.command.remove, vec!["todo".to_string()]);
            }
            other => panic!("expected SetKeywords, got {other:?}"),
        }

        let read = MailCommandRequest::from_operation(MailOperation::SetReadState(
            MessageSetReadStateArgs {
                source_id: "acct".into(),
                message_id: "m1".into(),
                read: true,
            },
        ))
        .expect("setReadState must have a direct-apply projection");
        match read {
            MailCommandRequest::SetKeywords(request) => {
                assert_eq!(request.command.add, vec!["$seen".to_string()]);
                assert!(request.command.remove.is_empty());
            }
            other => panic!("expected SetKeywords, got {other:?}"),
        }

        let unflag = MailCommandRequest::from_operation(MailOperation::SetFlaggedState(
            MessageSetFlaggedStateArgs {
                source_id: "acct".into(),
                message_id: "m1".into(),
                flagged: false,
            },
        ))
        .expect("setFlaggedState must have a direct-apply projection");
        match unflag {
            MailCommandRequest::SetKeywords(request) => {
                assert!(request.command.add.is_empty());
                assert_eq!(request.command.remove, vec!["$flagged".to_string()]);
            }
            other => panic!("expected SetKeywords, got {other:?}"),
        }
    }

    /// Role moves stay replica-only on this bridge: they need the account's
    /// role→mailbox map, which the direct-apply surface does not carry. (The
    /// rule engine resolves the role itself and applies a ReplaceMailboxes.)
    #[test]
    fn role_moves_are_still_rejected_by_the_direct_apply_bridge() {
        use posthaste_contract_core::mutation_args::MessageMoveToRoleArgs;
        let result =
            MailCommandRequest::from_operation(MailOperation::MoveToRole(MessageMoveToRoleArgs {
                source_id: "acct".into(),
                message_id: "m1".into(),
                role: "archive".into(),
            }));
        assert!(result.is_err(), "moveToRole has no direct-apply command");
    }

    #[test]
    fn down_frame_base_round_trips_through_json() {
        let frame = AuthorityServerFrame::Base {
            assertions: vec![
                BaseAssertion {
                    account_id: "acct".into(),
                    message_id: "m1".into(),
                    update: BaseUpdate::Present(MessageFoldState {
                        keywords: vec!["$flagged".into()],
                        mailbox_ids: vec!["inbox".into()],
                    }),
                    event: None,
                },
                BaseAssertion {
                    account_id: "acct".into(),
                    message_id: "m2".into(),
                    update: BaseUpdate::Removed,
                    event: None,
                },
            ],
        };
        let json = serde_json::to_value(&frame).expect("serialize");
        assert_eq!(json["type"], "base");
        assert_eq!(json["assertions"][0]["update"]["kind"], "present");
        assert_eq!(json["assertions"][1]["update"]["kind"], "removed");
        let restored: AuthorityServerFrame = serde_json::from_value(json).expect("deserialize");
        assert_eq!(restored, frame);
    }

    #[test]
    fn down_frame_settlement_carries_the_per_mutation_watermark() {
        let frame = AuthorityServerFrame::Settlement {
            mutation_id: MutationId("op1".into()),
            outcome: WireSettlementOutcome::Confirmed,
        };
        let json = serde_json::to_value(&frame).expect("serialize");
        assert_eq!(json["type"], "settlement");
        assert_eq!(json["mutationId"], "op1");
        assert_eq!(json["outcome"], "confirmed");
        let restored: AuthorityServerFrame = serde_json::from_value(json).expect("deserialize");
        assert_eq!(restored, frame);
    }

    #[test]
    fn settlement_outcome_bridges_the_engine_type() {
        assert_eq!(
            SettlementOutcome::from(WireSettlementOutcome::Failed),
            SettlementOutcome::Failed
        );
        assert_eq!(
            WireSettlementOutcome::from(SettlementOutcome::Confirmed),
            WireSettlementOutcome::Confirmed
        );
    }

    #[test]
    fn link_coverage_defaults_to_complete() {
        assert_eq!(LinkCoverage::default(), LinkCoverage::Complete);
        let json = serde_json::to_value(LinkCoverage::Complete).expect("serialize");
        assert_eq!(json["kind"], "complete");
    }

    // The op ⇄ command-wire bridge behind `AuthorityServerApi::apply`: the wire
    // JSON is byte-identical to the pre-split per-command request rows, the
    // server-side inverse rebuilds the same op, and replica-only operations are
    // rejected before any wire/store dispatch.
    #[test]
    fn direct_apply_command_wire_round_trips_the_operation() {
        let op: MailOperation = serde_json::from_value(serde_json::json!({
            "name": "message.setKeywords",
            "args": {
                "sourceId": "acct",
                "messageId": "m1",
                "command": {"add": ["$flagged"], "remove": []},
            },
        }))
        .expect("typed operation parses");

        let command = MailCommandRequest::from_operation(op.clone()).expect("a command op");
        assert_eq!(command.path(), LINK_SET_KEYWORDS_PATH);
        // The untagged wire shape is exactly the pre-split request struct's.
        let wire = serde_json::to_value(&command).expect("serialize");
        assert_eq!(
            wire,
            serde_json::json!({
                "accountId": "acct",
                "messageId": "m1",
                "command": {"add": ["$flagged"], "remove": []},
            })
        );
        // The server-side inverse rebuilds the same typed op.
        let request: SetKeywordsRequest = serde_json::from_value(wire).expect("deserialize");
        assert_eq!(request.into_operation(), op);
    }

    #[test]
    fn replica_only_operations_are_rejected_by_the_command_bridge() {
        let op: MailOperation = serde_json::from_value(serde_json::json!({
            "name": "message.moveToRole",
            "args": { "sourceId": "acct", "messageId": "m1", "role": "archive" },
        }))
        .expect("typed operation parses");
        let error = MailCommandRequest::from_operation(op).expect_err("no direct-apply surface");
        assert_eq!(error.envelope().code, RuntimeErrorCode::InvalidMutation);

        let cursor: MailOperation = serde_json::from_value(serde_json::json!({
            "name": "revCursor",
            "args": { "accountId": "acct", "cursorStepId": null, "redoTail": [] },
        }))
        .expect("typed operation parses");
        let error =
            MailCommandRequest::from_operation(cursor).expect_err("control ops target no message");
        assert_eq!(error.envelope().code, RuntimeErrorCode::InvalidMutation);
    }

    // A trivial in-memory transport proves the trait pair is object-safe and
    // usable — the shape `LocalAuthorityServer` (W1) and `RemoteAuthorityServer`
    // (W3) implement. The Api half is all defaults (this stub carries no read
    // channel).
    struct StubTransport;

    #[async_trait]
    impl AuthorityServerApi for StubTransport {}

    #[async_trait]
    impl AuthorityServerLink for StubTransport {
        async fn forward_mutation(
            &self,
            mutation: MutationRequest,
        ) -> Result<MutationReceipt, RuntimeError> {
            Ok(MutationReceipt {
                runtime_mutation_id: None,
                client_mutation_id: mutation.client_mutation_id,
                name: mutation.operation.name().to_string(),
                state: posthaste_contract_core::MutationSettlementState::Accepted,
                error: None,
                output: serde_json::Value::Null,
            })
        }

        async fn subscribe(
            &self,
            _coverage: LinkCoverage,
            _after_seq: Option<u64>,
        ) -> Result<DownStream, RuntimeError> {
            Ok(Box::pin(futures_util::stream::iter([SequencedFrame::new(
                1,
                AuthorityServerFrame::Heartbeat,
            )])))
        }
    }

    #[tokio::test]
    async fn authority_server_link_forwards_through_its_transport() {
        use futures_util::StreamExt;
        use posthaste_contract_core::ClientMutationId;

        let link = AuthorityServerLinkHandle::new(Arc::new(StubTransport));
        let receipt = link
            .forward_mutation(
                serde_json::from_value(serde_json::json!({
                    "name": "message.setKeywords",
                    "args": {
                        "sourceId": "acct",
                        "messageId": "m1",
                        "command": {"add": [], "remove": []},
                    },
                    "clientMutationId": "c1",
                }))
                .expect("request builds from the flat wire shape"),
            )
            .await
            .expect("forward");
        assert_eq!(receipt.client_mutation_id, ClientMutationId::new("c1"));

        let mut down = link
            .subscribe(LinkCoverage::Complete, None)
            .await
            .expect("subscribe");
        assert_eq!(
            down.next().await.and_then(|s| s.frame().cloned()),
            Some(AuthorityServerFrame::Heartbeat)
        );
    }
}
