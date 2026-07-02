//! The typed, wire-free client-facing domain RPC extracted from `RuntimeCore`
//! (RFC-L2-architecture-cleanup D7/D23). The 41 of 52 `RuntimeCore` methods that
//! return serde domain types — never frames or streams — split into four narrow
//! traits along change-cadence, so a subset consumer (e.g. `oauth_routes`, which
//! calls `get_account` only) can depend on just `&dyn RuntimeAccountApi` rather
//! than the whole surface (XVI). Finer than four is ceremony: no consumer
//! distinguishes catalog from page-reads, or compose from message-commands (XXI).
//!
//! An umbrella [`RuntimeApi`] supertrait (blanket-impl'd) recomposes the four
//! facets into one object for callers that need all of them (`AppState`).
//!
//! The 5 per-command message RPCs on [`RuntimeMailWriteApi`] stay as-is here;
//! their D21 collapse into a single `apply_mail_operation(MailOperation)` lands
//! at M5, not M3.
//!
//! Every method takes `caller: RuntimeCaller` first (the shared caller identity,
//! lives in `posthaste-contract-core`). Depends on `contract-core` + `domain-model`
//! only — both wasm-pure — so this crate is serde-only.

use async_trait::async_trait;
use posthaste_contract_core::{
    AccountScopeRequest, AccountVerificationResult, AutomationRulePreviewMutation,
    AutomationRulePreviewResult, CreateAccountMutation, CreateSmartMailboxMutation,
    MailQueryPage, MailQueryRequest, MessageResourceKind, PatchAccountMutation,
    PatchAppSettingsMutation, PatchSmartMailboxMutation, RuntimeAccountList, RuntimeCaller,
    RuntimeError, RuntimeResourceBytes, RuntimeStatus,
};
use posthaste_domain_model::{
    AccountId, AccountOverview, AddToMailboxCommand, AppSettings, CachedSenderAddress, CommandAck,
    CommandResult, DraftContent, Identity, MailboxId, MailboxSummary, MessageId, Operation,
    OperationId, RemoveFromMailboxCommand, ReplaceMailboxesCommand, ReplyContext, SendMessageRequest,
    SetKeywordsCommand, SmartMailbox, SmartMailboxId, SmartMailboxSummary, SyncMode, TagSummary,
};
use std::collections::BTreeMap;

/// Account + admin RPC: the account CRUD/lifecycle surface and the runtime
/// admin RPCs (`runtime_status`, `sync_account`, `reload_config`). Exactly the
/// facet `oauth_routes` + the test/bench harness-setup surface consume (XVI):
/// the one `get_account` caller takes `&dyn RuntimeAccountApi`.
#[async_trait]
pub trait RuntimeAccountApi: Send + Sync {
    async fn list_accounts(&self, caller: RuntimeCaller) -> Result<RuntimeAccountList, RuntimeError>;

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

    /// Reload the runtime's configuration from disk (admin RPC; rides with
    /// `set_account_enabled` in its one consumer, `api/accounts/lifecycle.rs`).
    async fn reload_config(&self, caller: RuntimeCaller) -> Result<(), RuntimeError>;

    async fn runtime_status(&self, caller: RuntimeCaller) -> Result<RuntimeStatus, RuntimeError>;

    async fn sync_account(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        mode: SyncMode,
    ) -> Result<usize, RuntimeError>;
}

/// App-settings + automation-preview RPC (3 methods). The settings facet.
#[async_trait]
pub trait RuntimeSettingsApi: Send + Sync {
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
}

/// Mail-catalog + mail-read RPC (12 methods): mailbox/smart-mailbox/tag catalog
/// reads + writes, mail-list queries, message detail, and lazy resource bytes.
/// One-shot serde reads/commands — no streams (streams live on the client-link
/// trait). `get_message_resource` is a plain read RPC (one-shot serde bytes, not
/// a resource stream — D7 prose said "resource streams", reality is one-shot).
#[async_trait]
pub trait RuntimeMailReadApi: Send + Sync {
    async fn list_mailboxes(
        &self,
        caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<BTreeMap<AccountId, Vec<MailboxSummary>>, RuntimeError>;

    async fn set_mailbox_role(
        &self,
        caller: RuntimeCaller,
        account_id: AccountId,
        mailbox_id: MailboxId,
        role: Option<String>,
    ) -> Result<Vec<MailboxSummary>, RuntimeError>;

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

    async fn query_mail_page(
        &self,
        caller: RuntimeCaller,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError>;

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
}

/// Compose-outbox + message-commands RPC (15 methods): identity/sender/reply/
/// draft reads, send/save/delete draft, the outbox operation lifecycle, and the
/// per-command message writes. The 5 per-command message RPCs
/// (`set_message_keywords` et al.) stay as-is here; their D21 collapse into one
/// `apply_mail_operation(MailOperation) -> CommandAck` lands at M5, not M3.
#[async_trait]
pub trait RuntimeMailWriteApi: Send + Sync {
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
}

/// The umbrella supertrait: every domain-RPC facet the runtime surface exposes.
/// Blanket-impl'd for any type implementing all four facets, so `RuntimeHandle`
/// (and test wrappers) get it for free. Lets a caller hold the whole surface as
/// one object (`Arc<dyn RuntimeApi>`) when it needs all of it (e.g. `AppState`),
/// while subset consumers depend on the narrow facet they use (XVI).
pub trait RuntimeApi: RuntimeAccountApi + RuntimeSettingsApi + RuntimeMailReadApi + RuntimeMailWriteApi {}

impl<T> RuntimeApi for T where
    T: RuntimeAccountApi + RuntimeSettingsApi + RuntimeMailReadApi + RuntimeMailWriteApi {}
