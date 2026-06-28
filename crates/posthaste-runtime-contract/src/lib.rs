//! Transport-neutral runtime contract shared by Posthaste runtime implementations.
//!
//! The types in this crate intentionally avoid Axum, Tauri, frontend, provider-client,
//! SQLite-table, or replica-table dependencies.
//!
//! spec: docs/eph/PLAN-L2-bundled-app-test-plan#runtime-contract-crate-first
//! spec: docs/eph/PLAN-L2-bundled-app-test-plan#contract-no-transport-types

mod mail_query;
pub mod mutation_args;

pub use mail_query::*;

use async_trait::async_trait;
use futures_util::stream::BoxStream;
use posthaste_domain::{
    AccountAppearance, AccountDriver, AccountId, AccountOverview, AddToMailboxCommand, AppSettings,
    AutomationRule, CachePolicy, CachedSenderAddress, CommandAck, CommandResult, DomainEvent,
    DraftContent, EventFilter, Identity, ImapTransportSettings, MailboxId, MailboxSummary,
    MessageAttachment, MessageId, MessageSummary, Operation, OperationId, ProviderAuthKind,
    ProviderHint, RemoveFromMailboxCommand, ReplaceMailboxesCommand, ReplyContext,
    SendMessageRequest, ServiceError, ServiceErrorKind, SetKeywordsCommand, SmartMailbox,
    SmartMailboxId, SmartMailboxRule, SmartMailboxSummary, SmtpTransportSettings, SyncMode,
    TagSummary, ValidationError,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt;

macro_rules! define_id {
    ($name:ident, u64, $getter:ident) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
        #[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            pub fn new(value: u64) -> Self {
                Self(value)
            }

            pub fn $getter(self) -> u64 {
                self.0
            }
        }
    };
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
            #[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
            #[serde(transparent)]
            pub struct $name(String);

            impl $name {
                pub fn new(value: impl Into<String>) -> Self {
                    Self(value.into())
                }

                pub fn as_str(&self) -> &str {
                    &self.0
                }
            }
        )+
    };
}

