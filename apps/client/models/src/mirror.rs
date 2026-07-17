//! TypeScript-shape twins of the domain-model types the wire reuses.
//!
//! The domain crate does not derive `ts_rs::TS`, so each reused type gets a
//! twin here that declares the identical serde shape and derives `TS`. Wire
//! structs keep their fields typed with the REAL domain types (the backend
//! serializes domain values untouched) and point ts-rs at these twins with
//! `#[ts(as = ...)]`.
//!
//! Drift protection: every struct here is `deny_unknown_fields`, and
//! `tests/mirror_drift.rs` serializes fully-populated domain values and
//! strictly decodes them into these twins — a renamed, removed, or added
//! domain field fails the test (or the exhaustive literals in it stop
//! compiling). These types are for TS generation and drift checks only; the
//! backend never constructs them.

use std::collections::BTreeMap;

use serde::Deserialize;
use ts_rs::TS;

/// Declares a TypeScript alias twin (`type Name = string`) for a domain
/// string-id newtype.
macro_rules! mirror_string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Deserialize, TS)]
        pub struct $name(pub String);
    };
}

mirror_string_id!(
    /// Twin of [`posthaste_domain_model::AccountId`].
    AccountId
);
mirror_string_id!(
    /// Twin of [`posthaste_domain_model::MailboxId`].
    MailboxId
);
mirror_string_id!(
    /// Twin of [`posthaste_domain_model::MessageId`].
    MessageId
);
mirror_string_id!(
    /// Twin of [`posthaste_domain_model::ThreadId`].
    ThreadId
);
mirror_string_id!(
    /// Twin of [`posthaste_domain_model::ConversationId`].
    ConversationId
);
mirror_string_id!(
    /// Twin of [`posthaste_domain_model::BlobId`].
    BlobId
);
mirror_string_id!(
    /// Twin of [`posthaste_domain_model::OperationId`].
    OperationId
);
mirror_string_id!(
    /// Twin of [`posthaste_domain_model::SmartMailboxId`].
    SmartMailboxId
);

/// Twin of [`posthaste_domain_model::Recipient`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Recipient {
    pub name: Option<String>,
    pub email: String,
}

/// Twin of [`posthaste_domain_model::MessageSummary`] — the list-row
/// projection.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageSummary {
    pub id: MessageId,
    pub source_id: AccountId,
    pub source_name: String,
    pub source_thread_id: ThreadId,
    pub conversation_id: ConversationId,
    pub subject: Option<String>,
    pub from_name: Option<String>,
    pub from_email: Option<String>,
    pub to: Vec<Recipient>,
    pub preview: Option<String>,
    pub received_at: String,
    pub has_attachment: bool,
    pub is_read: bool,
    pub is_flagged: bool,
    pub mailbox_ids: Vec<MailboxId>,
    pub keywords: Vec<String>,
    #[serde(default)]
    #[ts(optional, type = "number")]
    pub version: Option<u64>,
    #[serde(default)]
    #[ts(optional)]
    pub rfc_message_id: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub draft_id: Option<String>,
}

/// Twin of [`posthaste_domain_model::MailboxSummary`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MailboxSummary {
    pub id: MailboxId,
    pub name: String,
    pub role: Option<String>,
    #[ts(type = "number")]
    pub unread_emails: i64,
    #[ts(type = "number")]
    pub total_emails: i64,
}

/// Twin of [`posthaste_domain_model::MessageAttachment`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageAttachment {
    pub id: String,
    pub blob_id: BlobId,
    pub part_id: Option<String>,
    pub filename: Option<String>,
    pub mime_type: String,
    #[ts(type = "number")]
    pub size: i64,
    pub disposition: Option<String>,
    pub cid: Option<String>,
    pub is_inline: bool,
}

/// Twin of [`posthaste_domain_model::ListUnsubscribe`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListUnsubscribe {
    #[serde(default)]
    #[ts(optional)]
    pub https: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub mailto: Option<String>,
    #[serde(default)]
    pub one_click: bool,
}

