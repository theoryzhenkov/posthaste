//! The runtime contract surface: the `RuntimeCore` trait + the `RuntimeCaller`
//! identity and the subscription/stream types its methods return. The shared
//! *wire* vocabulary (ids, view models, MutationRequest/Receipt, errors,
//! mutation_args, mail_query) moved to `posthaste-contract-core`
//! (RFC-L2-architecture-cleanup D5/D6) and is re-exported below so this crate's
//! remaining consumers compile unchanged.
//!
//! TEMPORARY — this crate is dissolved at M3: `RuntimeCore` splits into
//! `posthaste-runtime-api` + `posthaste-client-link`, and the shim goes away.

// TEMPORARY migration shim — dissolved at RFC-L2-architecture-cleanup M3.
pub use posthaste_contract_core::*;

use async_trait::async_trait;
use futures_util::stream::BoxStream;
use posthaste_domain_model::{
    AccountId, AccountOverview, AddToMailboxCommand, AppSettings, CachedSenderAddress, CommandAck,
    CommandResult, DomainEvent, DraftContent, EventFilter, Identity, MailboxId, MailboxSummary,
    MessageId, Operation, OperationId, RemoveFromMailboxCommand, ReplaceMailboxesCommand,
    ReplyContext, SendMessageRequest, SetKeywordsCommand, SmartMailbox, SmartMailboxId,
    SmartMailboxSummary, SyncMode, TagSummary,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCaller {
    pub session_id: Option<RuntimeSessionId>,
    pub capabilities: RuntimeCallerCapabilities,
    pub account_scope: Option<Vec<String>>,
    pub operation_source: RuntimeOperationSource,
    pub correlation_id: Option<String>,
}

impl RuntimeCaller {
    pub fn system() -> Self {
        Self {
            session_id: None,
            capabilities: RuntimeCallerCapabilities::default(),
            account_scope: None,
            operation_source: RuntimeOperationSource::System,
            correlation_id: None,
        }
    }

    pub fn api() -> Self {
        Self {
            operation_source: RuntimeOperationSource::Api,
            ..Self::system()
        }
    }

    pub fn test() -> Self {
        Self {
            operation_source: RuntimeOperationSource::Test,
            ..Self::system()
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCallerCapabilities {
    #[serde(default)]
    pub actions: Vec<RuntimeCapability>,
    /// The caller's session can apply incremental mail-list view deltas
    /// ([`RuntimeFrame::ViewDelta`]) rather than whole-view replaces. When set,
    /// the runtime sends only the rows that changed instead of re-serializing
    /// the entire view on each recompute ([replication client-link L1](../../replication/client-link/L1.md)).
    /// Default `false`, so a client that does not understand deltas keeps
    /// receiving whole `ViewReplace` frames.
    #[serde(default)]
    pub view_delta: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeCapability {
    Read,
    Manage,
    Send,
    Tag,
    Move,
    Delete,
    Resource,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeOperationSource {
    System,
    Api,
    Desktop,
    Renderer,
    Test,
}

/// Live runtime event stream returned by authority runtimes.
pub type RuntimeEventStream = BoxStream<'static, DomainEvent>;

/// Runtime-owned event subscription: optional replayed backlog followed by live events.
pub struct RuntimeEventSubscription {
    pub replay: Vec<DomainEvent>,
    pub live: RuntimeEventStream,
}

pub type RuntimeViewFrameStream = BoxStream<'static, ViewFrame>;
pub type RuntimeFrameStream = BoxStream<'static, RuntimeFrame>;

pub struct RuntimeViewSubscription {
    pub catch_up: Option<ViewFrame>,
    pub live: RuntimeViewFrameStream,
}

pub struct RuntimeFrameSubscription {
    pub catch_up: Vec<RuntimeFrame>,
    pub live: RuntimeFrameStream,
}

#[async_trait]
pub trait RuntimeCore: Send + Sync {
    async fn runtime_status(&self, caller: RuntimeCaller) -> Result<RuntimeStatus, RuntimeError>;

    async fn get_app_settings(&self, caller: RuntimeCaller) -> Result<AppSettings, RuntimeError>;

    async fn patch_app_settings(
        &self,
        caller: RuntimeCaller,
        mutation: PatchAppSettingsMutation,
    ) -> Result<AppSettings, RuntimeError>;

    async fn preview_automation_rule(
        &self,
        caller: RuntimeCaller,
        mutation: AutomationRulePreviewMutation,
    ) -> Result<AutomationRulePreviewResult, RuntimeError>;

    async fn list_accounts(
        &self,
        caller: RuntimeCaller,
    ) -> Result<RuntimeAccountList, RuntimeError>;

    async fn get_account(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<AccountOverview, RuntimeError>;

    async fn resolve_account_scope(
        &self,
        caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<Vec<AccountId>, RuntimeError>;

    async fn list_mailboxes(
        &self,
        caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<BTreeMap<AccountId, Vec<MailboxSummary>>, RuntimeError>;

    async fn list_smart_mailboxes(
        &self,
        caller: RuntimeCaller,
    ) -> Result<Vec<SmartMailboxSummary>, RuntimeError>;

    async fn get_smart_mailbox(
        &self,
        caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<SmartMailbox, RuntimeError>;

    async fn create_smart_mailbox(
        &self,
        caller: RuntimeCaller,
        mutation: CreateSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError>;

    async fn patch_smart_mailbox(
        &self,
        caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
        mutation: PatchSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError>;

    async fn delete_smart_mailbox(
        &self,
        caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<(), RuntimeError>;

    async fn reset_default_smart_mailboxes(
        &self,
        caller: RuntimeCaller,
    ) -> Result<Vec<SmartMailboxSummary>, RuntimeError>;

    async fn list_tags(
        &self,
        caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<Vec<TagSummary>, RuntimeError>;

    async fn get_identity(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<Identity, RuntimeError>;

    async fn list_sender_addresses(
        &self,
        caller: RuntimeCaller,
    ) -> Result<Vec<CachedSenderAddress>, RuntimeError>;

    async fn get_reply_context(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<ReplyContext, RuntimeError>;

    async fn get_draft_content(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<DraftContent, RuntimeError>;

    async fn query_mail_page(
        &self,
        caller: RuntimeCaller,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError>;

    async fn open_session(&self, caller: RuntimeCaller) -> Result<RuntimeSession, RuntimeError>;

    async fn subscribe_runtime_frames(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        after_seq: Option<RuntimeSessionSeq>,
    ) -> Result<RuntimeFrameSubscription, RuntimeError>;

    async fn close_session(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
    ) -> Result<(), RuntimeError>;

    async fn open_session_view(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        descriptor: ViewDescriptor,
    ) -> Result<ViewSnapshot, RuntimeError>;

    async fn close_session_view(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        view_id: ViewId,
    ) -> Result<(), RuntimeError>;

    /// Grow an open windowed session view by `count` rows, returning the
    /// extended snapshot (also broadcast as a `ViewReplace` frame).
    async fn extend_session_view(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        view_id: ViewId,
        count: usize,
    ) -> Result<ViewSnapshot, RuntimeError>;

    async fn run_mutation(
        &self,
        caller: RuntimeCaller,
        request: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError>;

    async fn open_view(
        &self,
        caller: RuntimeCaller,
        descriptor: ViewDescriptor,
    ) -> Result<ViewSnapshot, RuntimeError>;

    async fn subscribe_view(
        &self,
        caller: RuntimeCaller,
        view_id: ViewId,
        after_revision: Option<ViewRevision>,
    ) -> Result<RuntimeViewSubscription, RuntimeError>;

    async fn send_message(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        request: SendMessageRequest,
    ) -> Result<(), RuntimeError>;

    /// Save a draft local-first, returning the enqueued operation. `draft_id` is
    /// `None` for a new draft or the existing draft's id for an edit.
    ///
    /// @spec docs/L1-outbox#operation-model
    async fn save_draft(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        draft_id: Option<MessageId>,
        request: SendMessageRequest,
    ) -> Result<Operation, RuntimeError>;

    /// Delete a draft local-first, returning the enqueued operation.
    ///
    /// @spec docs/L1-outbox#operation-model
    async fn delete_draft(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        draft_id: MessageId,
    ) -> Result<Operation, RuntimeError>;

    /// List an account's non-terminal outbox operations (pending/failed work),
    /// oldest first, for optimistic hydration and pending/failed UI.
    ///
    /// @spec docs/L1-outbox#operation-model
    async fn list_pending_operations(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<Vec<Operation>, RuntimeError>;

    /// Remove a queued or failed outbox operation (a user escape hatch for a
    /// dead op). In-flight operations cannot be discarded.
    async fn discard_operation(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        operation_id: OperationId,
    ) -> Result<(), RuntimeError>;

    /// Re-arm a failed outbox operation so the next flush re-attempts it.
    async fn retry_operation(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        operation_id: OperationId,
    ) -> Result<(), RuntimeError>;

    async fn set_message_keywords(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: SetKeywordsCommand,
    ) -> Result<CommandAck, RuntimeError>;

    async fn add_message_to_mailbox(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: AddToMailboxCommand,
    ) -> Result<CommandAck, RuntimeError>;

    async fn remove_message_from_mailbox(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: RemoveFromMailboxCommand,
    ) -> Result<CommandAck, RuntimeError>;

    async fn replace_message_mailboxes(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: ReplaceMailboxesCommand,
    ) -> Result<CommandAck, RuntimeError>;

    async fn destroy_message(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<CommandAck, RuntimeError>;

    async fn set_mailbox_role(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        mailbox_id: MailboxId,
        role: Option<String>,
    ) -> Result<Vec<MailboxSummary>, RuntimeError>;

    async fn get_message_detail(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<CommandResult, RuntimeError>;

    /// Resolve a message's lazy bytes (attachment blob or body) as raw bytes +
    /// content type. The single entry point for every deferred message resource.
    async fn get_message_resource(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        kind: MessageResourceKind,
    ) -> Result<RuntimeResourceBytes, RuntimeError>;

    async fn sync_account(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        mode: SyncMode,
    ) -> Result<usize, RuntimeError>;

    async fn replay_events(
        &self,
        caller: RuntimeCaller,
        filter: EventFilter,
    ) -> Result<Vec<DomainEvent>, RuntimeError>;

    async fn subscribe_events(
        &self,
        caller: RuntimeCaller,
        filter: EventFilter,
    ) -> Result<RuntimeEventSubscription, RuntimeError>;

    async fn create_account(
        &self,
        caller: RuntimeCaller,
        mutation: CreateAccountMutation,
    ) -> Result<AccountOverview, RuntimeError>;

    async fn patch_account(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        mutation: PatchAccountMutation,
    ) -> Result<AccountOverview, RuntimeError>;

    async fn delete_account(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<(), RuntimeError>;

    async fn verify_account(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<AccountVerificationResult, RuntimeError>;

    async fn set_account_enabled(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        enabled: bool,
    ) -> Result<(), RuntimeError>;

    async fn reload_config(&self, caller: RuntimeCaller) -> Result<(), RuntimeError>;
}
