//! Mutation argument shapes + parsing shared by the runtime mutation pipeline.
//!
//! These deserialize the `args` of a [`MutationRequest`] for each message-state
//! mutation and live on the **lean** side of the build seam: the runtime handle
//! parses them before forwarding the typed command over the backend link, so a
//! lean near node (no `backend` feature, no in-process `Backend`) still needs
//! them. They were factored out of `backend.rs` for exactly this reason.

use crate::{MutationRequest, RuntimeError, RuntimeErrorCode, RuntimeSessionSeq};
use posthaste_domain::SetKeywordsCommand;
use posthaste_link_core::MessageChangeDiff;
use serde::Deserialize;

/// Build a single-keyword add/remove command from a desired presence. Shared by
/// the backend's read-state/flagged-state application and the runtime's history
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSetKeywordsMutationArgs {
    pub source_id: String,
    pub message_id: String,
    pub command: SetKeywordsCommand,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSetReadStateArgs {
    pub source_id: String,
    pub message_id: String,
    pub read: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSetFlaggedStateArgs {
    pub source_id: String,
    pub message_id: String,
    pub flagged: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSetUserTagsArgs {
    pub source_id: String,
    pub message_id: String,
    #[serde(default)]
    pub add: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageMoveToMailboxArgs {
    pub source_id: String,
    pub message_id: String,
    pub mailbox_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageMoveToRoleArgs {
    pub source_id: String,
    pub message_id: String,
    pub role: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageReplaceMailboxesArgs {
    pub source_id: String,
    pub message_id: String,
    pub mailbox_ids: Vec<String>,
}

/// A message mutation that targets one message by id (archive/trash/destroy).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageTargetArgs {
    pub source_id: String,
    pub message_id: String,
}

/// `message.applyDiff`: apply an invertible change-diff (add/remove keywords +
/// add/remove mailboxes) to one message. The undo/redo vehicle — undo submits
/// `inverse(diff)` with `undoOf`, redo submits `diff` with `redoOf`. Execution is
/// an ordinary optimistic mutation through the outbox + replay guard; the
/// `undoOf`/`redoOf` seq hints are history-bookkeeping metadata the runtime uses
/// to navigate its own seq-ordered diff history (they do not gate execution).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageApplyDiffArgs {
    pub source_id: String,
    pub message_id: String,
    pub diff: MessageChangeDiff,
    #[serde(default)]
    pub undo_of: Option<RuntimeSessionSeq>,
    #[serde(default)]
    pub redo_of: Option<RuntimeSessionSeq>,
}

pub fn parse_args<T>(request: &MutationRequest) -> Result<T, RuntimeError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(request.args.clone()).map_err(|error| {
        RuntimeError::with_details(
            RuntimeErrorCode::InvalidMutation,
            format!("invalid args for mutation '{}'", request.name),
            serde_json::json!({ "error": error.to_string() }),
        )
    })
}