/// Twin of [`posthaste_domain_model::ThreadView`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadView {
    pub id: ThreadId,
    pub messages: Vec<MessageSummary>,
}

/// Twin of [`posthaste_domain_model::MessageSortField`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum MessageSortField {
    Date,
    From,
    Subject,
    Source,
    Flagged,
    Attachment,
}

/// Twin of [`posthaste_domain_model::SetKeywordsCommand`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetKeywordsCommand {
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

/// Twin of [`posthaste_domain_model::ReplaceMailboxesCommand`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceMailboxesCommand {
    pub mailbox_ids: Vec<MailboxId>,
}

/// Twin of [`posthaste_domain_model::SendMessageAttachment`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendMessageAttachment {
    pub filename: String,
    pub mime_type: String,
    pub content_base64: String,
}

/// Twin of [`posthaste_domain_model::SendMessageRequest`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendMessageRequest {
    pub from: Option<Recipient>,
    pub to: Vec<Recipient>,
    pub cc: Vec<Recipient>,
    pub bcc: Vec<Recipient>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    #[serde(default)]
    pub attachments: Vec<SendMessageAttachment>,
    #[serde(default)]
    pub draft_id: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub send_at: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub undo_window_seconds: Option<u32>,
}

/// Twin of [`posthaste_domain_model::OperationKind`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum OperationKind {
    SetKeywords,
    ReplaceMailboxes,
    Destroy,
    DraftCreate,
    DraftUpdate,
    DraftDelete,
    Send,
}

/// Twin of [`posthaste_domain_model::OperationState`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum OperationState {
    Pending,
    Inflight,
    Applied,
    Failed,
    DispatchUncertain,
}

/// Twin of [`posthaste_domain_model::OperationEntityKind`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum OperationEntityKind {
    Message,
    Draft,
}

/// Twin of [`posthaste_domain_model::AccountStatus`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AccountStatus {
    Ready,
    Syncing,
    Degraded,
    AuthError,
    Offline,
    Disabled,
}

/// Twin of [`posthaste_domain_model::PushStatus`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PushStatus {
    Connected,
    Reconnecting,
    Unsupported,
    Disabled,
}

// ------------------------------------------------------ mail-query AST twins

/// Twin of [`posthaste_domain_model::MailQueryGroupOperator`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum MailQueryGroupOperator {
    All,
    Any,
}

/// Twin of [`posthaste_domain_model::MailQueryField`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum MailQueryField {
    SourceId,
    SourceName,
    MessageId,
    ThreadId,
    ConversationId,
    MailboxId,
    MailboxName,
    MailboxRole,
    IsRead,
    IsFlagged,
    HasAttachment,
    Keyword,
    FromName,
    FromEmail,
    To,
    Subject,
    Preview,
    Body,
    ReceivedAt,
    Size,
}

/// Twin of [`posthaste_domain_model::MailQueryOperator`]. The domain accepts
/// legacy aliases for the ordered comparisons on decode but always serializes
/// the names below, which are therefore the whole TS-visible vocabulary.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum MailQueryOperator {
    Equals,
    In,
    Contains,
    BeginsWith,
    EndsWith,
    Regex,
    Lt,
    Gt,
    Le,
    Ge,
}

/// Twin of [`posthaste_domain_model::DateUnit`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum DateUnit {
    Minutes,
    Hours,
    Days,
    Weeks,
    Months,
}

/// Twin of [`posthaste_domain_model::DateValue`].
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DateValue {
    Absolute { value: String },
    Relative { amount: u32, unit: DateUnit },
}

/// Twin of [`posthaste_domain_model::MailQueryValue`] (untagged: each
/// variant is distinguished by its JSON shape).
#[derive(Debug, Deserialize, TS)]
#[serde(untagged)]
pub enum MailQueryValue {
    String(String),
    Strings(Vec<String>),
    Bool(bool),
    Date(DateValue),
}

