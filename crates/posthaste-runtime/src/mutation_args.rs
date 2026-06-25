//! Mutation argument shapes + parsing shared by the runtime mutation pipeline.
//!
//! These deserialize the `args` of a [`MutationRequest`] for each message-state
//! mutation and live on the **lean** side of the build seam: the runtime handle
//! parses them before forwarding the typed command over the backend link, so a
//! lean near node (no `backend` feature, no in-process `Backend`) still needs
//! them. They were factored out of `backend.rs` for exactly this reason.

use posthaste_domain::SetKeywordsCommand;
use posthaste_runtime_contract::{MutationRequest, RuntimeError};
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSetKeywordsMutationArgs {
    pub source_id: String,
    pub message_id: String,
    pub command: SetKeywordsCommand,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSetReadStateArgs {
    pub source_id: String,
    pub message_id: String,
    pub read: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSetFlaggedStateArgs {
    pub source_id: String,
    pub message_id: String,
    pub flagged: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSetUserTagsArgs {
    pub source_id: String,
    pub message_id: String,
    #[serde(default)]
    pub add: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageMoveToMailboxArgs {
    pub source_id: String,
    pub message_id: String,
    pub mailbox_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageMoveToRoleArgs {
    pub source_id: String,
    pub message_id: String,
    pub role: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageReplaceMailboxesArgs {
    pub source_id: String,
    pub message_id: String,
    pub mailbox_ids: Vec<String>,
}

/// A message mutation that targets one message by id (archive/trash/destroy).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageTargetArgs {
    pub source_id: String,
    pub message_id: String,
}

pub fn parse_args<T>(request: &MutationRequest) -> Result<T, RuntimeError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(request.args.clone()).map_err(|error| {
        RuntimeError::with_details(
            posthaste_runtime_contract::RuntimeErrorCode::InvalidMutation,
            format!("invalid args for mutation '{}'", request.name),
            serde_json::json!({ "error": error.to_string() }),
        )
    })
}
