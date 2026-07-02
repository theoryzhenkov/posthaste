//! Mutation argument shapes + parsing shared by the runtime mutation pipeline.
//!
//! These deserialize the `args` of a [`MutationRequest`] for each message-state
//! mutation and live on the **lean** side of the build seam: the runtime handle
//! parses them before forwarding the typed command over the authority server link, so a
//! lean near node (no in-process `AuthorityServer`) still needs
//! them. They were factored out of `authority server.rs` for exactly this reason.

use crate::{RuntimeError, RuntimeErrorCode};
use posthaste_domain_model::SetKeywordsCommand;
use posthaste_link_core::MessageChangeDiff;
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
/// through the outbox + replay guard. Undo/redo history is client-owned
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