/// Twin of [`posthaste_domain_model::MailQueryCondition`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MailQueryCondition {
    pub field: MailQueryField,
    pub operator: MailQueryOperator,
    pub negated: bool,
    pub value: MailQueryValue,
}

/// Twin of [`posthaste_domain_model::MailQueryGroup`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MailQueryGroup {
    pub operator: MailQueryGroupOperator,
    pub negated: bool,
    pub nodes: Vec<MailQueryRuleNode>,
}

/// Twin of [`posthaste_domain_model::MailQueryRuleNode`].
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MailQueryRuleNode {
    Group(MailQueryGroup),
    Condition(MailQueryCondition),
}

/// Twin of [`posthaste_domain_model::MailQueryRule`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MailQueryRule {
    pub root: MailQueryGroup,
}

// -------------------------------------------------- smart-mailbox/tag twins

/// Twin of [`posthaste_domain_model::SmartMailboxKind`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SmartMailboxKind {
    Default,
    User,
}

/// Twin of [`posthaste_domain_model::TagSummary`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TagSummary {
    pub name: String,
    #[ts(type = "number")]
    pub unread_messages: i64,
    #[ts(type = "number")]
    pub total_messages: i64,
}

// --------------------------------------------------------- settings twins

/// Twin of [`posthaste_domain_model::CachePolicy`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CachePolicy {
    #[ts(type = "number")]
    pub soft_cap_bytes: u64,
    #[ts(type = "number")]
    pub hard_cap_bytes: u64,
    pub cache_bodies: bool,
    pub cache_raw_messages: bool,
    pub cache_attachments: bool,
}

/// Twin of [`posthaste_domain_model::AutomationTrigger`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AutomationTrigger {
    MessageArrived,
    MessageChanged,
    Manual,
}

/// Twin of [`posthaste_domain_model::AutomationAction`].
#[derive(Debug, Deserialize, TS)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum AutomationAction {
    ApplyTag { tag: String },
    RemoveTag { tag: String },
    MarkRead,
    MarkUnread,
    Flag,
    Unflag,
    MoveToMailbox { mailbox_id: MailboxId },
}

/// Twin of [`posthaste_domain_model::AutomationRule`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub triggers: Vec<AutomationTrigger>,
    pub condition: MailQueryRule,
    pub actions: Vec<AutomationAction>,
    pub backfill: bool,
}

/// Twin of [`posthaste_domain_model::ThemeMode`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

/// Twin of [`posthaste_domain_model::UiDensity`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum UiDensity {
    Compact,
    Cozy,
    Comfortable,
}

/// Twin of [`posthaste_domain_model::ThemeColors`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeColors {
    pub accent_hue: Option<u32>,
    pub surface_hue: Option<u32>,
    #[serde(default)]
    pub tokens: BTreeMap<String, String>,
}

/// Twin of [`posthaste_domain_model::GlassBloom`].
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct GlassBloom {
    pub id: String,
    pub hue: u32,
    pub x: f64,
    pub y: f64,
    pub opacity: f64,
    pub radius: f64,
}

/// Twin of [`posthaste_domain_model::GlassTheme`].
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct GlassTheme {
    pub blooms: Vec<GlassBloom>,
}

/// Twin of [`posthaste_domain_model::Appearance`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Appearance {
    pub mode: Option<ThemeMode>,
    pub theme: Option<String>,
    pub density: Option<UiDensity>,
    pub light: Option<ThemeColors>,
    pub dark: Option<ThemeColors>,
    pub glass_theme: Option<GlassTheme>,
}

/// Twin of [`posthaste_domain_model::Notifications`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Notifications {
    pub new_mail: Option<bool>,
    pub sound: Option<bool>,
}

/// Twin of [`posthaste_domain_model::MailboxColor`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MailboxColor {
    pub source_id: AccountId,
    pub mailbox_id: MailboxId,
    pub hue: u32,
}

/// Twin of [`posthaste_domain_model::TagAppearance`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TagAppearance {
    pub name: String,
    #[serde(default)]
    #[ts(optional)]
    pub fg: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub bg: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub icon: Option<String>,
}

