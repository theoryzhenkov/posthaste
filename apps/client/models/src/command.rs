//! The write side of the protocol: one typed [`Command`] enum posted to
//! `POST /command` inside [`CommandEnvelope`], and the acceptance reply.
//!
//! Mail variants carry the same intent vocabulary the outbox stores; the
//! payload types are the domain's own (`SetKeywordsCommand`,
//! `ReplaceMailboxesCommand`, `SendMessageRequest`), so the backend decodes
//! straight into the types it enqueues. A rejection is the API error
//! envelope ([`crate::ApiError`]), not a third shape.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The envelope for every write: a client-generated idempotency id and the
/// intent. Retrying the same id is safe — the command applies once and the
/// replay returns the original outcome.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CommandEnvelope {
    /// Client-generated idempotency id (e.g. a ULID/UUID string).
    pub id: String,
    pub command: Command,
}

/// One write intent — every command the API accepts. Externally tagged, so
/// the wire shape is `{ "setKeywords": { ... } }`.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Command {
    /// Add/remove keywords (read, flagged, ...) on a message.
    SetKeywords(SetKeywordsIntent),
    /// Atomically replace a message's mailbox memberships (move, archive,
    /// label).
    ReplaceMailboxes(ReplaceMailboxesIntent),
    /// Permanently destroy a message.
    Destroy(DestroyMessageIntent),
    /// Create a draft; the draft id is client-minted and stays stable across
    /// provider id rotation.
    CreateDraft(CreateDraftIntent),
    /// Update an existing draft in place, keyed by its stable draft id.
    UpdateDraft(UpdateDraftIntent),
    /// Discard a draft, keyed by its stable draft id.
    DiscardDraft(DiscardDraftIntent),
    /// Submit a message for delivery (immediately, or held/scheduled through
    /// the request's hold fields).
    Send(SendIntent),
    /// Create a mail account with minimal identity settings; connection
    /// details are configured through the settings surface.
    CreateAccount(CreateAccountIntent),
    /// Update an account's minimal identity settings; absent fields are
    /// preserved.
    UpdateAccount(UpdateAccountIntent),
}

/// Target + keyword change for [`Command::SetKeywords`].
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SetKeywordsIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::MessageId")]
    pub message_id: domain::MessageId,
    #[ts(as = "crate::mirror::SetKeywordsCommand")]
    pub change: domain::SetKeywordsCommand,
}

/// Target + mailbox replacement for [`Command::ReplaceMailboxes`].
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceMailboxesIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::MessageId")]
    pub message_id: domain::MessageId,
    #[ts(as = "crate::mirror::ReplaceMailboxesCommand")]
    pub change: domain::ReplaceMailboxesCommand,
}

/// Target for [`Command::Destroy`].
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DestroyMessageIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::MessageId")]
    pub message_id: domain::MessageId,
}

/// Content for [`Command::CreateDraft`]. The draft's stable id travels in
/// `draft.draftId`.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreateDraftIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::SendMessageRequest")]
    pub draft: domain::SendMessageRequest,
}

/// Target + content for [`Command::UpdateDraft`].
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDraftIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    /// The stable draft id (survives provider id rotation).
    pub draft_id: String,
    #[ts(as = "crate::mirror::SendMessageRequest")]
    pub draft: domain::SendMessageRequest,
}

/// Target for [`Command::DiscardDraft`].
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DiscardDraftIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    /// The stable draft id (survives provider id rotation).
    pub draft_id: String,
}

/// Content for [`Command::Send`]. Hold semantics (undo-send window,
/// send-later time, originating draft) travel inside the request itself.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SendIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::SendMessageRequest")]
    pub request: domain::SendMessageRequest,
}

/// Minimal account creation for [`Command::CreateAccount`].
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountIntent {
    /// Display name for the account.
    pub name: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub full_name: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub signature: Option<String>,
    /// Email address patterns owned by this account.
    #[serde(default)]
    pub email_patterns: Vec<String>,
    /// Whether the account starts enabled; the backend default applies when
    /// absent.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub enabled: Option<bool>,
}

/// Minimal account patch for [`Command::UpdateAccount`]; absent fields are
/// preserved.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAccountIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub name: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub full_name: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub signature: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub email_patterns: Option<Vec<String>>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub enabled: Option<bool>,
}

/// The acceptance reply for a command: recorded and visible at this
/// generation. A query answered at or past it shows the command's local
/// effect; the provider verdict arrives later as pending-operations state.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CommandAccepted {
    #[ts(type = "number")]
    pub generation: u64,
}