define_id!(
    RuntimeSessionId,
    ViewId,
    SubscriptionId,
    ClientMutationId,
    RuntimeMutationId,
);
define_id!(ViewRevision, u64, get);
define_id!(RuntimeSessionSeq, u64, get);

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSession {
    pub session_id: RuntimeSessionId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAccountList {
    pub ids: Vec<AccountId>,
    pub enabled_ids: Vec<AccountId>,
    pub items: Vec<AccountOverview>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum AccountScopeRequest {
    EnabledAccounts,
    Explicit { account_ids: Vec<AccountId> },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountTransportMutation {
    pub provider: Option<ProviderHint>,
    pub auth: Option<ProviderAuthKind>,
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub imap: Option<ImapTransportSettings>,
    pub smtp: Option<SmtpTransportSettings>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SecretWriteMode {
    #[default]
    Keep,
    Replace,
    Clear,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretWriteMutation {
    #[serde(default)]
    pub mode: SecretWriteMode,
    pub password: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountMutation {
    pub id: Option<String>,
    pub name: String,
    pub full_name: Option<String>,
    #[serde(default)]
    pub email_patterns: Vec<String>,
    pub driver: Option<AccountDriver>,
    pub enabled: Option<bool>,
    pub appearance: Option<AccountAppearance>,
    #[serde(default)]
    pub transport: AccountTransportMutation,
    #[serde(default)]
    pub secret: SecretWriteMutation,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchAccountMutation {
    pub name: Option<String>,
    pub full_name: Option<String>,
    pub email_patterns: Option<Vec<String>>,
    pub driver: Option<AccountDriver>,
    pub enabled: Option<bool>,
    pub appearance: Option<AccountAppearance>,
    pub transport: Option<AccountTransportMutation>,
    pub secret: Option<SecretWriteMutation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchAppSettingsMutation {
    #[serde(default)]
    pub default_account_id: Option<Option<String>>,
    pub cache_policy: Option<CachePolicy>,
    pub automation_rules: Option<Vec<AutomationRule>>,
    pub automation_drafts: Option<Vec<AutomationRule>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRulePreviewMutation {
    pub condition: SmartMailboxRule,
    pub limit: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRulePreviewResult {
    pub total: i64,
    pub items: Vec<MessageSummary>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSmartMailboxMutation {
    pub name: String,
    pub position: Option<i64>,
    pub rule: SmartMailboxRule,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchSmartMailboxMutation {
    pub name: Option<String>,
    pub position: Option<i64>,
    pub rule: Option<SmartMailboxRule>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountVerificationResult {
    pub ok: bool,
    pub identity_email: Option<String>,
    pub push_supported: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub lifecycle: RuntimeLifecycle,
    pub store: RuntimeStoreStatus,
    pub account_count: usize,
}

/// Which lazy byte-resource of a message to resolve. The single way to name a
/// message's deferred bytes — attachment blob or body — so they share one
/// fetch/cache/serve path (the lazy-resource unification) instead of diverging.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageResourceKind {
    Attachment(String),
    BodyHtml,
    BodyText,
}

/// Raw bytes of a resolved message resource plus how to serve them. The server
/// applies any per-kind transform (e.g. HTML sanitization) before responding.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeResourceBytes {
    pub bytes: Vec<u8>,
    pub content_type: String,
    /// Suggested download filename (attachments); `None` for body resources.
    pub filename: Option<String>,
    /// Inline attachments the server transform needs to rewrite `cid:` URLs in
    /// body HTML. Empty for non-body resources. Carried here so the body-html
    /// transform stays server-side without a second detail load.
    #[serde(default)]
    pub inline_attachments: Vec<MessageAttachment>,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(rename_all = "camelCase"))]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum RuntimeFrame {
    ViewSnapshot {
        #[serde(rename = "sessionSeq")]
        session_seq: RuntimeSessionSeq,
        #[serde(rename = "viewId")]
        view_id: ViewId,
        revision: ViewRevision,
        snapshot: ViewSnapshot,
    },
    ViewReplace {
        #[serde(rename = "sessionSeq")]
        session_seq: RuntimeSessionSeq,
        #[serde(rename = "viewId")]
        view_id: ViewId,
        revision: ViewRevision,
        snapshot: ViewSnapshot,
    },
    /// An incremental mail-list update: only the rows that changed since the
    /// last snapshot, for a session that opted into deltas
    /// ([replication client-link L1](../../replication/client-link/L1.md)). Replaces a whole `ViewReplace`
    /// for row-local changes (flags, reads, removals).
    ViewDelta {
        #[serde(rename = "sessionSeq")]
        session_seq: RuntimeSessionSeq,
        #[serde(rename = "viewId")]
        view_id: ViewId,
        revision: ViewRevision,
        delta: MailListDelta,
    },
    ViewError {
        #[serde(rename = "sessionSeq")]
        session_seq: RuntimeSessionSeq,
        #[serde(rename = "viewId")]
        view_id: ViewId,
        error: RuntimeAdapterError,
    },
    ViewClosed {
        #[serde(rename = "sessionSeq")]
        session_seq: RuntimeSessionSeq,
        #[serde(rename = "viewId")]
        view_id: ViewId,
    },
    /// A verdict about a named client mutation, keyed to its **client** mutation
    /// id so the client correlates it to the optimistic op in its outbox. The
    /// authoritative state change (if any) arrives *separately* as a
    /// `message.updated` domain fact via [`Self::Notification`] — this frame is
    /// the verdict, not the data, and the two are never merged (facts come from
    /// the firehose uncorrelated; verdicts come from the command path keyed by
    /// id). A `Confirmed` retires the op by absorption when the base carries its
    /// effect (no revert); a `Rejected` reverts the optimism and surfaces the
    /// error. Replaces the former `MutationSettlement`
    /// ([mutation.notification design](../../eph/DESIGN-L2-mutation-notification.md)).
    MutationNotification {
        #[serde(rename = "sessionSeq")]
        session_seq: RuntimeSessionSeq,
        #[serde(rename = "clientMutationId")]
        client_mutation_id: ClientMutationId,
        notification: MutationNotification,
    },
    Notification {
        #[serde(rename = "sessionSeq")]
        session_seq: RuntimeSessionSeq,
        kind: String,
        #[cfg_attr(feature = "openapi", schema(value_type = Object))]
        payload: Value,
    },
    Heartbeat {
        #[serde(rename = "sessionSeq")]
        session_seq: RuntimeSessionSeq,
    },
}

impl RuntimeFrame {
    pub fn session_seq(&self) -> RuntimeSessionSeq {
        match self {
            Self::ViewSnapshot { session_seq, .. }
            | Self::ViewReplace { session_seq, .. }
            | Self::ViewDelta { session_seq, .. }
            | Self::ViewError { session_seq, .. }
            | Self::ViewClosed { session_seq, .. }
            | Self::MutationNotification { session_seq, .. }
            | Self::Notification { session_seq, .. }
            | Self::Heartbeat { session_seq } => *session_seq,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum ViewFrame {
    Snapshot {
        snapshot: ViewSnapshot,
    },
    Replace {
        snapshot: ViewSnapshot,
    },
    Error {
        view_id: ViewId,
        revision: ViewRevision,
        error: RuntimeAdapterError,
    },
    Closed {
        view_id: ViewId,
    },
}

impl ViewFrame {
    pub fn revision(&self) -> Option<ViewRevision> {
        match self {
            Self::Snapshot { snapshot } | Self::Replace { snapshot } => Some(snapshot.revision),
            Self::Error { revision, .. } => Some(*revision),
            Self::Closed { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum RuntimeLifecycle {
    Starting,
    Ready,
    Degraded,
    Stopping,
    Stopped,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStoreStatus {
    pub config_loaded: bool,
    pub state_store_open: bool,
    pub cache_root_ready: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ViewDescriptor {
    pub family: String,
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub payload: Value,
    /// Whether the client entity store self-maintains this view's membership from
    /// the `message.updated` firehose (evaluable predicates only). When true
    /// the runtime skips the per-event re-serve for this view (option iii,
    /// single-source-view-membership); when false the runtime re-serves on every
    /// affecting event — required for `Deferred` mail-lists (smart-mailbox /
    /// global / non-`date`) the store cannot self-maintain. Single source: the
    /// client computes it from its predicate and stamps it here; the runtime
    /// never re-derives it (no TS↔Rust drift).
    #[serde(default, skip_serializing_if = "is_false")]
    pub client_self_maintained: bool,
}

/// `skip_serializing_if` helper: omit `client_self_maintained` from the wire when
/// false (the default) — only `clientSelfMaintained: true` is sent, so non-
/// self-maintained + non-mail-list descriptors stay unchanged on the wire.
fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum ViewLifecycle {
    Loading,
    Ready,
    Updating,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReadWatermark {
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCoverage {
    /// The sort-key ranges a consumer holds every matching row within, with no
    /// gaps. A range is inclusive in the composite sort-key domain (`(sortField,
    /// dir, id)`): `from = None` is unbounded above (TOP, the greatest sort key);
    /// `to = None` is unbounded below (BOTTOM). A single range `[TOP, BOTTOM]`
    /// denotes a complete result; `[TOP, k]` a window from the top down to `k`
    /// with potentially more rows below. Empty for a view with no held rows and
    /// no claim of completeness.
    ///
    /// Replaces the coarse `RuntimeCoverageKind { Complete, Partial, Unknown }`,
    /// which was hardcoded `Complete` for windowed mail lists and so could not
    /// distinguish "absent because unchanged" from "absent because not held"
    /// ([replication client-link L2 coverage redesign](../../docs/eph/DESIGN-L2-client-link-reactive-store.md)).
    #[serde(default)]
    pub ranges: Vec<CoverageRange>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CoverageRange {
    /// Inclusive lower bound; `None` = TOP (unbounded above).
    #[serde(default)]
    pub from: Option<Value>,
    /// Inclusive upper bound; `None` = BOTTOM (unbounded below).
    #[serde(default)]
    pub to: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MailListViewState {
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub scope: Value,
    pub projection_kind: MailListProjectionKind,
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub sort: Value,
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub window_request: Value,
    pub rows: Vec<MailListRowState>,
    pub continuation: MailListContinuation,
    pub read_watermark: Option<ReadWatermark>,
    pub coverage: RuntimeCoverage,
    pub known_total_count: Option<u64>,
    pub anchor: MailListAnchorState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum MailListProjectionKind {
    Message,
    Conversation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MailListRowState {
    pub row_key: String,
    pub resource_ref: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub projection: Value,
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub sort_key: Value,
    pub order_key: String,
}

/// An incremental mail-list view update ([replication client-link L1](../../replication/client-link/L1.md)):
/// the rows that changed since the last snapshot, instead of the whole view. The
/// client reconciles it against its held rows — drop rows absent from `order`,
/// reorder to `order`, then apply `upserts` by `row_key`. Emitted only to a
/// session that declared [`view_delta`](RuntimeCallerCapabilities::view_delta),
/// and only when the change is row-local (structural changes still send a whole
/// `ViewReplace`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MailListDelta {
    /// The new full row order (row keys), when it changed (add/remove/reorder);
    /// `None` when unchanged. Rows whose key is absent from a present `order` are
    /// removed.
    pub order: Option<Vec<String>>,
    /// Rows that are new or whose content changed, keyed by `row_key`.
    pub upserts: Vec<MailListRowState>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MailListContinuation {
    pub before_cursor: Option<String>,
    pub after_cursor: Option<String>,
    pub has_before: bool,
    pub has_after: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum MailListAnchorState {
    NotRequested,
    Kept {
        row_key: String,
    },
    Moved {
        previous_row_key: String,
        row_key: String,
    },
    Removed {
        row_key: String,
    },
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ViewSnapshot {
    pub view_id: ViewId,
    pub descriptor: ViewDescriptor,
    pub revision: ViewRevision,
    pub lifecycle: ViewLifecycle,
    pub read_watermark: Option<ReadWatermark>,
    pub coverage: RuntimeCoverage,
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub data: Value,
    pub error: Option<RuntimeAdapterError>,
}

/// Phase 2 undo/redo: the client-supplied reversible-op step payload (carried
/// in [`MutationRequest::context`] as `{"revStep": {...}}` on a forward
/// action). The server appends it to `rev_log` on confirmation. `diff` is the
/// `MessageChangeDiff` JSON captured client-side.
///
/// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevStepInput {
    /// Client-generated ULID; the cursor key + idempotency key.
    pub step_id: String,
    /// `MessageChangeDiff` JSON (`{keywords, mailboxes}{added, removed}`).
    pub diff: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MutationRequest {
    pub session_id: Option<RuntimeSessionId>,
    pub name: String,
    #[serde(default)]
    pub args: Value,
    pub client_mutation_id: ClientMutationId,
    pub context: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MutationReceipt {
    pub runtime_mutation_id: Option<RuntimeMutationId>,
    pub client_mutation_id: ClientMutationId,
    pub name: String,
    pub state: MutationSettlementState,
    pub error: Option<RuntimeAdapterError>,
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub output: Value,
}

/// A terminal verdict about a named client mutation, carried by
/// [`RuntimeFrame::MutationNotification`] and keyed to the client mutation id.
/// The two outcomes are deliberately the only ones on the wire: `Confirmed` is
/// otherwise implicit in the base update (it serves the no-op-confirmation and
/// durable-outbox-clear cases), and `Rejected` is the *only* signal a failed
/// mutation produces — a rejection changes no state, so no `message.updated`
/// accompanies it. The non-terminal acks (Accepted/Queued) are not emitted: the
/// client already tracks the op the moment it enqueues it.
///
/// @spec docs/eph/DESIGN-L2-mutation-notification
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(rename_all = "camelCase"))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum MutationNotification {
    /// The mutation succeeded. The client retires the optimistic op once the
    /// authoritative base absorbs its effect (or immediately, for a no-op).
    Confirmed,
    /// The mutation was rejected (validation, conflict, transport). The client
    /// drops the op, reverts the optimistic projection, and surfaces the error.
    Rejected { error: RuntimeAdapterError },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum MutationSettlementState {
    Accepted,
    Confirmed,
    Failed,
}

impl MutationSettlementState {
    /// A terminal state will not transition again and can be safely evicted from
    /// the session's live mutation cache once the client has had a chance to
    /// observe the settlement.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            MutationSettlementState::Confirmed | MutationSettlementState::Failed
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAdapterError {
    pub code: RuntimeErrorCode,
    pub message: String,
    pub retryable: bool,
    pub correlation_id: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub details: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum RuntimeErrorCode {
    RuntimeNotReady,
    InvalidDescriptor,
    InvalidMutation,
    InvalidSecret,
    InvalidAccount,
    AccountBaseUrlRequired,
    AccountSecretRequired,
    AccountUsernameRequired,
    AccountSenderRequired,
    Unauthorized,
    NotFound,
    ProviderUnavailable,
    Conflict,
    NetworkError,
    StateMismatch,
    CannotCalculateChanges,
    GatewayRejected,
    SecretUnavailable,
    SecretUnsupported,
    StorageFailure,
    StorageCorrupted,
    ConfigValidation,
    ConfigIo,
    ConfigParse,
    TransportDisconnected,
    Internal,
}

#[derive(Debug)]
pub struct RuntimeError(pub RuntimeAdapterError);

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}: {}", self.0.code, self.0.message)
    }
}

impl std::error::Error for RuntimeError {}

impl RuntimeError {
    pub fn new(code: RuntimeErrorCode, message: impl Into<String>) -> Self {
        Self::with_details(code, message, Value::Null)
    }

    pub fn with_details(
        code: RuntimeErrorCode,
        message: impl Into<String>,
        details: Value,
    ) -> Self {
        Self(RuntimeAdapterError {
            code,
            message: message.into(),
            retryable: false,
            correlation_id: None,
            details,
        })
    }

    pub fn retryable(code: RuntimeErrorCode, message: impl Into<String>) -> Self {
        Self(RuntimeAdapterError {
            code,
            message: message.into(),
            retryable: true,
            correlation_id: None,
            details: Value::Null,
        })
    }

    pub fn internal(message: impl Into<String>, correlation_id: Option<String>) -> Self {
        Self(RuntimeAdapterError {
            code: RuntimeErrorCode::Internal,
            message: message.into(),
            retryable: false,
            correlation_id,
            details: Value::Null,
        })
    }

    pub fn runtime_not_ready(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::RuntimeNotReady, message)
    }

    pub fn invalid_descriptor(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::InvalidDescriptor, message)
    }

    pub fn invalid_mutation(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::InvalidMutation, message)
    }

    pub fn invalid_secret(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::InvalidSecret, message)
    }

    pub fn invalid_account(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::InvalidAccount, message)
    }

    pub fn account_base_url_required(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::AccountBaseUrlRequired, message)
    }

    pub fn account_secret_required(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::AccountSecretRequired, message)
    }

    pub fn account_username_required(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::AccountUsernameRequired, message)
    }

    pub fn account_sender_required(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::AccountSenderRequired, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::Unauthorized, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::NotFound, message)
    }

    pub fn provider_unavailable(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::ProviderUnavailable, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(RuntimeErrorCode::Conflict, message)
    }

    pub fn compensation_failed(
        operation: impl Into<String>,
        original: RuntimeError,
        rollback: RuntimeError,
    ) -> Self {
        let RuntimeAdapterError {
            code,
            message,
            retryable,
            correlation_id,
            details,
        } = original.0;
        let original_envelope = RuntimeAdapterError {
            code: code.clone(),
            message: message.clone(),
            retryable,
            correlation_id: correlation_id.clone(),
            details,
        };
        Self(RuntimeAdapterError {
            code,
            message,
            retryable,
            correlation_id,
            details: json!({
                "compensation": {
                    "operation": operation.into(),
                    "original": original_envelope,
                    "rollback": rollback.0,
                }
            }),
        })
    }

    pub fn envelope(&self) -> &RuntimeAdapterError {
        &self.0
    }
}

impl From<ValidationError> for RuntimeError {
    fn from(error: ValidationError) -> Self {
        match error {
            ValidationError::InvalidAccount(message) => Self::invalid_account(message),
            ValidationError::BaseUrlRequired(message) => Self::account_base_url_required(message),
            ValidationError::SecretRequired(message) => Self::account_secret_required(message),
            ValidationError::UsernameRequired(message) => Self::account_username_required(message),
            ValidationError::SenderRequired(message) => Self::account_sender_required(message),
            ValidationError::DuplicateSourceId(id) => {
                Self::invalid_account(format!("duplicate source id '{id}'"))
            }
            ValidationError::DuplicateSmartMailboxId(id) => {
                Self::invalid_account(format!("duplicate smart mailbox id '{id}'"))
            }
            ValidationError::DanglingDefaultAccount(_) => {
                Self::invalid_account("default account must reference an existing account")
            }
        }
    }
}

impl From<ServiceError> for RuntimeError {
    fn from(error: ServiceError) -> Self {
        let code = match error.kind() {
            ServiceErrorKind::NotFound => RuntimeErrorCode::NotFound,
            ServiceErrorKind::Conflict => RuntimeErrorCode::Conflict,
            ServiceErrorKind::StateMismatch => RuntimeErrorCode::StateMismatch,
            ServiceErrorKind::AuthError => RuntimeErrorCode::Unauthorized,
            ServiceErrorKind::GatewayUnavailable => RuntimeErrorCode::ProviderUnavailable,
            ServiceErrorKind::NetworkError => RuntimeErrorCode::NetworkError,
            ServiceErrorKind::CannotCalculateChanges => RuntimeErrorCode::CannotCalculateChanges,
            ServiceErrorKind::GatewayRejected => RuntimeErrorCode::GatewayRejected,
            ServiceErrorKind::SecretUnavailable => RuntimeErrorCode::SecretUnavailable,
            ServiceErrorKind::SecretUnsupported => RuntimeErrorCode::SecretUnsupported,
            ServiceErrorKind::StorageFailure => RuntimeErrorCode::StorageFailure,
            ServiceErrorKind::StorageCorrupted => RuntimeErrorCode::StorageCorrupted,
            ServiceErrorKind::ConfigValidation => RuntimeErrorCode::ConfigValidation,
            ServiceErrorKind::ConfigIo => RuntimeErrorCode::ConfigIo,
            ServiceErrorKind::ConfigParse => RuntimeErrorCode::ConfigParse,
        };
        Self::new(code, error.to_string())
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_domain::StoreError;

    #[test]
    fn service_error_conversion_preserves_runtime_error_code() {
        let error = RuntimeError::from(ServiceError::from(StoreError::NotFound(
            "account missing".to_string(),
        )));

        assert_eq!(error.envelope().code, RuntimeErrorCode::NotFound);
    }

    #[test]
    fn retryable_constructor_marks_retryable_envelope() {
        let error =
            RuntimeError::retryable(RuntimeErrorCode::ProviderUnavailable, "gateway unavailable");

        assert!(error.envelope().retryable);
        assert_eq!(error.envelope().code, RuntimeErrorCode::ProviderUnavailable);
    }

    #[test]
    fn mail_list_view_state_serializes_rows_continuation_and_anchor() {
        let state = MailListViewState {
            scope: serde_json::json!({ "kind": "sourceMailbox", "sourceId": "primary" }),
            projection_kind: MailListProjectionKind::Message,
            sort: serde_json::json!({ "field": "date", "direction": "desc" }),
            window_request: serde_json::json!({ "limit": 50 }),
            rows: vec![MailListRowState {
                row_key: "primary:m1".to_string(),
                resource_ref: Some("message:primary:m1".to_string()),
                projection: serde_json::json!({ "subject": "Subject" }),
                sort_key: serde_json::json!(["2026-04-28T12:00:00Z", "m1"]),
                order_key: "0001".to_string(),
            }],
            continuation: MailListContinuation {
                before_cursor: Some("before-1".to_string()),
                after_cursor: Some("after-1".to_string()),
                has_before: true,
                has_after: true,
            },
            read_watermark: Some(ReadWatermark {
                value: "watermark-1".to_string(),
            }),
            coverage: RuntimeCoverage {
                ranges: vec![CoverageRange {
                    from: None,
                    to: Some(serde_json::json!(["2026-04-28T12:00:00Z", "m1"])),
                }],
            },
            known_total_count: Some(1),
            anchor: MailListAnchorState::Kept {
                row_key: "primary:m1".to_string(),
            },
        };

        let serialized = serde_json::to_value(&state).expect("state should serialize");
        assert_eq!(serialized["rows"][0]["rowKey"], "primary:m1");
        assert_eq!(serialized["continuation"]["beforeCursor"], "before-1");
        assert_eq!(serialized["continuation"]["afterCursor"], "after-1");
        assert_eq!(serialized["readWatermark"]["value"], "watermark-1");
        assert_eq!(
            serialized["coverage"]["ranges"][0]["to"].clone(),
            serde_json::json!(["2026-04-28T12:00:00Z", "m1"])
        );
        assert_eq!(serialized["anchor"]["kind"], "kept");
    }

    #[test]
    fn runtime_frame_serializes_session_fields_as_camel_case() {
        let snapshot = ViewSnapshot {
            view_id: ViewId::new("view-1"),
            descriptor: ViewDescriptor {
                family: "mailList".to_string(),
                payload: serde_json::json!({ "sourceId": "primary" }),
                ..Default::default()
            },
            revision: ViewRevision::new(7),
            lifecycle: ViewLifecycle::Ready,
            read_watermark: None,
            coverage: RuntimeCoverage {
                ranges: vec![CoverageRange {
                    from: None,
                    to: None,
                }],
            },
            data: serde_json::json!({ "rows": [] }),
            error: None,
        };
        let serialized = serde_json::to_value(RuntimeFrame::ViewReplace {
            session_seq: RuntimeSessionSeq::new(2),
            view_id: ViewId::new("view-1"),
            revision: ViewRevision::new(7),
            snapshot,
        })
        .expect("frame should serialize");

        assert_eq!(serialized["type"], "viewReplace");
        assert_eq!(serialized["sessionSeq"], 2);
        assert_eq!(serialized["viewId"], "view-1");
        assert!(serialized.get("session_seq").is_none());
        assert!(serialized.get("view_id").is_none());
    }

    #[test]
    fn mail_list_view_snapshot_carries_complete_runtime_coverage() {
        let state = MailListViewState {
            scope: serde_json::json!({ "kind": "sourceMailbox", "sourceId": "primary" }),
            projection_kind: MailListProjectionKind::Message,
            sort: serde_json::json!({ "field": "date", "direction": "desc" }),
            window_request: serde_json::json!({ "limit": 50 }),
            rows: Vec::new(),
            continuation: MailListContinuation {
                before_cursor: None,
                after_cursor: None,
                has_before: false,
                has_after: false,
            },
            read_watermark: Some(ReadWatermark {
                value: "watermark-1".to_string(),
            }),
            coverage: RuntimeCoverage {
                ranges: vec![CoverageRange {
                    from: None,
                    to: None,
                }],
            },
            known_total_count: Some(0),
            anchor: MailListAnchorState::Removed {
                row_key: "primary:m1".to_string(),
            },
        };
        let snapshot = ViewSnapshot {
            view_id: ViewId::new("view-1"),
            descriptor: ViewDescriptor {
                family: "mailList".to_string(),
                payload: serde_json::json!({ "sourceId": "primary" }),
                ..Default::default()
            },
            revision: ViewRevision::new(1),
            lifecycle: ViewLifecycle::Ready,
            read_watermark: Some(ReadWatermark {
                value: "watermark-1".to_string(),
            }),
            coverage: RuntimeCoverage {
                ranges: vec![CoverageRange {
                    from: None,
                    to: None,
                }],
            },
            data: serde_json::to_value(state).expect("state should serialize"),
            error: None,
        };

        assert_eq!(
            snapshot.coverage.ranges,
            vec![CoverageRange {
                from: None,
                to: None
            }]
        );
        assert_eq!(snapshot.data["continuation"]["hasAfter"], false);
        assert_eq!(snapshot.data["anchor"]["kind"], "removed");
        assert_eq!(snapshot.read_watermark.unwrap().value, "watermark-1");
    }
}