/// Twin of [`posthaste_domain_model::MailboxGroup`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MailboxGroup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub mailbox_ids: Vec<String>,
    #[ts(type = "number")]
    pub order: i64,
}

/// Twin of [`posthaste_domain_model::ComposeSettings`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComposeSettings {
    #[serde(default)]
    #[ts(optional)]
    pub undo_send_delay_seconds: Option<u32>,
}

/// Twin of [`posthaste_domain_model::AppSettings`] — the whole settings
/// document as served by the settings family.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppSettings {
    pub default_account_id: Option<AccountId>,
    pub cache_policy: CachePolicy,
    #[serde(default)]
    pub automation_rules: Vec<AutomationRule>,
    #[serde(default)]
    pub automation_drafts: Vec<AutomationRule>,
    #[serde(default)]
    pub appearance: Option<Appearance>,
    #[serde(default)]
    pub notifications: Option<Notifications>,
    #[serde(default)]
    pub mailbox_colors: Vec<MailboxColor>,
    #[serde(default)]
    pub tags: Vec<TagAppearance>,
    #[serde(default)]
    pub smart_mailbox_order: Vec<SmartMailboxId>,
    #[serde(default)]
    pub account_order: Vec<AccountId>,
    #[serde(default)]
    pub mailbox_groups: Vec<MailboxGroup>,
    #[serde(default)]
    pub compose: Option<ComposeSettings>,
}

// ---------------------------------------------------- account-config twins

/// Twin of [`posthaste_domain_model::AccountDriver`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AccountDriver {
    Jmap,
    ImapSmtp,
    Mock,
}

/// Twin of [`posthaste_domain_model::ProviderHint`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProviderHint {
    Generic,
    Gmail,
    Outlook,
    Icloud,
}

/// Twin of [`posthaste_domain_model::ProviderAuthKind`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProviderAuthKind {
    Password,
    AppPassword,
    #[serde(rename = "oauth2")]
    OAuth2,
}

/// Twin of [`posthaste_domain_model::TransportSecurity`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TransportSecurity {
    Tls,
    StartTls,
    Plain,
}

/// Twin of [`posthaste_domain_model::ImapTransportSettings`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImapTransportSettings {
    pub host: String,
    pub port: u16,
    pub security: TransportSecurity,
}

/// Twin of [`posthaste_domain_model::SmtpTransportSettings`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SmtpTransportSettings {
    pub host: String,
    pub port: u16,
    pub security: TransportSecurity,
}

/// Twin of [`posthaste_domain_model::SecretKind`] — where a secret is
/// stored, never the secret itself.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SecretKind {
    Env,
    Os,
}

/// Twin of [`posthaste_domain_model::SecretStatus`] — the redacted
/// credential state the read side serves. It carries no material and no
/// lookup key.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SecretStatus {
    pub storage: SecretKind,
    pub configured: bool,
    pub label: Option<String>,
}

/// Twin of [`posthaste_domain_model::AccountAppearance`].
#[derive(Debug, Deserialize, TS)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum AccountAppearance {
    Initials {
        initials: String,
        color_hue: u16,
    },
    Image {
        image_id: String,
        initials: String,
        color_hue: u16,
    },
}

// ------------------------------------------------------- sync/rev-log twins

/// Twin of [`posthaste_domain_model::SyncMode`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SyncMode {
    Incremental,
    FullMetadata,
}

/// Twin of [`posthaste_domain_model::RevLogStep`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevLogStep {
    pub step_id: String,
    pub seq: u32,
    pub message_id: String,
    pub source_id: String,
    /// The step's message-change diff, opaque at this layer.
    #[ts(type = "unknown")]
    pub diff: serde_json::Value,
    pub created_at: String,
}

/// Twin of [`posthaste_domain_model::RevCursor`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevCursor {
    pub cursor_step_id: Option<String>,
    pub redo_tail: Vec<String>,
}
