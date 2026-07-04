//! Mutation argument shapes + parsing shared by the runtime mutation pipeline.
//!
//! These deserialize the `args` of a [`MutationRequest`] for each message-state
//! mutation and live on the **lean** side of the build seam: the runtime handle
//! parses them before forwarding the typed command over the authority server link, so a
//! lean near node (no in-process `AuthorityServer`) still needs
//! them. They were factored out of `authority server.rs` for exactly this reason.

use crate::{RuntimeError, RuntimeErrorCode};
use posthaste_domain_model::{SendMessageRequest, SetKeywordsCommand};
use posthaste_replica_core::MessageChangeDiff;
use serde::{Deserialize, Serialize};

/// Build a single-keyword add/remove command from a desired presence. Shared by
/// the authority server's read-state/flagged-state application and the runtime's history
/// capture for the same mutations.
pub fn keyword_toggle(keyword: &str, present: bool) -> SetKeywordsCommand {
    if present {
        SetKeywordsCommand {
            add: vec![keyword.to_string()],
            remove: Vec::new(),
        }
    } else {
        SetKeywordsCommand {
            add: Vec::new(),
            remove: vec![keyword.to_string()],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSetKeywordsMutationArgs {
    pub source_id: String,
    pub message_id: String,
    pub command: SetKeywordsCommand,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSetReadStateArgs {
    pub source_id: String,
    pub message_id: String,
    pub read: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSetFlaggedStateArgs {
    pub source_id: String,
    pub message_id: String,
    pub flagged: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSetUserTagsArgs {
    pub source_id: String,
    pub message_id: String,
    #[serde(default)]
    pub add: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageMoveToMailboxArgs {
    pub source_id: String,
    pub message_id: String,
    pub mailbox_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageMoveToRoleArgs {
    pub source_id: String,
    pub message_id: String,
    pub role: String,
}

/// `message.addToMailbox` / `message.removeFromMailbox`: add or remove a message
/// from a single mailbox (a membership *delta*, unlike the full-replace
/// `ReplaceMailboxes`). These are the typed form of the REST per-command
/// `add-to-mailbox`/`remove-from-mailbox` surface; they carry no named-mutation
/// forward path (the client sends them through the direct REST command route).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageMailboxMembershipArgs {
    pub source_id: String,
    pub message_id: String,
    pub mailbox_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageReplaceMailboxesArgs {
    pub source_id: String,
    pub message_id: String,
    pub mailbox_ids: Vec<String>,
}

/// A message mutation that targets one message by id (archive/trash/destroy).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageTargetArgs {
    pub source_id: String,
    pub message_id: String,
}

/// `message.deleteDraft`: discard a draft through the optimistic runtime-mutation
/// path (D130). `message_id` is the visible list-row id the client folds the
/// optimistic destroy on (the blink); `draft_id` is the stable
/// `X-Posthaste-Draft-Id` (D131) the gateway resolves to the current live Email
/// id, so the discard survives the id rotation a JMAP autosave causes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDeleteDraftArgs {
    pub source_id: String,
    pub message_id: String,
    pub draft_id: String,
}

/// `message.saveDraft`: save (create or update) a draft through the optimistic
/// runtime-mutation path (M65/D130) rather than the fire-and-forget REST POST.
/// `message_id` is the stable draft key (D131) the far node uses as the
/// `draft_id` for the create/update; `request` is the full compose payload. The
/// operation is **not** locally foldable (the fold vocabulary has no upsert and
/// carries no draft content — see [`crate::MailOperation::fold_effect`]); its
/// value is the typed, idempotent path + the reconciling `message.updated`
/// emitted on the draft settlement (D132), not an optimistic fold.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSaveDraftArgs {
    pub source_id: String,
    pub message_id: String,
    pub request: SendMessageRequest,
}

/// `message.send`: send a message through the optimistic runtime-mutation path
/// (M66/D130). `message_id` is the originating draft's row id: the optimistic
/// fold is a `Destroy` on it ("it left Drafts"), reverted if the send parks
/// (D125) or fails, confirmed on a real ack. `request` is the full compose
/// payload (its `draft_id` names the draft the send consumes on ack, D126). A
/// fresh compose that was never saved uses its client draft key here — the
/// `Destroy` is then a deferred no-op (no base row), which is harmless.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSendArgs {
    pub source_id: String,
    pub message_id: String,
    pub request: SendMessageRequest,
}

/// `message.snooze`: move a message to the Snoozed mailbox + record the return
/// time. `until` is unix seconds (UTC). @spec docs/eph/DESIGN-L2-snooze
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSnoozeArgs {
    pub source_id: String,
    pub message_id: String,
    pub until: i64,
}

/// `message.unsnooze`: move a snoozed message back to the Inbox + clear its
/// return time. @spec docs/eph/DESIGN-L2-snooze
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageUnsnoozeArgs {
    pub source_id: String,
    pub message_id: String,
}

/// `message.applyDiff`: apply an invertible change-diff (add/remove keywords +
/// add/remove mailboxes) to one message. The undo/redo vehicle — undo submits
/// `inverse(diff)`, redo submits `diff` — and an ordinary optimistic mutation
/// through the pending set + replay guard. Undo/redo history is client-owned
/// (@spec docs/eph/DESIGN-L2-undo-redo-synced-history), so the runtime applies
/// the diff without navigating or recording any history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageApplyDiffArgs {
    pub source_id: String,
    pub message_id: String,
    pub diff: MessageChangeDiff,
}

/// Deserialize a message-mutation's `args` value into its typed argument struct.
/// Used by [`crate::MailOperation`]'s legacy `{name, args}` construction helper
/// and by tests; the wire itself parses the whole operation in one serde pass.
pub fn parse_args<T>(name: &str, args: &serde_json::Value) -> Result<T, RuntimeError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(args.clone()).map_err(|error| {
        RuntimeError::with_details(
            RuntimeErrorCode::InvalidMutation,
            format!("invalid args for mutation '{name}'"),
            serde_json::json!({ "error": error.to_string() }),
        )
    })
}
