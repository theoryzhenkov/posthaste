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
//! [`AuthorityServerLink`] is the Rust abstraction over this link's two channels; it is
//! selected by configuration (in-process co-located by default, remote when
//! split — [replication authority-server-link L2 §6](../replication/authority-server-link/L2.md)). The transport is what
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

use posthaste_domain_model::{
    AccountId, AccountOverview, AddToMailboxCommand, AppSettings, CachedSenderAddress, CommandAck,
    ConversationId, ConversationView, DomainEvent, DraftContent, EventFilter, Identity, MailboxId,
    MailboxSummary, MessageDetail, MessageId, MessageSummary, Operation, OperationId,
    RemoveFromMailboxCommand, ReplaceMailboxesCommand, ReplyContext, RevLogSnapshot,
    SendMessageRequest, SetKeywordsCommand, SmartMailbox, SmartMailboxId, SmartMailboxSummary,
    SyncMode, TagSummary,
};
use posthaste_link_core::{MessageFoldState, MutationId, SettlementOutcome};
use posthaste_contract_core::{
    AccountScopeRequest, AccountVerificationResult, AutomationRulePreviewMutation,
    AutomationRulePreviewResult, CreateAccountMutation, CreateSmartMailboxMutation, MailQueryPage,
    MailQueryRequest, MessageResourceKind, MutationReceipt, MutationRequest, PatchAccountMutation,
    PatchAppSettingsMutation, PatchSmartMailboxMutation, RuntimeAccountList, RuntimeError,
    RuntimeErrorCode, RuntimeResourceBytes,
};

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
/// This is the wire-shaped, serializable twin of `link_core::MessageBaseUpdate`
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseAssertion {
    /// The account the message belongs to. Carried so a near node can scope the
    /// change (cache eviction, view recompute) to the right account rather than
    /// matching on the bare message id.
    pub account_id: String,
    pub message_id: String,
    pub update: BaseUpdate,
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AuthorityServerFrame {
    /// A batch of ordered authoritative base updates to apply to the base cache.
    Base { assertions: Vec<BaseAssertion> },
    /// A forwarded mutation reached its terminal outcome at the far node — the
    /// per-mutation confirmation watermark. Retires the matching outbox entry.
    Settlement {
        /// The engine's mutation id, carried directly on the wire (D12 — no
        /// serde mirror type; `MutationId` is already serde and this crate
        /// depends on `link-core`).
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

/// The ordered down-channel: authoritative base assertions + confirmation,
/// tagged with the watermark per [`AuthorityServerFrame`].
pub type DownStream = BoxStream<'static, AuthorityServerFrame>;

/// One link's two channels, transport-neutral. The transport is the only thing
/// that varies across deployments — in-process and co-located by default
/// (W1, behavior-preserving), remote when the far node lives elsewhere
/// (W3) — and is selected by configuration, never at build time
/// ([replication authority-server-link L2 §6](../replication/authority-server-link/L2.md), assertion `transport-selected-by-config`).
///
/// The same trait carries **both** links (assertion `one-link-transport`): the
/// client↔runtime link is conformant by construction (the contract is the wire
/// it already speaks), and the runtime↔authority-server link adopts it via
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
    async fn subscribe(&self, coverage: LinkCoverage) -> Result<DownStream, RuntimeError>;

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
    ) -> Result<DownStream, RuntimeError> {
        let _ = runtime_id;
        self.subscribe(coverage).await
    }

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
    // The named up-channel ([`forward_mutation`](Self::forward_mutation)) carries
    // session-originated message mutations through the replica; these typed
    // commands are the direct (REST) command surface, applied at the far node
    // and returning the typed ack. Default-erroring so a transport that does not
    // carry the write channel is simply not a command sink (the remote wire is
    // wired per-op alongside the reads).

    /// Write: set/clear keywords on a message.
    async fn set_keywords(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        command: SetKeywordsCommand,
    ) -> Result<CommandAck, RuntimeError> {
        let _ = (account_id, message_id, command);
        Err(write_channel_unsupported())
    }

    /// Write: add a message to a mailbox.
    async fn add_to_mailbox(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        command: AddToMailboxCommand,
    ) -> Result<CommandAck, RuntimeError> {
        let _ = (account_id, message_id, command);
        Err(write_channel_unsupported())
    }

    /// Write: remove a message from a mailbox.
    async fn remove_from_mailbox(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        command: RemoveFromMailboxCommand,
    ) -> Result<CommandAck, RuntimeError> {
        let _ = (account_id, message_id, command);
        Err(write_channel_unsupported())
    }

    /// Write: replace a message's mailbox membership.
    async fn replace_mailboxes(
        &self,
        account_id: AccountId,
        message_id: MessageId,
        command: ReplaceMailboxesCommand,
    ) -> Result<CommandAck, RuntimeError> {
        let _ = (account_id, message_id, command);
        Err(write_channel_unsupported())
    }

    /// Write: destroy a message.
    async fn destroy_message(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<CommandAck, RuntimeError> {
        let _ = (account_id, message_id);
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

    /// Write: discard a pending outbox operation.
    async fn discard_operation(&self, operation_id: OperationId) -> Result<(), RuntimeError> {
        let _ = operation_id;
        Err(write_channel_unsupported())
    }

    /// Write: re-arm a failed outbox operation to pending.
    async fn retry_operation(
        &self,
        account_id: AccountId,
        operation_id: OperationId,
    ) -> Result<(), RuntimeError> {
        let _ = (account_id, operation_id);
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
/// runtime's typed handle to the authority server, carried by a swappable
/// [`AuthorityServerLink`]. The runtime reaches the authority server **only** through these two
/// channels — never by reading the authority server store across the link (assertion
/// `authority-server-link-is-replication-only`); reads become state the near node derives
/// locally from its base cache (W2).
///
/// This is the runtime↔authority-server *instantiation* of the shared contract. The
/// client↔runtime link is the same contract carried by the same transport
/// abstraction, so there is one mechanism, two consumers.
#[derive(Clone)]
pub struct AuthorityServerLinkHandle {
    transport: Arc<dyn AuthorityServerLink>,
}

impl AuthorityServerLinkHandle {
    /// Build an authority-server link over a transport. The transport is config-selected
    /// upstream ([replication authority-server-link L2 §6](../replication/authority-server-link/L2.md)); this type does not
    /// know or care which one it holds.
    pub fn new(transport: Arc<dyn AuthorityServerLink>) -> Self {
        Self { transport }
    }

    /// Forward a named mutation up to the authority server (up-channel).
    pub async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        self.transport.forward_mutation(mutation).await
    }

    /// Subscribe to the authority server's authoritative base-assertion stream
    /// (down-channel).
    pub async fn subscribe(&self, coverage: LinkCoverage) -> Result<DownStream, RuntimeError> {
        self.transport.subscribe(coverage).await
    }

    /// Read channel: read a mail-list query page through to the authority server (the
    /// authority owns the query engine). A near node reads through here on a
    /// cache miss.
    pub async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        self.transport.query_mail_page(request).await
    }

    /// Read channel: the current summary of one message through to the authority server.
    pub async fn current_summary(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<Option<MessageSummary>, RuntimeError> {
        self.transport.current_summary(account_id, message_id).await
    }

    /// Read channel: a message's detail through to the authority server.
    pub async fn message_detail(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        self.transport.message_detail(account_id, message_id).await
    }

    /// Read channel: a conversation through to the authority server.
    pub async fn conversation(
        &self,
        conversation_id: ConversationId,
    ) -> Result<ConversationView, RuntimeError> {
        self.transport.conversation(conversation_id).await
    }

    /// The underlying transport, for callers that need to inspect or hold it.
    pub fn transport(&self) -> &Arc<dyn AuthorityServerLink> {
        &self.transport
    }
}

/// Generate `AuthorityServerLinkHandle`'s per-op delegations from the shared link-op table:
/// each forwards straight to the wrapped transport, so the link surface cannot
/// drift from [`AuthorityServerLink`]. The up-channel (`forward_mutation`), down-channel
/// (`subscribe`), the four read-channel methods that are not table rows
/// (`query_mail_page`/`current_summary`/`message_detail`/`conversation`), and the
/// `new`/`transport` accessors stay hand-written above.
macro_rules! authority_server_link_delegations {
    ($($method:ident => $path:literal => $req:ident { $($field:ident : $fty:ty),* $(,)? } => $ret:ty;)*) => {
        impl AuthorityServerLinkHandle {
            $(
                pub async fn $method(&self, $($field: $fty),*) -> Result<$ret, RuntimeError> {
                    self.transport.$method($($field),*).await
                }
            )*
        }
    };
}

/// The canonical runtime↔authority-server link op table — one source of truth for the
/// remote wire ([replication authority-server-link L2 §2](../replication/authority-server-link/L2.md)). Each row is
/// `method => "path" => RequestStruct { field: Type, .. } => ReturnType`.
///
/// This is an *x-macro*: invoke it with an emitter macro and it expands to the
/// emitter applied to the whole table. Three emitters consume it, so the wire
/// cannot drift — the request structs (here), the [`RemoteAuthorityServer`] client
/// methods (`authority-runtime`), and the `link_router` handlers + routes
/// (`posthaste-server`) are all generated from this one list. Types are written
/// fully-qualified so the table expands correctly in every crate.
///
/// Only the request/response ops live here; the up-channel (`forward_mutation`)
/// and the SSE down-channel (`subscribe`) keep their bespoke handlers.
#[macro_export]
macro_rules! for_each_link_op {
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
            set_keywords => "/v1/link/set-keywords" => SetKeywordsRequest {
                account_id: $crate::reexport::AccountId,
                message_id: $crate::reexport::MessageId,
                command: $crate::reexport::SetKeywordsCommand
            } => $crate::reexport::CommandAck;
            add_to_mailbox => "/v1/link/add-to-mailbox" => AddToMailboxRequest {
                account_id: $crate::reexport::AccountId,
                message_id: $crate::reexport::MessageId,
                command: $crate::reexport::AddToMailboxCommand
            } => $crate::reexport::CommandAck;
            remove_from_mailbox => "/v1/link/remove-from-mailbox" => RemoveFromMailboxRequest {
                account_id: $crate::reexport::AccountId,
                message_id: $crate::reexport::MessageId,
                command: $crate::reexport::RemoveFromMailboxCommand
            } => $crate::reexport::CommandAck;
            replace_mailboxes => "/v1/link/replace-mailboxes" => ReplaceMailboxesRequest {
                account_id: $crate::reexport::AccountId,
                message_id: $crate::reexport::MessageId,
                command: $crate::reexport::ReplaceMailboxesCommand
            } => $crate::reexport::CommandAck;
            destroy_message => "/v1/link/destroy-message" => DestroyMessageRequest {
                account_id: $crate::reexport::AccountId,
                message_id: $crate::reexport::MessageId
            } => $crate::reexport::CommandAck;
            set_mailbox_role => "/v1/link/set-mailbox-role" => SetMailboxRoleRequest {
                account_id: $crate::reexport::AccountId,
                mailbox_id: $crate::reexport::MailboxId,
                role: Option<String>
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
            discard_operation => "/v1/link/discard-operation" => DiscardOperationRequest {
                operation_id: $crate::reexport::OperationId
            } => ();
            retry_operation => "/v1/link/retry-operation" => RetryOperationRequest {
                account_id: $crate::reexport::AccountId,
                operation_id: $crate::reexport::OperationId
            } => ();
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

/// Re-exports so [`for_each_link_op`] can name `runtime-contract` types with a
/// single stable path that resolves in every crate that expands the table
/// (`posthaste_contract_core` may not be a direct dependency name everywhere,
/// but `posthaste_authority_server_link` always is).
pub mod reexport {
    pub use posthaste_domain_model::{
        AccountId, AccountOverview, AddToMailboxCommand, AppSettings, CachedSenderAddress,
        CommandAck, DomainEvent, DraftContent, EventFilter, Identity, MailboxId, MailboxSummary,
        MessageId, Operation, OperationId, RemoveFromMailboxCommand, ReplaceMailboxesCommand,
        ReplyContext, SendMessageRequest, SetKeywordsCommand, SmartMailbox, SmartMailboxId,
        SmartMailboxSummary, SyncMode, TagSummary,
    };
    pub use posthaste_contract_core::{
        AccountScopeRequest, AccountVerificationResult, AutomationRulePreviewMutation,
        AutomationRulePreviewResult, CreateAccountMutation, CreateSmartMailboxMutation,
        MessageResourceKind, PatchAccountMutation, PatchAppSettingsMutation,
        PatchSmartMailboxMutation, RuntimeAccountList, RuntimeResourceBytes,
    };
}

/// Generate the shared request struct for every link op (one per row of
/// [`for_each_link_op`]). Both the client and the server deserialize the same
/// type, so the wire shape has a single definition.
macro_rules! define_link_request_structs {
    ($($method:ident => $path:literal => $req:ident { $($field:ident : $fty:ty),* $(,)? } => $ret:ty;)*) => {
        $(
            #[derive(Debug, Serialize, Deserialize)]
            #[serde(rename_all = "camelCase")]
            pub struct $req { $(pub $field: $fty),* }
        )*
    };
}
for_each_link_op!(define_link_request_structs);
for_each_link_op!(authority_server_link_delegations);

#[cfg(test)]
mod tests {
    use super::*;

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
                },
                BaseAssertion {
                    account_id: "acct".into(),
                    message_id: "m2".into(),
                    update: BaseUpdate::Removed,
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

    // A trivial in-memory transport proves the trait is object-safe and usable —
    // the shape `InProcessTransport` (W1) and `RemoteTransport` (W3) implement.
    struct StubTransport;

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

        async fn subscribe(&self, _coverage: LinkCoverage) -> Result<DownStream, RuntimeError> {
            Ok(Box::pin(futures_util::stream::iter([AuthorityServerFrame::Heartbeat])))
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
            .subscribe(LinkCoverage::Complete)
            .await
            .expect("subscribe");
        assert_eq!(down.next().await, Some(AuthorityServerFrame::Heartbeat));
    }
}
