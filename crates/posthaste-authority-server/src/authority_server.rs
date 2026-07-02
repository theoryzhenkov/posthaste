//! The authority server far node: the single owner of message-command authority server access.
//!
//! This is the **far node** of the runtime↔authority-server coherent link
//! ([replication authority-server-link L1 §2-§3](../replication/authority-server-link/L1.md)). It owns the `MailService` +
//! store and is the one place message-state commands cross from the runtime into
//! the authority server: each applies the command to the service, publishes the resulting
//! authoritative domain events, and nudges the provider outbox to flush.
//!
//! Today it is reached **in-process** (co-located), through
//! [`LocalAuthorityServer`](crate::local_authority_server::LocalAuthorityServer): the runtime
//! calls it directly, zero serialization, identical to the pre-link behavior
//! (assertion `colocated-unchanged`). Extracting it as a named type is the W1
//! seam — the runtime no longer reaches the authority server by scattered direct
//! `service`/`store` calls on the mutation path; it goes through this far node.
//!
//! Reads stay on the runtime's direct store access for now; W2 moves the
//! runtime's served views onto a near-node base cache fed by this node's
//! down-channel, at which point reads stop crossing the link too.
//!
//! @spec docs/replication/authority-server-link/L1#3-the-backendapi-contract

use std::collections::BTreeMap;
use std::sync::Arc;

use posthaste_domain_service::{
    now_iso8601, AccountId, AccountOverview, AddToMailboxCommand, AppSettings, CachedSenderAddress,
    CommandAck, ConversationId, ConversationView, DomainEvent, DraftContent, EventFilter, Identity,
    MailService, MailStore, MailboxId, MailboxSummary, MessageDetail, MessageId, MessageSummary,
    Operation, OperationId, RemoveFromMailboxCommand, ReplaceMailboxesCommand, ReplyContext,
    RevLogSnapshot, SendMessageRequest, ServiceErrorKind, SetKeywordsCommand, SharedGateway,
    SmartMailbox, SmartMailboxId, SmartMailboxSummary, StoreError, SyncMode, SyncTrigger,
    TagSummary, EVENT_TOPIC_REV_LOG_APPENDED,
};
use posthaste_link_core::{MessageChangeDiff, MessageFoldState};
use posthaste_observability::{events, ph_warn};
use posthaste_contract_core::{
    AccountScopeRequest, AccountVerificationResult, AutomationRulePreviewMutation,
    AutomationRulePreviewResult, CreateAccountMutation, CreateSmartMailboxMutation, MailQueryPage,
    MailQueryRequest, MessageResourceKind, MutationReceipt, MutationRequest,
    MutationSettlementState, PatchAccountMutation, PatchAppSettingsMutation,
    PatchSmartMailboxMutation, RevCursorArgs, RevStepInput, RuntimeAccountList,
    RuntimeError, RuntimeErrorCode, RuntimeResourceBytes,
};
use tokio::sync::{broadcast, mpsc};

use crate::account_reads::AccountReadService;
use crate::live_accounts::LiveAccountRuntimeProvider;
use crate::mail_queries::MailQueryService;
use crate::mutations::AccountMutationService;
use crate::runtime_registry::{ForwardAcceptance, RuntimeRegistry};
use posthaste_authority_server_link::{
    AuthorityServerFrame, AuthorityServerLinkId, WireSettlementOutcome,
};
use posthaste_contract_core::mutation_args::keyword_toggle;
use posthaste_contract_core::MailOperation;
use posthaste_link_core::MutationId;

/// The authority server far node ([replication authority-server-link L1 §3](../replication/authority-server-link/L1.md)): owns the
/// service + store + the live-account supervisor + the event publisher, and
/// applies message-state commands to them.
pub(crate) struct AuthorityServer {
    service: Arc<MailService>,
    store: Arc<dyn MailStore>,
    mail_queries: Arc<MailQueryService>,
    account_reads: Arc<AccountReadService>,
    account_mutations: Option<Arc<AccountMutationService>>,
    live_accounts: Arc<dyn LiveAccountRuntimeProvider>,
    event_sender: broadcast::Sender<DomainEvent>,
    runtimes: RuntimeRegistry,
}

impl AuthorityServer {
    pub(crate) fn new(
        service: Arc<MailService>,
        store: Arc<dyn MailStore>,
        mail_queries: Arc<MailQueryService>,
        account_reads: Arc<AccountReadService>,
        account_mutations: Option<Arc<AccountMutationService>>,
        live_accounts: Arc<dyn LiveAccountRuntimeProvider>,
        event_sender: broadcast::Sender<DomainEvent>,
    ) -> Self {
        Self {
            service,
            store,
            mail_queries,
            account_reads,
            account_mutations,
            live_accounts,
            event_sender,
            runtimes: RuntimeRegistry::new(),
        }
    }

    /// The account/config mutation service, or the not-ready error when this
    /// authority server was built without one (some migration/test compositions).
    fn account_mutations(&self) -> Result<&AccountMutationService, RuntimeError> {
        self.account_mutations.as_deref().ok_or_else(|| {
            RuntimeError::runtime_not_ready("account mutation runtime is not available")
        })
    }

    /// Resolve a best-effort gateway for the account, swallowing the error: the
    /// draft/resource reads serve cached data offline when no live gateway is
    /// available.
    async fn optional_gateway(&self, account_id: &AccountId) -> Option<SharedGateway> {
        self.live_accounts.gateway(account_id).await.ok()
    }
}

mod commands;
mod pubsub;
mod reads;


/// Map a store-layer failure to an internal runtime error — the shape the
/// runtime handle used before these reads moved to the far node.
fn store_error_to_runtime_error(error: StoreError) -> RuntimeError {
    RuntimeError::new(RuntimeErrorCode::Internal, error.to_string())
}
