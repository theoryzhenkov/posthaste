//! The write side of the protocol: one typed [`Command`] enum posted to
//! `POST /command` inside [`CommandEnvelope`], and the acceptance reply —
//! one submodule per family.
//!
//! Mail variants carry the same intent vocabulary the outbox stores; the
//! payload types are the domain's own (`SetKeywordsCommand`,
//! `ReplaceMailboxesCommand`, `SendMessageRequest`), so the backend decodes
//! straight into the types it enqueues. A rejection is the API error
//! envelope ([`crate::ApiError`]), not a third shape.
//!
//! Secret material travels ONLY in [`Command::SetAccountSecret`] — the one
//! dedicated secret-bearing command. No other command, and no settings
//! payload, carries a credential.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

mod account;
mod account_secret;
mod automation;
mod draft;
mod mail;
mod mailbox;
mod operation;
mod rev_log;
mod send;
mod settings;
mod smart_mailbox;
mod snooze;
mod sync;
mod unsubscribe;

pub use account::*;
pub use account_secret::*;
pub use automation::*;
pub use draft::*;
pub use mail::*;
pub use mailbox::*;
pub use operation::*;
pub use rev_log::*;
pub use send::*;
pub use settings::*;
pub use smart_mailbox::*;
pub use snooze::*;
pub use sync::*;
pub use unsubscribe::*;

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
    /// details are configured through `updateAccountTransport` and
    /// `setAccountSecret`.
    CreateAccount(CreateAccountIntent),
    /// Update an account's identity/appearance settings; absent fields are
    /// preserved. Enabling/disabling is this command's `enabled` field.
    UpdateAccount(UpdateAccountIntent),
    /// Update an account's transport endpoints (secrets-safe: this shape
    /// cannot carry a credential).
    UpdateAccountTransport(UpdateAccountTransportIntent),
    /// THE dedicated secret-bearing command: set, replace, or clear an
    /// account's stored credential.
    SetAccountSecret(SetAccountSecretIntent),
    /// Delete an account and its local data.
    DeleteAccount(DeleteAccountIntent),
    /// Upload an account logo image (base64, like compose attachments).
    SetAccountLogo(SetAccountLogoIntent),
    /// Complete an OAuth authorization started by the `oauthStart` query,
    /// handing back the provider redirect.
    CompleteOauth(CompleteOauthIntent),
    /// Replace the global settings document (read-modify-write against the
    /// `appSettings` query).
    UpdateSettings(UpdateSettingsIntent),
    /// Create a user smart mailbox.
    CreateSmartMailbox(CreateSmartMailboxIntent),
    /// Update a smart mailbox's name, role, or rule; absent fields are
    /// preserved.
    UpdateSmartMailbox(UpdateSmartMailboxIntent),
    /// Delete a smart mailbox.
    DeleteSmartMailbox(DeleteSmartMailboxIntent),
    /// Restore the built-in smart mailboxes to their defaults.
    ResetSmartMailboxes(ResetSmartMailboxesIntent),
    /// Create a top-level provider mailbox.
    CreateMailbox(CreateMailboxIntent),
    /// Rename a provider mailbox.
    RenameMailbox(RenameMailboxIntent),
    /// Delete a provider mailbox (refused non-empty unless the intent says
    /// otherwise).
    DeleteMailbox(DeleteMailboxIntent),
    /// Assign or clear a mailbox's role (inbox, archive, snooze, ...).
    SetMailboxRole(SetMailboxRoleIntent),
    /// Create an automation rule.
    CreateAutomationRule(CreateAutomationRuleIntent),
    /// Replace an automation rule, keyed by the rule's id.
    UpdateAutomationRule(UpdateAutomationRuleIntent),
    /// Delete an automation rule.
    DeleteAutomationRule(DeleteAutomationRuleIntent),
    /// Snooze a message until a wall-clock time (the backend returns it to
    /// the inbox when due).
    Snooze(SnoozeIntent),
    /// Return a snoozed message to the inbox now.
    Unsnooze(UnsnoozeIntent),
    /// Undo the account's most recent reversible operation (moves the
    /// rev-log cursor down).
    Undo(UndoIntent),
    /// Re-apply the most recently undone operation (moves the rev-log cursor
    /// up).
    Redo(RedoIntent),
    /// Trigger a sync cycle for one account now.
    SyncNow(SyncNowIntent),
    /// Re-queue a failed or parked outbox operation.
    RetryOperation(RetryOperationIntent),
    /// Cancel a pending outbox operation (including a held send inside its
    /// undo window).
    CancelOperation(CancelOperationIntent),
    /// Execute a message's RFC 8058 one-click unsubscribe, backend-side.
    Unsubscribe(UnsubscribeIntent),
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
