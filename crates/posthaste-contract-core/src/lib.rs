//! The shared wire vocabulary above domain-model (RFC-L2-architecture-cleanup
//! D5/D6): the typed command set, view models, opaque ids, MutationRequest/
//! Receipt, MutationSettlementState, and RuntimeAdapterError. Everything the
//! runtime-contract crate fuses with the `RuntimeCore` trait, minus the trait
//! itself — so both link surfaces depend on it without dragging `RuntimeCore`.
//!
//! Wasm-pure: serde only (no tokio/reqwest/mail-parser/axum). The `openapi`
//! feature derives `utoipa::ToSchema` on the wire types; consumers forward it.

mod mail_operation;
mod mail_query;
pub mod mutation_args;

pub use mail_operation::MailOperation;
pub use mail_query::*;

use posthaste_domain_model::{
    AccountAppearance, AccountDriver, AccountId, AccountOverview, Appearance, AutomationRule,
    CachePolicy, ImapTransportSettings, MailboxColor, MessageAttachment, MessageSummary,
    Notifications, ProviderAuthKind, ProviderHint, ServiceError, ServiceErrorKind,
    SmartMailboxId, SmartMailboxRule, SmtpTransportSettings, TagAppearance, ValidationError,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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
    RuntimeLinkId,
    ViewId,
    SubscriptionId,
    ClientMutationId,
    RuntimeMutationId,
);
define_id!(ViewRevision, u64, get);
define_id!(RuntimeLinkSeq, u64, get);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLinkConnection {
    pub link_id: RuntimeLinkId,
}

/// The identity + capabilities of whoever is calling the runtime surface. Shared
/// by both trait crates extracted from `RuntimeCore` (`posthaste-runtime-api` and
/// `posthaste-client-link`) so they share one caller vocabulary without an edge
/// between them (RFC-L2-architecture-cleanup D7/D23 — RuntimeCaller lives in the
/// shared vocabulary crate).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCaller {
    pub link_id: Option<RuntimeLinkId>,
    pub capabilities: RuntimeCallerCapabilities,
    pub account_scope: Option<Vec<String>>,
    pub operation_source: RuntimeOperationSource,
    pub correlation_id: Option<String>,
}

impl RuntimeCaller {
    pub fn system() -> Self {
        Self {
            link_id: None,
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
    /// The caller's link can apply incremental mail-list view deltas
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
    pub signature: Option<String>,
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
    pub signature: Option<String>,
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
    pub appearance: Option<Appearance>,
    pub notifications: Option<Notifications>,
    pub mailbox_colors: Option<Vec<MailboxColor>>,
    /// Per-tag presentation overrides (color + icon). Overwrites the stored list
    /// wholesale.
    pub tags: Option<Vec<TagAppearance>>,
    /// Explicit sidebar arrangement (ids). Overwrites the stored list wholesale
    /// — the drag-to-reorder primitive (see [`AppSettings::smart_mailbox_order`]).
    pub smart_mailbox_order: Option<Vec<SmartMailboxId>>,
    pub account_order: Option<Vec<AccountId>>,
    /// Force the current backfill rules to re-run after persisting, even when
    /// the rule fingerprint is unchanged (on-demand "backfill now").
    #[serde(default)]
    pub force_backfill: bool,
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
    /// Optional view role (e.g. `"archive"`) tagging this smart mailbox with a
    /// built-in role's icon/accent and contextual actions. `None` for a plain
    /// saved query. Validated against the known mailbox roles.
    #[serde(default)]
    pub role: Option<String>,
    pub rule: SmartMailboxRule,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchSmartMailboxMutation {
    pub name: Option<String>,
    /// Sparse: absent leaves the role unchanged; a known role string sets it; an
    /// empty string clears it. (An empty-string sentinel, rather than a JSON
    /// `null`, because plain `Option<Option<_>>` can't distinguish a present
    /// `null` from an absent field over the wire.)
    #[serde(default)]
    pub role: Option<String>,
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
        #[serde(rename = "linkSeq")]
        link_seq: RuntimeLinkSeq,
        #[serde(rename = "viewId")]
        view_id: ViewId,
        revision: ViewRevision,
        snapshot: ViewSnapshot,
    },
    ViewReplace {
        #[serde(rename = "linkSeq")]
        link_seq: RuntimeLinkSeq,
        #[serde(rename = "viewId")]
        view_id: ViewId,
        revision: ViewRevision,
        snapshot: ViewSnapshot,
    },
    /// An incremental mail-list update: only the rows that changed since the
    /// last snapshot, for a link that opted into deltas
    /// ([replication client-link L1](../../replication/client-link/L1.md)). Replaces a whole `ViewReplace`
    /// for row-local changes (flags, reads, removals).
    ViewDelta {
        #[serde(rename = "linkSeq")]
        link_seq: RuntimeLinkSeq,
        #[serde(rename = "viewId")]
        view_id: ViewId,
        revision: ViewRevision,
        delta: MailListDelta,
    },
    ViewError {
        #[serde(rename = "linkSeq")]
        link_seq: RuntimeLinkSeq,
        #[serde(rename = "viewId")]
        view_id: ViewId,
        error: RuntimeAdapterError,
    },
    ViewClosed {
        #[serde(rename = "linkSeq")]
        link_seq: RuntimeLinkSeq,
        #[serde(rename = "viewId")]
        view_id: ViewId,
    },
    /// A verdict about a named client mutation, keyed to its **client** mutation
    /// id so the client correlates it to the optimistic op in its pending set. The
    /// authoritative state change (if any) arrives *separately* as a
    /// `message.updated` domain fact via [`Self::Notification`] — this frame is
    /// the verdict, not the data, and the two are never merged (facts come from
    /// the firehose uncorrelated; verdicts come from the command path keyed by
    /// id). A `Confirmed` retires the op by absorption when the base carries its
    /// effect (no revert); a `Rejected` reverts the optimism and surfaces the
    /// error. Replaces the former `MutationSettlement`
    /// ([mutation.notification design](../../eph/DESIGN-L2-mutation-notification.md)).
    MutationNotification {
        #[serde(rename = "linkSeq")]
        link_seq: RuntimeLinkSeq,
        #[serde(rename = "clientMutationId")]
        client_mutation_id: ClientMutationId,
        notification: MutationNotification,
    },
    Notification {
        #[serde(rename = "linkSeq")]
        link_seq: RuntimeLinkSeq,
        kind: String,
        #[cfg_attr(feature = "openapi", schema(value_type = Object))]
        payload: Value,
    },
    Heartbeat {
        #[serde(rename = "linkSeq")]
        link_seq: RuntimeLinkSeq,
    },
}

impl RuntimeFrame {
    pub fn link_seq(&self) -> RuntimeLinkSeq {
        match self {
            Self::ViewSnapshot { link_seq, .. }
            | Self::ViewReplace { link_seq, .. }
            | Self::ViewDelta { link_seq, .. }
            | Self::ViewError { link_seq, .. }
            | Self::ViewClosed { link_seq, .. }
            | Self::MutationNotification { link_seq, .. }
            | Self::Notification { link_seq, .. }
            | Self::Heartbeat { link_seq } => *link_seq,
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
/// link that declared [`view_delta`](RuntimeCallerCapabilities::view_delta),
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

/// Phase 2 undo/redo: the `revCursor` control-mutation args — an idempotent
/// cursor assignment the client sends after moving its optimistic cursor
/// locally (Phase 1). The server validates the referenced steps exist + applies
/// it to `rev_cursor`.
///
/// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevCursorArgs {
    pub account_id: String,
    /// The topmost APPLIED step (`None` = all undone). Must exist in `rev_log`.
    pub cursor_step_id: Option<String>,
    /// The undone step_ids above the cursor, in `seq` order. Each must exist.
    pub redo_tail: Vec<String>,
}

/// A forwarded operation on the link's up-channel. Carries the typed
/// [`MailOperation`] (D8) — the operation is parsed once at the wire edge and
/// travels typed inward; there is no stringly `name`/`args` pair to re-parse per
/// site. The wire shape is `{"linkId": …, "name": "message.…", "args": {…},
/// "clientMutationId": …, "context": …}` — the operation is flattened, so its
/// adjacently-tagged `name`/`args` sit at the envelope's top level exactly where
/// the old stringly fields were.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MutationRequest {
    pub link_id: Option<RuntimeLinkId>,
    #[serde(flatten)]
    pub operation: MailOperation,
    pub client_mutation_id: ClientMutationId,
    pub context: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MutationReceipt {
    pub runtime_mutation_id: Option<RuntimeMutationId>,
    pub client_mutation_id: ClientMutationId,
    /// The canonical operation name, echoed for the client's settlement join.
    /// Derived from the operation variant (one fact), never a free string.
    pub name: String,
    pub state: MutationSettlementState,
    pub error: Option<RuntimeAdapterError>,
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub output: Value,
}

/// The settlement-query response for one `(link, clientMutationId)` key
/// (`GET /runtime/links/{id}/mutations/{clientMutationId}`): the receipt the
/// runtime holds, or `null` when it has no record (unknown link, never
/// accepted, or already evicted/cleared under the D47 ledger rule). Consumed by
/// the near-end reconciler's sent-but-unsettled step (D44b): a terminal receipt
/// settles locally, `null` re-forwards.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMutationSettlement {
    pub receipt: Option<MutationReceipt>,
}

/// A terminal verdict about a named client mutation, carried by
/// [`RuntimeFrame::MutationNotification`] and keyed to the client mutation id.
/// The two outcomes are deliberately the only ones on the wire: `Confirmed` is
/// otherwise implicit in the base update (it serves the no-op-confirmation and
/// durable-pending-set-clear cases), and `Rejected` is the *only* signal a failed
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
    /// the link's live mutation cache once the client has had a chance to
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

/// Generate the one-line `RuntimeError` constructors that just pair a method
/// name with a code (mirrors the crate's `define_id!` idiom). The non-trivial
/// constructors (`new`, `with_details`, `retryable`, `internal`,
/// `compensation_failed`) stay hand-written above.
macro_rules! runtime_error_ctors {
    ($($name:ident => $code:ident),+ $(,)?) => {
        impl RuntimeError {
            $(
                pub fn $name(message: impl Into<String>) -> Self {
                    Self::new(RuntimeErrorCode::$code, message)
                }
            )+
        }
    };
}

runtime_error_ctors! {
    runtime_not_ready => RuntimeNotReady,
    invalid_descriptor => InvalidDescriptor,
    invalid_mutation => InvalidMutation,
    invalid_secret => InvalidSecret,
    invalid_account => InvalidAccount,
    account_base_url_required => AccountBaseUrlRequired,
    account_secret_required => AccountSecretRequired,
    account_username_required => AccountUsernameRequired,
    account_sender_required => AccountSenderRequired,
    unauthorized => Unauthorized,
    not_found => NotFound,
    provider_unavailable => ProviderUnavailable,
    conflict => Conflict,
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

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_domain_model::StoreError;

    #[test]
    fn service_error_conversion_preserves_runtime_error_code() {
        let error = RuntimeError::from(ServiceError::from(StoreError::NotFound(
            "account missing".to_string(),
        )));

        assert_eq!(error.envelope().code, RuntimeErrorCode::NotFound);
    }

    #[test]
    fn mutation_request_flattens_the_operation_to_the_envelope_top_level() {
        // The wire keeps the flat `{linkId, name, args, clientMutationId,
        // context}` shape: the flattened adjacently-tagged operation surfaces
        // `name`/`args` at the top level exactly where the old stringly fields
        // were, and round-trips back into a typed operation.
        let wire = serde_json::json!({
            "linkId": "sess-1",
            "name": "message.replaceMailboxes",
            "args": { "sourceId": "acct", "messageId": "m1", "mailboxIds": ["inbox"] },
            "clientMutationId": "op-1",
            "context": null
        });
        let request: MutationRequest = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(request.operation.name(), "message.replaceMailboxes");
        assert_eq!(request.operation.account_id(), "acct");
        assert_eq!(serde_json::to_value(&request).unwrap(), wire);
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
            link_seq: RuntimeLinkSeq::new(2),
            view_id: ViewId::new("view-1"),
            revision: ViewRevision::new(7),
            snapshot,
        })
        .expect("frame should serialize");

        assert_eq!(serialized["type"], "viewReplace");
        assert_eq!(serialized["linkSeq"], 2);
        assert_eq!(serialized["viewId"], "view-1");
        assert!(serialized.get("link_seq").is_none());
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
